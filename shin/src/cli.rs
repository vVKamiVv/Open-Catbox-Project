use std::path::PathBuf;

use clap::Parser;
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
    /// Seconds to wait after a captured line has typed out, so animations settle
    #[clap(long, default_value_t = 4.0)]
    pub capture_settle: f32,
}

