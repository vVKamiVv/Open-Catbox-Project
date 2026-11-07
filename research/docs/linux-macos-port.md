# Linux and macOS: the plan

Where the port stands on Linux and macOS, what is in the way, and the order to do it in. The decisions are at the end;
work started on 2026-11-07.
File references are as of v0.2.7.

## Where it stands

**The game (`shin`) is mostly portable already.** It came from upstream shin with Linux and macOS in mind:

- Rendering is wgpu (`shin-render/src/init.rs:159`, all backends): Vulkan or OpenGL on Linux, Metal on macOS.
- Audio is kira over cpal (ALSA on Linux, which reaches PulseAudio and PipeWire; CoreAudio on macOS). Controllers are
  gilrs. Windowing is winit, sized in logical pixels with the scale factor passed through (`shin-window/src/lib.rs`).
- Files: positional reads have a Unix implementation (`shin-primitives/src/stateless_reader/sys/unix.rs`); data,
  saves and exports use `dirs_next` (`~/.local/share/shin`, `~/Library/Application Support/shin`).
- The Pages workflow already type-checks every `shin-*` crate on Ubuntu (`cargo doc`), so the code compiles there.
  Nothing has been run there yet.

**The launcher does not build off Windows.**

- `windows-registry` is an unconditional dependency, but the crate is Windows-only. `install.rs` is all Windows: the
  self-installer, `.lnk` shortcuts through PowerShell, the Installed apps entry, `%LOCALAPPDATA%\Programs`, deleting
  itself after exit.
- `find_shin()` looks for `shin.exe` (`main.rs:110`). The updater downloads and runs `OpenCatbox-Setup.exe`
  (`main.rs:464`).
- `home.rs` reads `%APPDATA%`. The UI builds a `\Open Catbox` path and talks about the Start menu.
- The Switch dump reader (`src/nx/`), setup, translations, music and version history are plain Rust and portable.

## Movies (the hard part)

The game's movies are H.264. The decoder is chosen in `shin-video/src/h264_decoder/mod.rs`:

| | Today | Plan |
|---|---|---|
| Windows | Media Foundation (built into Windows), ffmpeg as a fallback | unchanged |
| Linux | `ffmpeg` on the PATH, or a build with the `gstreamer` feature | GStreamer: most desktops have it; H.264 needs gst-libav or openh264 (Fedora: RPM Fusion) |
| macOS | `ffmpeg` on the PATH | a new VideoToolbox backend (built into macOS), shaped like `media_foundation.rs` |

Using the system's decoder keeps the project out of H.264 patent licensing: Windows, macOS and the distro carry
it. Bundling an ffmpeg or openh264 build would not. Telling players "install ffmpeg" works, but it is a poor first
experience.

## The order of work

### 1. The game on Linux

- A CI job that builds and tests `shin` on Ubuntu (packages `libasound2-dev libudev-dev`, plus the GStreamer
  development packages).
- Default to borderless fullscreen off Windows. Exclusive fullscreen, today's default (`port_settings.rs:191`), does
  not exist on Wayland and misbehaves on macOS.
- Case-sensitive file systems: lookups inside `data.rom` already match exactly. Loose files (a translation's
  `patch/` folder, `exefs`) are read from disk, where `Data` and `data` differ, so lower-case names on lookup as
  `chars.rs:542` already does.
- Play Episode 1 through on a Linux machine: text, voices, a movie, a save and a load, a controller.

### 2. The game on macOS

- The VideoToolbox movie decoder.
- macOS keys: Cmd+Q quits, Cmd+Ctrl+F toggles fullscreen (Macs have no F11 key and often no Insert). The bindings
  already use physical keys (`bindings.rs`), so keyboard layouts are fine.
- Check Retina scaling, and that the capture tools assume 100 % scale.
- Build for both Apple Silicon and Intel (`universal-apple-darwin`, `lipo` for shin).

### 3. The launcher everywhere

- `windows-registry` and `install.rs` behind `cfg(windows)`. On Linux and macOS there is no installer screen: the
  package installs the app.
- `find_shin()`: `shin` next to the launcher in the AppImage, `Contents/MacOS/shin` in the app bundle (Tauri's
  `bundle.externalBin`).
