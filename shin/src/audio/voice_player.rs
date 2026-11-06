//! Character voices, as the original's `VoicePlayer` (play `0x710011a0f0`, update `0x710011ab70`, stop
//! `0x710011a6b0`, status `0x710011a980`).
//!
//! A line's voice spec names one file or several separated by `|`, each `/voice/<name>.nxa`; they start together.
//! Each voice keeps its own volume, played at `v2 / 100 x volume`; a stop fades it by 0.2 per frame, so a full-volume
//! voice fades over 5 frames. With the muting flag, a file whose speakers (the scenario's voice mapping) are all
//! switched off in setting v19 is skipped (`0x71000eadc0`).
//!
//! Lip-sync: a bustup whose character speaks in a playing voice opens its mouth by the voice's level 0-2
//! (`ADV::vfunc_63` `0x7100014eb0`: the first playing voice whose speakers include the character). The level comes
//! from a table of 50 ms windows made from the decoded samples (`0x71000f55d0`), read at the playing position
//! (`StreamSound::lipLevel` `0x71000f54e0`).
//!
//! Panning: every frame each such bustup pans the voices its character speaks in to its place on screen when
//! setting v5 is on, or to the centre when it is off (`0x7100119ef0`).
//!
//! shortcut: files load synchronously, so "start together once all are loaded" is immediate; the voice segments of
//! `@x` (unused by Umineko) are not done; a voice fading out after a stop is no longer panned.

use std::sync::{Arc, Mutex};

use bitflags::bitflags;
use kira::track::TrackId;
use shin_audio::{AudioData, AudioFile, AudioHandle, AudioManager, AudioSettings};
use shin_core::{
    format::scenario::{Scenario, info::VoiceMappingInfoItem},
    time::{Ticks, Tween},
    vm::command::types::{AudioWaitStatus, Pan, Volume},
};
use shin_tasks::ComputeTask;
use tracing::warn;

use crate::asset::system::AssetServer;

bitflags! {
    #[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Clone, Copy)]
    pub struct VoicePlayFlags: i32 {
        const ENABLE_CHARACTER_LIPSYNC = 1;
        const ENABLE_CHARACTER_MUTING = 2;
    }
}

struct Voice {
    handle: AudioHandle,
    volume: f32,
    /// The speakers whose bustups lip-sync to it (empty without the lip-sync flag)
    speakers: Vec<u8>,
    /// The mouth level of each 50 ms window, empty until made
    levels: Vec<u8>,
    levels_task: Option<ComputeTask<Vec<u8>>>,
}

/// The lip-sync level of each speaking character, published by the voice player every frame
static LIPSYNC: Mutex<Vec<(u8, i32)>> = Mutex::new(Vec::new());

/// The pans asked by the bustups this frame: (character id, pan from -1 to 1)
static PANS: Mutex<Vec<(u16, f32)>> = Mutex::new(Vec::new());

/// A bustup of `character_id` at `pan` (its screen x over the half-width): the voice player pans that character's
/// voices there (`0x7100119ef0`)
pub fn request_voice_pan(character_id: u16, pan: f32) {
    // one entry per character, the last bustup's: nothing drains them outside the story (the Character Box)
    let mut pans = PANS.lock().unwrap();
    match pans.iter_mut().find(|(id, _)| *id == character_id) {
        Some(entry) => entry.1 = pan,
        None => pans.push((character_id, pan)),
    }
}

/// The mouth level (0-2) of `character_id` from the voices playing now, or -1 when the character is not speaking
pub fn lipsync_level(character_id: u16) -> i32 {
    LIPSYNC
        .lock()
        .unwrap()
        .iter()
        .find(|(speaker, _)| *speaker as u16 == character_id)
        .map_or(-1, |&(_, level)| level)
}

pub struct VoicePlayer {
    audio_manager: Arc<AudioManager>,
    asset_server: Arc<AssetServer>,
    /// The voices of the current line
    playing: Vec<Voice>,
    /// The voice volume setting (v2, 0-100)
    setting_volume: u8,
    /// The per-character voice switches (setting v19, a bit per character id)
    character_mask: u32,
    /// Setting v5: voices follow their speaker's bustup across the screen
    pan_enabled: bool,
}

impl VoicePlayer {
    pub fn new(audio_manager: Arc<AudioManager>, asset_server: Arc<AssetServer>) -> Self {
        Self {
            audio_manager,
            asset_server,
            playing: Vec::new(),
            setting_volume: 90,
            character_mask: u32::MAX,
            pan_enabled: true,
        }
    }

