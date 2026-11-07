//! Installing the launcher. On Windows it installs itself (`windows.rs`): one Setup that copies itself into a folder
//! of the user's, with shortcuts and an Installed apps entry. On Linux and macOS the system's package does that (the
//! AppImage, `.deb`, `.rpm`, or the `.app` from the `.dmg`), so `other.rs` only keeps the same calls working.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(not(windows))]
mod other;
#[cfg(not(windows))]
pub use other::*;
