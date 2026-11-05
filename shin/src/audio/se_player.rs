use std::sync::Arc;

use kira::track::{TrackBuilder, TrackHandle, TrackId, TrackRoutes};
use shin_audio::{AudioData, AudioFile, AudioHandle, AudioManager, AudioSettings};
use shin_core::{
    time::{Ticks, Tween},
    vm::command::types::{AudioWaitStatus, Pan, Volume},
};
use tracing::warn;

pub const SE_SLOT_COUNT: usize = 32;

pub struct SePlayer {
    audio_manager: Arc<AudioManager>,
    se_tracks: [TrackHandle; SE_SLOT_COUNT],
    se_slots: [Option<AudioHandle>; SE_SLOT_COUNT],
    /// Automatic channels -1, -2, ... used by SEONCE (index 0 is channel -1), like the original's negative ids
    once_slots: Vec<AudioHandle>,
}

impl SePlayer {
    pub fn new(audio_manager: Arc<AudioManager>) -> Self {
        let mut manager = audio_manager.kira_manager().lock();

        let se_tracks = [(); SE_SLOT_COUNT].map(|_| {
            manager
                .add_sub_track(TrackBuilder::new().routes(TrackRoutes::parent(TrackId::Main)))
                .expect("Failed to create se track")
        });

        drop(manager);

        Self {
            audio_manager,
            se_tracks,
            se_slots: [(); SE_SLOT_COUNT].map(|_| None),
            once_slots: Vec::new(),
        }
    }

    /// Plays `se` once on the first automatic channel that is not playing and returns that channel (-1, -2, ...)
    pub fn play_once(&mut self, se: Arc<AudioFile>, volume: Volume, pan: Pan) -> i32 {
        let kira_data = AudioData::from_audio_file(se, AudioSettings {
            track: TrackId::Main,
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

    pub fn play(
        &mut self,
        slot: i32,
        se: Arc<AudioFile>,
        repeat: bool,
        volume: Volume,
        pan: Pan,
        fade_in: Tween,
    ) {
        let slot = slot as usize;

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
            old_handle.stop(Tween::MS_15).unwrap();
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

        if let Some(mut se) = self.se_slots[slot].take() {
            se.stop(fade_out).unwrap();
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