    /// The voice settings: volume (v2), the per-character switches (v19) and the speaker panning (v5)
    pub fn set_settings(&mut self, setting_volume: u8, character_mask: u32, pan_enabled: bool) {
        self.setting_volume = setting_volume.min(100);
        self.character_mask = character_mask;
        self.pan_enabled = pan_enabled;
    }

    /// Plays the voice spec `voicefiles_spec`; false when no file was started
    pub fn play(
        &mut self,
        scenario: &Scenario,
        voicefiles_spec: &str,
        segment_start: u32,
        segment_duration: u32,
        flags: VoicePlayFlags,
        volume: Volume,
    ) -> bool {
        self.stop();
        if segment_start != 0 || segment_duration != 0 {
            warn!("Voice segments are not supported; playing {voicefiles_spec} whole");
        }
        let volume = volume.0.max(0.0);
        let mut started = 0;
        for name in voicefiles_spec.split('|').filter(|name| !name.is_empty()) {
            let mapping = mapping(scenario, name);
            if flags.contains(VoicePlayFlags::ENABLE_CHARACTER_MUTING)
                && !speaks(mapping, self.character_mask)
            {
                continue;
            }
            let speakers = match mapping {
                Some(item) if flags.contains(VoicePlayFlags::ENABLE_CHARACTER_LIPSYNC) => {
                    item.lipsync_character_ids.0.to_vec()
                }
                _ => Vec::new(),
            };
            // the archive stores lowercase names
            let path = format!("/voice/{}.nxa", name.to_ascii_lowercase());
            let file = match self.asset_server.load_sync::<AudioFile>(&path) {
                Ok(file) => file,
                Err(e) => {
                    warn!("Could not load the voice {path}: {e:?}");
                    continue;
                }
            };
            let gain = Volume(self.setting_volume as f32 / 100.0 * volume);
            let levels_task = (!speakers.is_empty()).then(|| {
                let file = file.clone();
                shin_tasks::compute::spawn(move || lipsync_table(&file))
            });
            let handle = self.audio_manager.play(AudioData::from_audio_file(file, AudioSettings {
                track: TrackId::Main,
                fade_in: Tween::linear(Ticks::ZERO),
                loop_start: None,
                volume: gain,
                pan: Pan::default(),
            }));
            self.playing.push(Voice {
                handle,
                volume,
                speakers,
                levels: Vec::new(),
                levels_task,
            });
            started += 1;
        }
        started > 0
    }

    /// Fades the current voices out: their volume drops by 0.2 per frame
    pub fn stop(&mut self) {
        for mut voice in self.playing.drain(..) {
            let _ = voice
                .handle
                .stop(Tween::linear(Ticks::from_f32(voice.volume / 0.2)));
        }
    }

    /// Holds the voices where they are, or lets them go on (`0x710011aab0`)
    pub fn set_paused(&mut self, paused: bool) {
        for voice in &mut self.playing {
            let _ = voice.handle.set_paused(paused);
        }
    }

    /// Playing while any voice of the line still plays (stopping ones no longer count)
    pub fn get_wait_status(&self) -> AudioWaitStatus {
        if self
            .playing
            .iter()
            .any(|voice| voice.handle.get_wait_status().contains(AudioWaitStatus::PLAYING))
        {
            AudioWaitStatus::PLAYING
        } else {
            AudioWaitStatus::empty()
        }
    }
}

impl VoicePlayer {
    /// Applies the bustups' pans and publishes the lip-sync levels of the voices playing now; call once per frame
    pub fn publish_lipsync(&mut self) {
        for (character_id, pan) in PANS.lock().unwrap().drain(..) {
            let pan = if self.pan_enabled { pan } else { 0.0 };
            for voice in &mut self.playing {
                if voice.speakers.iter().any(|&id| id as u16 == character_id) {
                    let _ = voice
                        .handle
                        .set_panning(Pan(pan), Tween::linear(Ticks::ZERO));
                }
            }
        }

        let mut levels = LIPSYNC.lock().unwrap();
        levels.clear();
        for voice in &mut self.playing {
            if let Some(table) = voice
                .levels_task
                .as_mut()
                .and_then(|task| task.poll_naive())
            {
                voice.levels = table;
                voice.levels_task = None;
            }
            if !voice
                .handle
                .get_wait_status()
                .contains(AudioWaitStatus::PLAYING)
            {
                continue;
            }
            // the entry of the 50 ms window playing now, or the last one past the end
            let window = (voice.handle.position().as_seconds() * 20.0) as usize;
            let level = match voice.levels.len() {
                0 => 0,
                len => voice.levels[window.min(len - 1)] as i32,
            };
            for &speaker in &voice.speakers {
                if !levels.iter().any(|&(other, _)| other == speaker) {
                    levels.push((speaker, level));
                }
            }
        }
    }
}

