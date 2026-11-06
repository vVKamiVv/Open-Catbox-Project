use std::sync::Arc;

use kira::track::{TrackBuilder, TrackHandle, TrackId, TrackRoutes};
use shin_audio::{AudioData, AudioFile, AudioHandle, AudioManager, AudioSettings};
use shin_core::{
    time::{Ticks, Tween},
    format::scenario::info::SeId,
    vm::command::types::{AudioWaitStatus, Pan, Volume},
};
use tracing::warn;

use super::bgm_player::{at_least_4_frames, set_track_gain};

pub const SE_SLOT_COUNT: usize = 32;

pub struct SePlayer {
    audio_manager: Arc<AudioManager>,
    se_tracks: [TrackHandle; SE_SLOT_COUNT],
    se_slots: [Option<AudioHandle>; SE_SLOT_COUNT],
    /// What each slot plays: the sound and whether it loops
    se_ids: [Option<(SeId, bool)>; SE_SLOT_COUNT],
    /// Automatic channels -1, -2, ... used by SEONCE (index 0 is channel -1), like the original's negative ids
    once_slots: Vec<AudioHandle>,
    /// The track of the automatic channels
    once_track: TrackHandle,
    /// The gain last given to each slot's track, then the automatic channels' track
    applied_gains: [Option<f32>; SE_SLOT_COUNT + 1],
}

impl SePlayer {
    pub fn new(audio_manager: Arc<AudioManager>) -> Self {
        let mut manager = audio_manager.kira_manager().lock();

        let se_tracks = [(); SE_SLOT_COUNT].map(|_| {
            manager
                .add_sub_track(TrackBuilder::new().routes(TrackRoutes::parent(TrackId::Main)))
                .expect("Failed to create se track")
        });
        let once_track = manager
            .add_sub_track(TrackBuilder::new().routes(TrackRoutes::parent(TrackId::Main)))
            .expect("Failed to create se track");

        drop(manager);

        Self {
            audio_manager,
            se_tracks,
            se_slots: [(); SE_SLOT_COUNT].map(|_| None),
            se_ids: [None; SE_SLOT_COUNT],
            once_slots: Vec::new(),
            once_track,
            applied_gains: [None; SE_SLOT_COUNT + 1],
        }
    }

    /// The SE volume setting (v1, 0-100) scales every sound effect, and the voice-focus factor the looping ones
    /// (`0x71000f3210`)
    pub fn set_gain(&mut self, setting_volume: u8, duck: f32) {
        let setting = setting_volume.min(100) as f32 / 100.0;
        for slot in 0..=SE_SLOT_COUNT {
            let looping = slot < SE_SLOT_COUNT && matches!(self.se_ids[slot], Some((_, true)));
            let gain = if looping { setting * duck } else { setting };
            if self.applied_gains[slot].replace(gain) != Some(gain) {
                let track = if slot < SE_SLOT_COUNT {
                    &mut self.se_tracks[slot]
                } else {
                    &mut self.once_track
                };
                set_track_gain(track, gain);
            }
        }
    }

    /// Holds every sound effect where it is, or lets them go on (`0x71000f3090`)
    pub fn set_paused(&mut self, paused: bool) {
        for handle in self.se_slots.iter_mut().flatten().chain(&mut self.once_slots) {
            let _ = handle.set_paused(paused);
        }
    }

    /// Plays `se` once on the first automatic channel that is not playing and returns that channel (-1, -2, ...)
    pub fn play_once(&mut self, se: Arc<AudioFile>, volume: Volume, pan: Pan) -> i32 {
        let kira_data = AudioData::from_audio_file(se, AudioSettings {
            track: self.once_track.id(),
            fade_in: Tween::linear(Ticks::ZERO),
            loop_start: None,
            volume,
            pan,
        });
        let handle = self.audio_manager.play(kira_data);

        let free = self
            .once_slots
            .iter()
            .position(|h| !h.get_wait_status().contains(AudioWaitStatus::PLAYING));
        let index = match free {
            Some(index) => {
                self.once_slots[index] = handle;
                index
            }
            None => {
                self.once_slots.push(handle);
                self.once_slots.len() - 1
            }
        };
        -(index as i32) - 1
    }

    /// `0x71000f2580`: a loop already playing on the slot only takes the new volume and pan; anything else replaces
    /// what the slot plays
    pub fn play(
        &mut self,
        slot: i32,
        id: SeId,
        se: Arc<AudioFile>,
        repeat: bool,
        volume: Volume,
        pan: Pan,
        fade_in: Tween,
    ) {
        let slot = slot as usize;

        if repeat && self.se_ids[slot] == Some((id, true)) {
            if let Some(handle) = self.se_slots[slot].as_mut() {
                if handle.get_wait_status().contains(AudioWaitStatus::PLAYING) {
                    handle.set_volume(volume, fade_in).unwrap();
                    handle.set_panning(pan, fade_in).unwrap();
                    return;
                }
            }
        }
        self.se_ids[slot] = Some((id, repeat));

        let loop_start = repeat.then_some(se.info().loop_start);
        let kira_data = AudioData::from_audio_file(se, AudioSettings {
            track: self.se_tracks[slot].id(),
            fade_in,
            loop_start,
            volume,
            pan,
        });

        let handle = self.audio_manager.play(kira_data);

        if let Some(mut old_handle) = self.se_slots[slot].take() {
            old_handle.stop(at_least_4_frames(fade_in)).unwrap();
        }

        self.se_slots[slot] = Some(handle);
    }

    pub fn set_volume(&mut self, slot: i32, volume: Volume, tween: Tween) {
        let slot = slot as usize;

        if let Some(handle) = self.se_slots[slot].as_mut() {
            handle.set_volume(volume, tween).unwrap();
        } else {
            warn!(
                "Tried to set volume of se slot {}, but there was no se playing",
                slot
            );
        }
    }

    pub fn set_panning(&mut self, slot: i32, pan: Pan, tween: Tween) {
        let slot = slot as usize;

        if let Some(handle) = self.se_slots[slot].as_mut() {
            handle.set_panning(pan, tween).unwrap();
        } else {
            warn!(
                "Tried to set pan of se slot {}, but there was no se playing",
                slot
            );
        }
    }

    pub fn stop(&mut self, slot: i32, fade_out: Tween) {
        let slot = slot as usize;

        self.se_ids[slot] = None;
        if let Some(mut se) = self.se_slots[slot].take() {
            // `0x71000f2b30`: stops fade over at least 4 frames
            se.stop(at_least_4_frames(fade_out)).unwrap();
        } else {
            warn!("Tried to stop a SE that was not playing");
        }
    }

    pub fn stop_all(&mut self, fade_out: Tween) {
        for slot in 0..SE_SLOT_COUNT {
            if self.se_slots[slot].is_some() {
                self.stop(slot as i32, fade_out);
            }
        }
        // the original's stop-all (0x71000f2d20) covers every channel, automatic ones included
        for mut handle in self.once_slots.drain(..) {
            handle.stop(fade_out).unwrap();
        }
    }

    pub fn get_wait_status(&self, slot: i32) -> AudioWaitStatus {
        if slot < 0 {
            return self
                .once_slots
                .get((-slot - 1) as usize)
                .map_or(AudioWaitStatus::empty(), AudioHandle::get_wait_status);
        }
        let slot = slot as usize;

        if let Some(handle) = self.se_slots[slot].as_ref() {
            handle.get_wait_status()
        } else {
            AudioWaitStatus::empty()
        }
    }
}
