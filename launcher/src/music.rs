//! The launcher's music player: the Music Box's tracks, read from the game data on this computer (nothing of the game
//! is shipped). The track list comes from the language's script (`/main.snr`, its BGM and Music Box tables), each
//! track's audio from `/bgm/<name>.nxa`, handed to the page as Ogg Opus (repackaged, not re-encoded). How the
//! player plays them is its own: its settings are in `music.json`, apart from the launcher's, and nothing the game
//! does with a track (the Music Box's play-once flag) decides anything here.

use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use shin_core::{
    format::{
        audio::{read_audio, to_ogg_opus},
        rom::RomReader,
        scenario::Scenario,
    },
    primitives::stateless_reader::StatelessFile,
};

#[derive(Debug, Clone, Serialize)]
pub struct Track {
    /// The title the game shows when the track starts
    pub title: String,
    /// The `.nxa` under `/bgm/`
    pub file: String,
}

/// The player's settings, in `music.json` beside `launcher.json`
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Seconds one track fades into the next; 0 for none
    pub crossfade: f32,
    /// Times a track goes through its loop before the next one starts
    pub loops: u32,
    /// Seconds a track fades out over at the end of its last loop; 0 for none
    pub fade_out: f32,
    /// Whether the music goes on when the game starts
    pub keep_playing: bool,
    pub playlists: Vec<Playlist>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            crossfade: 2.0,
            loops: 1,
            fade_out: 4.0,
            keep_playing: false,
            playlists: Vec::new(),
        }
    }
}

/// A named list of tracks, by their `/bgm/` file names (the same in every language)
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct Playlist {
    pub name: String,
    pub files: Vec<String>,
}

impl Settings {
    fn path(launcher_settings: &Path) -> PathBuf {
        launcher_settings.with_file_name("music.json")
    }

    /// The settings beside `launcher_settings`; the first time, the crossfade that 0.2.6 kept in `launcher.json`
    /// (`music_crossfade`) is brought over and the file written, as `launcher.json` drops it on its next save.
    pub fn load(launcher_settings: &Path) -> Self {
        let path = Self::path(launcher_settings);
        if let Ok(text) = fs::read_to_string(&path) {
            return serde_json::from_str(&text).unwrap_or_default();
        }
        let old = fs::read_to_string(launcher_settings)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v.get("music_crossfade")?.as_f64());
        let mut settings = Self::default();
        if let Some(crossfade) = old {
            settings.crossfade = crossfade as f32;
            let _ = settings.store(launcher_settings); // not written: asked again next time
        }
        settings
    }

    pub fn store(&self, launcher_settings: &Path) -> Result<(), String> {
        let path = Self::path(launcher_settings);
        fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::write(&path, serde_json::to_string_pretty(self).unwrap()).map_err(|e| e.to_string())
    }
}

/// The language's patch first (its script and anything it replaces), then the game's own data
fn roms(data: &Path, lang: &str) -> Vec<RomReader<StatelessFile>> {
    let patch = data.join("languages").join(lang).join("patch.rom");
    [patch, data.join("data.rom")]
        .iter()
        .filter_map(|path| {
            let file = File::open(path).ok()?;
            RomReader::new(StatelessFile::new(file).ok()?).ok()
        })
        .collect()
}

fn read(roms: &mut [RomReader<StatelessFile>], path: &str) -> Option<Vec<u8>> {
    use std::io::Read;
    roms.iter_mut().find_map(|rom| {
        let file = rom.find_file(path).ok()?;
        let mut bytes = Vec::new();
        rom.open_file(file).ok()?.read_to_end(&mut bytes).ok()?;
        Some(bytes)
    })
}

/// The Music Box's tracks in its order
pub fn tracks(data: &Path, lang: &str) -> Result<Vec<Track>, String> {
    let mut roms = roms(data, lang);
    let script = read(&mut roms, "/main.snr").ok_or("The game data is not set up.")?;
    let scenario = Scenario::new(script.into()).map_err(|e| format!("Cannot read the script: {e}"))?;
    let tables = scenario.info_tables();
    Ok(tables
        .music_box_info
        .iter()
        .map(|entry| {
            let bgm = tables.bgm_info(entry.bgm_id);
            Track {
                title: bgm.display_name.as_str().to_string(),
                file: bgm.name.as_str().to_string(),
            }
        })
        .collect())
}

/// Track `file` for the page: 12 bytes (loop start, loop end and sample rate, little-endian u32; the loop in samples
/// after the pre-skip, as the game seeks) and then the Ogg Opus stream
pub fn track(data: &Path, lang: &str, file: &str) -> Result<Vec<u8>, String> {
    let mut roms = roms(data, lang);
    let nxa = read(&mut roms, &format!("/bgm/{}.nxa", file.to_ascii_lowercase()))
        .ok_or_else(|| format!("{file} is not in the game data."))?;
    let audio = read_audio(&nxa).map_err(|e| format!("Cannot read {file}: {e}"))?;
    let info = audio.info().clone();
    let mut out = Vec::with_capacity(nxa.len() + 4096);
    for value in [info.loop_start, info.loop_end, info.sample_rate] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend(to_ogg_opus(audio));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_start_from_the_old_crossfade_and_round_trip() {
        let dir = std::env::temp_dir().join(format!("oc-music-{}", std::process::id()));
        let launcher = dir.join("launcher.json");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&launcher, r#"{"fullscreen":true,"music_crossfade":5.0}"#).unwrap();
        let mut settings = Settings::load(&launcher);
        assert_eq!(
            settings,
            Settings {
                crossfade: 5.0,
                ..Settings::default()
            }
        );
        assert!(
            dir.join("music.json").is_file(),
            "the old value is written over at once"
        );
        settings.loops = 3;
        settings.playlists.push(Playlist {
            name: "Battle".into(),
            files: vec!["umib_001".into()],
        });
        settings.store(&launcher).unwrap();
        fs::write(&launcher, r#"{"music_crossfade":1.0}"#).unwrap();
        assert_eq!(
            Settings::load(&launcher),
            settings,
            "music.json wins once it exists"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    /// With the game set up (OPEN_CATBOX_DATA=<data folder>): the English list and a track
    #[test]
    #[ignore]
    fn real_data() {
        let data = std::path::PathBuf::from(std::env::var("OPEN_CATBOX_DATA").unwrap());
        let tracks = tracks(&data, "en").unwrap();
        assert!(tracks.len() > 50, "{}", tracks.len());
        assert!(tracks.iter().all(|t| !t.title.is_empty() && !t.file.is_empty()));
        let bytes = track(&data, "en", &tracks[0].file).unwrap();
        assert_eq!(&bytes[12..16], b"OggS");
        println!("{} tracks, the first {:?}, {} bytes", tracks.len(), tracks[0], bytes.len());
    }
}
