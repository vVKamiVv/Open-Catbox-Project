use kira::track::TrackId;
use shin_audio::{AudioData, AudioManager, AudioSettings};
use shin_core::{
    format::sysse::SysSe,
    time::{Ticks, Tween},
    vm::command::types::{Pan, Volume},
};
use tracing::warn;

use crate::asset::system::AssetServer;

/// Plays a built-in system sound from `/sysse.bin` once, as the original's `SysSePlayer::play` (`0x71000272b0`)
pub fn play_sys_se(audio: &AudioManager, assets: &AssetServer, name: &str, volume: Volume) {
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
