use std::path::PathBuf;

use clap::Parser;
use shin_core::format::save::{GameDataEntry, SaveData};
use clap_num::maybe_hex;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
/// A visual novel engine
pub struct Cli {
    /// Search this directory for assets
    ///
    /// The directory must contain either a directory named "data" or a file named "data.rom".
    /// Consult the README for more information.
    #[clap(short, long)]
    pub assets_dir: Option<PathBuf>,
    /// A folder with the game's executable (`main`) and a translation's `.ips` patches, for the texts the engine
    /// keeps there; by default `SHIN_EXEFS`, or an `exefs` folder next to the assets
    #[clap(long)]
    pub exefs: Option<PathBuf>,
    /// Automatically fast-forward the scenario to the specified address (useful for debugging)
    #[clap(long, value_parser=maybe_hex::<u32>)]
    pub fast_forward_to: Option<u32>,
    /// Start scenario execution from the specified address (useful for debugging)
    ///
    /// Called "unsafe" because the VM is not designed to be started from arbitrary addresses.
    /// But in lieu of proper scene loading this helps a lot with testing later parts of the episodes.
    #[clap(long, value_parser=maybe_hex::<u32>)]
    pub unsafe_entry_point: Option<u32>,
    /// Fast-forward to each of these message ids (comma-separated, in script order), save the screen as
    /// `<capture-dir>/<id>.png` once the line has typed out, and quit after the last one
    ///
    /// For comparing with the original captured by `research/tools/compare/eden_capture.py`. The PNG has the canvas
    /// size, so use a display at 100% scale for 1920x1080.
    #[clap(long, value_delimiter = ',')]
    pub capture_messages: Vec<u32>,
    /// Where `--capture-messages` saves its PNGs
    #[clap(long, default_value = "captures/port")]
    pub capture_dir: PathBuf,
    /// The save file (the Switch's `SAVEDATA` format); by default `SAVEDATA` in the user's data directory
    /// (`%APPDATA%\\shin` on Windows)
    #[clap(long)]
    pub savedata: Option<PathBuf>,
    /// Skip the title and start Episode 1 at once
    #[clap(long)]
    pub new_game: bool,
    /// The episode (1-based) a story started without a save begins with, so `--capture-messages` and
    /// `--fast-forward-to` reach later episodes through their own set-up
    #[clap(long, default_value_t = 1, value_parser = clap::value_parser!(i32).range(1..))]
    pub episode: i32,
    /// Skip the title and load a save: `auto` for the auto slot, or a slot number (1-100)
    #[clap(long, value_parser = parse_slot)]
    pub load: Option<LoadSlot>,
    /// Seconds to wait after a captured line has typed out, so animations settle
    #[clap(long, default_value_t = 4.0)]
    pub capture_settle: f32,
    /// Developer tools: F3 opens the debug overlays (FPS, VM state, layers and their tweens); F6 pauses the game's
    /// time, F7 steps one frame while paused, F8/F9 halve/double its speed (1/8x to 8x), F10 resets it to 1x
    #[clap(long)]
    pub dev: bool,
}

/// A save slot to load
#[derive(Debug, Clone, Copy)]
pub enum LoadSlot {
    Auto,
    /// 0-based
    Manual(usize),
}

impl LoadSlot {
    pub fn entry(self, save: &SaveData) -> Option<GameDataEntry> {
        match self {
            LoadSlot::Auto => save.auto_save_slot.as_ref(),
            LoadSlot::Manual(slot) => save.manual_save_slots[slot].as_ref(),
        }
        .map(|game| game.entry.clone())
    }
}

fn parse_slot(slot: &str) -> Result<LoadSlot, String> {
    if slot.eq_ignore_ascii_case("auto") {
        return Ok(LoadSlot::Auto);
    }
    match slot.parse::<usize>() {
        Ok(n @ 1..=100) => Ok(LoadSlot::Manual(n - 1)),
        _ => Err(format!("`{slot}` is neither `auto` nor a slot number from 1 to 100")),
    }
}
