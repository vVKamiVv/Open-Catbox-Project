//! The launcher's music player: the Music Box's tracks, read from the game data on this computer (nothing of the game
//! is shipped). The track list comes from the language's script (`/main.snr`, its BGM and Music Box tables), each
//! track's audio from `/bgm/<name>.nxa`, handed to the page as Ogg Opus (repackaged, not re-encoded).

use std::{fs::File, path::Path};

use serde::Serialize;
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
    /// False for the tracks the Music Box plays once (the opening themes)
    pub looping: bool,
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
                looping: entry.once_flag == 0,
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