/// The mouth level of each 50 ms window of a voice (`0x71000f55d0`): the peak of every sample of every channel, with
/// hysteresis between closed (0), half open (1) and open (2). A last partial window gets no entry.
fn lipsync_table(file: &Arc<AudioFile>) -> Vec<u8> {
    use shin_core::format::audio::{AudioBuffer, AudioDecoder, AudioFrameSource};

    let mut decoder = match AudioDecoder::new(file.clone()) {
        Ok(decoder) => decoder,
        Err(e) => {
            warn!("Could not decode a voice for its lip-sync: {e:?}");
            return Vec::new();
        }
    };
    let mut buffer = AudioBuffer::with_capacity(decoder.max_frame_size());
    let mut peaks = Vec::new();
    let mut skip = decoder.pre_skip() as usize;
    while decoder.read_frame(&mut buffer) {
        let samples = buffer.samples();
        let start = skip.min(samples.len());
        skip -= start;
        peaks.extend(samples[start..].iter().map(|&(l, r)| l.abs().max(r.abs())));
        buffer.clear();
    }
    let window = (decoder.sample_rate() / 20).max(1) as usize;
    lipsync_levels(
        peaks
            .chunks_exact(window)
            .map(|w| w.iter().fold(0.0f32, |a, &b| a.max(b))),
    )
}

/// The level state machine over the windows' peaks (thresholds `0x710015a44c`, `aac8`, `a550`, `a574`); half open
/// closes after 4 windows without a change
fn lipsync_levels(peaks: impl Iterator<Item = f32>) -> Vec<u8> {
    let (mut state, mut hold) = (0u8, 0u32);
    peaks
        .map(|peak| {
            let next = match state {
                0 if peak > 0.12 => Some(1),
                1 if peak > 0.55 => Some(2),
                1 if peak < 0.10 => Some(0),
                2 if peak < 0.45 => Some(1),
                _ => None,
            };
            match next {
                Some(next) => {
                    state = next;
                    hold = 0;
                }
                None => {
                    hold += 1;
                    if hold >= 4 && state == 1 {
                        state = 0;
                        hold = 0;
                    }
                }
            }
            state
        })
        .collect()
}

/// The scenario's voice mapping entry for a voice file
fn mapping<'a>(scenario: &'a Scenario, name: &str) -> Option<&'a VoiceMappingInfoItem> {
    scenario
        .info_tables()
        .voice_mapping_info
        .iter()
        .find(|item| matches_pattern(item.name_pattern.as_str(), name))
}

/// Whether a voice file has a speaker switched on in `mask`; files without a mapping always play (`0x71000eadc0`)
fn speaks(mapping: Option<&VoiceMappingInfoItem>, mask: u32) -> bool {
    match mapping {
        None => true,
        Some(VoiceMappingInfoItem {
            lipsync_character_ids,
            ..
        }) => lipsync_character_ids
            .0
            .iter()
            .any(|&id| mask & (1 << (id & 0x1f)) != 0),
    }
}

/// A voice mapping pattern: a file name, or one with `*` standing for any run of characters
fn matches_pattern(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern.eq_ignore_ascii_case(name),
        Some((prefix, rest)) => {
            name.len() >= prefix.len()
                && name[..prefix.len()].eq_ignore_ascii_case(prefix)
                && (0..=name.len() - prefix.len())
                    .any(|start| matches_pattern(rest, &name[prefix.len() + start..]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{lipsync_levels, matches_pattern};

    #[test]
    fn lipsync_hysteresis() {
        let levels = |peaks: &[f32]| lipsync_levels(peaks.iter().copied());
        assert_eq!(levels(&[0.11, 0.13, 0.6, 0.5, 0.44, 0.11]), [0, 1, 2, 2, 1, 1]);
        // half open closes after 4 windows without a change
        assert_eq!(levels(&[0.2, 0.2, 0.2, 0.2, 0.2, 0.2]), [1, 1, 1, 1, 0, 1]);
        assert_eq!(levels(&[0.2, 0.05]), [1, 0]);
    }

    #[test]
    fn voice_mapping_patterns() {
        assert!(matches_pattern("01/11500001", "01/11500001"));
        assert!(matches_pattern("01/115*", "01/11500003"));
        assert!(matches_pattern("*/1150*", "19/11500003"));
        assert!(!matches_pattern("01/116*", "01/11500003"));
        assert!(matches_pattern("*", "anything"));
    }
}
