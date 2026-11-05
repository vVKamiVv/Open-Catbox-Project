//! Character voices, as the original's `VoicePlayer` (play `0x710011a0f0`, update `0x710011ab70`, stop
//! `0x710011a6b0`, status `0x710011a980`).
//!
//! A line's voice spec names one file or several separated by `|`, each `/voice/<name>.nxa`; they start together.
//! Each voice keeps its own volume, played at `v2 / 100 x volume`; a stop fades it by 0.2 per frame, so a full-volume
//! voice fades over 5 frames. With the muting flag, a file whose speakers (the scenario's voice mapping) are all
//! switched off in setting v19 is skipped (`0x71000eadc0`).
//!
//! Lip-sync: a bustup whose character speaks in a playing voice opens its mouth by the voice's level 0-3
//! (`ADV::vfunc_63` `0x7100014eb0`: the first playing voice whose speakers include the character).
//!
//! shortcut: files load synchronously, so "start together once all are loaded" is immediate; the speaker panning
//! (setting v5) and the voice segments of `@x` (unused by Umineko) are not done. The original reads the level from a
//! per-stream table filled while decoding (stream `+0x2e8`, not decoded yet); the port derives it from the voice's
//! loudness instead.

use std::sync::{Arc, Mutex};

use bitflags::bitflags;
use kira::track::TrackId;
use shin_audio::{AudioData, AudioFile, AudioHandle, AudioManager, AudioSettings};
use shin_core::{
    format::scenario::{Scenario, info::VoiceMappingInfoItem},
    time::{Ticks, Tween},
    vm::command::types::{AudioWaitStatus, Pan, Volume},
};
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
}

/// The lip-sync level of each speaking character, published by the voice player every frame
static LIPSYNC: Mutex<Vec<(u8, i32)>> = Mutex::new(Vec::new());

/// The mouth level (0-3) of `character_id` from the voices playing now, or -1 when the character is not speaking
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
}

impl VoicePlayer {
    pub fn new(audio_manager: Arc<AudioManager>, asset_server: Arc<AssetServer>) -> Self {
        Self {
            audio_manager,
            asset_server,
            playing: Vec::new(),
            setting_volume: 90,
            character_mask: u32::MAX,
        }
    }

    /// The voice settings: volume (v2) and the per-character switches (v19)
    pub fn set_settings(&mut self, setting_volume: u8, character_mask: u32) {
        self.setting_volume = setting_volume.min(100);
        self.character_mask = character_mask;
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
    /// Publishes the lip-sync levels of the voices playing now; call once per frame
    pub fn publish_lipsync(&self) {
        let mut levels = LIPSYNC.lock().unwrap();
        levels.clear();
        for voice in &self.playing {
            if !voice
                .handle
                .get_wait_status()
                .contains(AudioWaitStatus::PLAYING)
            {
                continue;
            }
            // shortcut: loudness steps instead of the original's per-stream level table
            let level = (voice.handle.get_amplitude() / 0.1).min(3.0) as i32;
            for &speaker in &voice.speakers {
                if !levels.iter().any(|&(other, _)| other == speaker) {
                    levels.push((speaker, level));
                }
            }
        }
    }
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
    use super::matches_pattern;

    #[test]
    fn voice_mapping_patterns() {
        assert!(matches_pattern("01/11500001", "01/11500001"));
        assert!(matches_pattern("01/115*", "01/11500003"));
        assert!(matches_pattern("*/1150*", "19/11500003"));
        assert!(!matches_pattern("01/116*", "01/11500003"));
        assert!(matches_pattern("*", "anything"));
    }
}