- The home: the portable home (beside the program) cannot work inside an AppImage (read-only) or a signed `.app`
  (writing into it breaks the signature). Use the per-user folder there (`dirs_next`, the same as shin's).
- UI wording: no "Start menu" or "Windows user" off Windows. Build paths with the platform's separator.

### 4. Packages and updates

- **Linux:** an AppImage (runs on most distros, updates itself through the Tauri updater) and a `.deb`. A `.desktop`
  file for the menu. Build on the oldest Ubuntu still supported, so the AppImage runs on older systems.
- **macOS:** a `.dmg` holding the `.app`; the updater replaces the app from a `.app.tar.gz`.
- **The update feed:** `latest.json` gains `linux-x86_64`, `darwin-aarch64` and `darwin-x86_64` next to
  `windows-x86_64` (`make-latest-json.ps1` writes only Windows today). Off Windows the launcher uses the updater
  plugin's own install, not the Setup handoff.
- **CI:** a GitHub Actions matrix (`ubuntu-22.04`, `macos-14`, and Windows if CI takes over the release from
  `release.ps1`) building with `cargo tauri build`. The update signing key becomes a repository secret. The scan
  for user names and paths in the binaries gets a bash version.

### 5. macOS signing (a decision for the maintainer)

macOS blocks apps that are not signed and notarized. Since macOS 15, players can only open such an app from System
Settings → Privacy & Security, and many will not find it. Signing needs an **Apple Developer ID (99 USD a year)**;
Tauri then signs and notarizes in CI from secrets. Without it, the macOS release ships with instructions for opening
it anyway. Linux needs nothing like this.

## Risks

- **Controllers on Linux:** gilrs reads `/dev/input`, which needs the player in the `input` group or a udev rule on
  some distros. The Switch Pro Controller over Bluetooth maps differently on macOS.
- **Wayland:** window placement and exclusive fullscreen are not available. Borderless fullscreen works.
- **GPU drivers:** old OpenGL-only Linux machines fall back to wgpu's GL backend, which is the least tested path.
- **Testing:** CI can build and run the unit tests on both, but playing them through needs real Linux and macOS
  machines.

## Status (2026-11-07)

Done, in this order of the plan:

1. **The game on Linux:** builds and passes every test on Ubuntu (CI on 22.04, and 24.04 in WSL). GStreamer is
   always built there (`shin-video/src/h264_decoder/linux.rs`: GStreamer when it has an H.264 decoder, else ffmpeg,
   else the movies are skipped). In WSL it decodes the game's `no81` and a B-frame movie in display order, renders
   Episode 1's lines within 32 of the Windows captures everywhere (mean difference 0.06, software rendering), and
   exits cleanly. Found and fixed on the way: a crash at every exit on Wayland (the surface outlived the display
   connection), no sound device stopping the game (it now plays silently), GStreamer 1.20 reporting the end of a
   movie as an error. Exclusive fullscreen is shown as borderless off Windows; loose asset folders match without
   regard to case.
2. **The game on macOS:** the VideoToolbox decoder (`video_toolbox.rs`, pictures put in display order); Cmd+Q and
   Cmd+Ctrl+F. Built and tested by CI on GitHub's macOS runner (Apple Silicon), which decodes the B-frame movie.
3. **The launcher everywhere:** builds and passes its tests on Linux and macOS. The installer is Windows-only
   (`install/windows.rs`), the home is the user's data folder, updates go through the updater plugin. In WSL the
   `.deb` installs with apt (dependencies resolved), the installed launcher finds the game data and lists the
   music (215 tracks).
4. **Packages and updates:** `release-unix.yml` builds the AppImage, `.deb`, `.rpm` and `.dmg` on a published release,
   signs the update files and adds them to `latest.json`. Built in WSL: 94 MB, 16 MB, 16 MB.
5. **macOS signing:** none, by decision; the README and the website say how to open the app the first time.

Left:

- **Playing it through on real machines.** WSL could not press Play in the launcher (WebKit gets no clicks through
  WSLg) and renders in software; macOS has only been built and tested on CI. A first play on real Linux and macOS
  computers is the next check.
- **The release workflow's secrets** (`TAURI_SIGNING_PRIVATE_KEY`, `_PASSWORD`) must be added to the repository
  before the first release with these packages.

## Decisions (2026-11-07)

1. **Movies:** each system's own decoder: GStreamer on Linux (falling back to ffmpeg when no H.264 plugin is
   installed, as Windows falls back from Media Foundation), VideoToolbox on macOS.
2. **macOS ships unsigned**, with instructions for opening it (System Settings → Privacy & Security → Open Anyway).
3. **Linux packages:** AppImage, `.deb` and `.rpm`.
