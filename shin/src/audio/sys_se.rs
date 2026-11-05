use std::sync::atomic::{AtomicU8, Ordering};

use kira::track::TrackId;
use shin_audio::{AudioData, AudioManager, AudioSettings};
use shin_core::{
    format::sysse::SysSe,
    time::{Ticks, Tween},
    vm::command::types::{Pan, Volume},
};
use tracing::warn;

use crate::asset::system::AssetServer;

/// The system sound volume setting (v3, 0-100), read when a sound starts as the original reads the save data
static SETTING_VOLUME: AtomicU8 = AtomicU8::new(55);
/// The BGM, SE and voice volume settings (v0, v1, v2) for the movies, which take one of them by their volume source
static MOVIE_SETTING_VOLUMES: [AtomicU8; 3] = [AtomicU8::new(70), AtomicU8::new(70), AtomicU8::new(90)];

pub fn set_setting_volume(percent: u8) {
    SETTING_VOLUME.store(percent.min(100), Ordering::Relaxed);
}

pub fn set_movie_setting_volumes(bgm: u8, se: u8, voice: u8) {
    for (slot, percent) in MOVIE_SETTING_VOLUMES.iter().zip([bgm, se, voice]) {
        slot.store(percent.min(100), Ordering::Relaxed);
    }
}

/// A movie's settings gain by its volume source (0 ignore, 1 BGM, 2 SE, 3 voice), as the original's movie player
pub fn movie_setting_gain(source: shin_core::format::scenario::info::MovieVolumeSource) -> f32 {
    use shin_core::format::scenario::info::MovieVolumeSource;
    let slot = match source {
        MovieVolumeSource::Ignore => return 1.0,
        MovieVolumeSource::Bgm => 0,
        MovieVolumeSource::Se => 1,
        MovieVolumeSource::Voice => 2,
    };
    MOVIE_SETTING_VOLUMES[slot].load(Ordering::Relaxed) as f32 / 100.0
}

/// Plays a built-in system sound from `/sysse.bin` once, as the original's `SysSePlayer::play` (`0x71000272b0`)
pub fn play_sys_se(audio: &AudioManager, assets: &AssetServer, name: &str, volume: Volume) {
    let volume = Volume(volume.0 * SETTING_VOLUME.load(Ordering::Relaxed) as f32 / 100.0);
    let sys_se = match assets.load_sync::<SysSe>("/sysse.bin") {
        Ok(sys_se) => sys_se,
        Err(e) => {
            warn!("could not load sysse.bin: {e:?}");
            return;
        }
    };
    let Some(sound) = sys_se.sounds.get(name) else {
        warn!("sysse.bin has no \"{name}\" sound");
        return;
    };
    // fire and forget: the handle is not needed to let it play out
    audio.play(AudioData {
        source: sound.decode(),
        settings: AudioSettings {
            track: TrackId::Main,
            fade_in: Tween::linear(Ticks::ZERO),
            loop_start: None,
            volume,
            pan: Pan::default(),
        },
    });
}
