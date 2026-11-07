//! The game's sound players, over `shin-audio`: one BGM player shared by every scene (crossfades, the BGM volume and
//! voice focus), the sound-effect player (32 slots plus SEONCE's automatic channels; a looping sound re-issued keeps
//! playing), the voice player (panning to the speaker's bustup and the lip-sync level table) and the system sounds of
//! the menus and SYSSE. See `research/docs/engine-internals.md` §10, §15 and §16.

mod bgm_player;
mod se_player;
pub mod sys_se;
mod voice_player;

pub use bgm_player::BgmPlayer;
pub use se_player::{SePlayer, SE_SLOT_COUNT};
pub use sys_se::play_sys_se;
pub use voice_player::{VoicePlayFlags, VoicePlayer, lipsync_level, request_voice_pan};
