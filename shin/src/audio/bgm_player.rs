use std::sync::Arc;

use kira::track::{TrackBuilder, TrackHandle, TrackId, TrackRoutes};
use shin_audio::{AudioData, AudioFile, AudioHandle, AudioManager, AudioSettings};
use shin_core::{
    time::{Ticks, Tween},
    format::scenario::info::BgmId,
    vm::command::types::{AudioWaitStatus, Pan, Volume},
};
use tracing::warn;

pub struct BgmPlayer {
    audio_manager: Arc<AudioManager>,
    bgm_track: TrackHandle,
    // TODO: async track loading?
    current_bgm: Option<AudioHandle>,
    /// The track `current_bgm` plays
    current_id: Option<BgmId>,
    /// The BGM volume setting (v0, 0-100)
    setting_volume: u8,
    /// The voice-focus factor (1, or down to 0.5 while a voice plays)
    duck: f32,
    /// The gain last given to the track
    applied_gain: Option<f32>,
}

impl BgmPlayer {
    pub fn new(audio_manager: Arc<AudioManager>) -> Self {
        let mut manager = audio_manager.kira_manager().lock();

        let bgm_track = manager
            .add_sub_track(TrackBuilder::new().routes(TrackRoutes::parent(TrackId::Main)))
            .expect("Failed to create bgm track");

        drop(manager);

        Self {
            audio_manager,
            bgm_track,
            current_bgm: None,
            current_id: None,
            setting_volume: 70,
            duck: 1.0,
            applied_gain: None,
        }
    }

    /// The BGM volume setting last given (v0, 0-100)
    pub fn setting_volume(&self) -> u8 {
        self.setting_volume
    }

    /// The BGM volume setting (v0, 0-100) and the voice-focus factor scale the BGM track (`0x7100033eb0`)
    pub fn set_gain(&mut self, setting_volume: u8, duck: f32) {
        self.setting_volume = setting_volume;
        self.duck = duck;
        let gain = self.setting_volume.min(100) as f32 / 100.0 * self.duck;
        if self.applied_gain.replace(gain) != Some(gain) {
            set_track_gain(&mut self.bgm_track, gain);
        }
    }

    /// `0x71000337f0`: the track already playing only takes the new volume; another one crossfades in. Returns
    /// whether a new track started (which shows the song title in the original)
    pub fn play(
        &mut self,
        id: BgmId,
        bgm: Arc<AudioFile>,
        _display_name: &str,
        repeat: bool,
        volume: Volume,
        fade_in: Tween,
    ) -> bool {
        if self.current_id == Some(id) {
            if let Some(handle) = self.current_bgm.as_mut() {
                if handle.get_wait_status().contains(AudioWaitStatus::PLAYING) {
                    handle.set_volume(volume, fade_in).unwrap();
                    return false;
                }
            }
        }
        self.current_id = Some(id);

        let loop_start = repeat.then_some(bgm.info().loop_start);
        let kira_data = AudioData::from_audio_file(bgm, AudioSettings {
            track: self.bgm_track.id(),
            fade_in,
            loop_start,
            volume,
            pan: Pan::default(),
        });

        let handle = self.audio_manager.play(kira_data);

        if let Some(mut old_handle) = self.current_bgm.take() {
            old_handle.stop(at_least_4_frames(fade_in)).unwrap();
        }

        self.current_bgm = Some(handle);
        true
    }

    pub fn set_volume(&mut self, volume: Volume, tween: Tween) {
        if let Some(handle) = self.current_bgm.as_mut() {
            handle.set_volume(volume, tween).unwrap();
        } else {
            warn!("Tried to set volume of BGM, but no BGM is currently playing");
        }
    }

    pub fn get_wait_status(&self) -> AudioWaitStatus {
        self.current_bgm
            .as_ref()
            .map_or(AudioWaitStatus::empty(), AudioHandle::get_wait_status)
    }

    /// Playback position of the current track (zero when nothing plays)
    pub fn position(&self) -> Ticks {
        self.current_bgm
            .as_ref()
            .map_or(Ticks::ZERO, AudioHandle::position)
    }

    pub fn stop(&mut self, fade_out: Tween) {
        self.current_id = None;
        if let Some(mut handle) = self.current_bgm.take() {
            handle.stop(at_least_4_frames(fade_out)).unwrap();
        } else {
            warn!("Tried to stop BGM, but no BGM is currently playing");
        }
    }
}

// TODO: make it renderable and updatable, as it can display they track name when the BGM starts

/// Sets a track's linear gain (the original multiplies its samples)
pub fn set_track_gain(track: &mut TrackHandle, gain: f32) {
    track.set_volume(
        kira::Volume::Amplitude(gain.max(0.0) as f64),
        kira::tween::Tween::default(),
    );
}

/// The original's players never cut a sound: stops and crossfades last at least 4 frames (`0x71000f2b30`)
pub fn at_least_4_frames(tween: Tween) -> Tween {
    Tween {
        duration: tween.duration.max(Ticks::from_f32(4.0)),
        ..tween
    }
}
