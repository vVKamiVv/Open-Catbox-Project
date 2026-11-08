# Android: the plan

Whether the port can run on Android phones and tablets, what is in the way, and the order to do it in. Written
2026-11-08; nothing has been done yet. File references are as of v0.3.1. The decisions for the maintainer are at the
end.

## Where it stands

**Some groundwork came from upstream shin:**

- **Window:** winit is built with `android-native-activity` (`shin-window/Cargo.toml:37`). `run_winit` already takes the
  `AndroidApp` and hands it to the event loop (`shin-window/src/lib.rs:1055-1064`), and the desktop-only window
  attributes are skipped there (`lib.rs:203`).
- **Rendering:** wgpu uses Vulkan, or OpenGL ES where a phone has no usable Vulkan.
- **Returning to the app** once it is running is handled: `resumed` re-creates the surface (`surface_reinit`,
  `shin-render/src/init.rs:242`, called from `lib.rs:661`).
- **Audio:** kira plays through cpal 0.15, which has an Android output (AAudio, OpenSL ES on older phones).
- **File reads:** positional reads use the `unix` implementation (`shin-primitives/src/stateless_reader/sys/mod.rs`),
  which covers Android.
- **Touch:** the Switch game was made for a touch screen, and the port already reproduces its gestures (tap, long
  press, drag, flick: `shin/src/ui/gesture.rs`). They are fed by the mouse, and winit's touch events are not read
  (`shin-input/src/raw_input_state.rs:294`). Touch only has to be routed into them; nothing needs redesigning.

**Missing in the game:**

- **No Android entry point.** Android loads a native library (`cdylib`) and calls `android_main`. shin is a normal
  program with a `main`.
- **Going to the background.** There is no `suspended` handler (`lib.rs:618` has only `resumed`). On Android the
  window's surface is destroyed when the player switches apps, and drawing to it then crashes. It must be dropped,
  drawing and sound paused, and the save written, since Android can end a backgrounded app without warning.
  Switching away while the game is still starting reaches `todo!()` (`lib.rs:644` and `649`).
- **Folders.** `dirs_next` has no answer on Android, and the game asks it for:
  - the save (`shin/src/savedata.rs:52`);
  - the game data (`shin/src/asset/system/locate.rs:110`);
  - exports from the galleries (`shin/src/export.rs:232,256`, `shin/src/character_box.rs:330`).

  These come from the app's own storage (`AndroidApp::internal_data_path` / `external_data_path`) instead.
- **Movies.** In `shin-video/src/h264_decoder/mod.rs`, Android falls into the last branch, which starts an ffmpeg
  program; that cannot work there. It needs its own decoder: see below.
- **Controllers.** gilrs 0.11 has no Android backend. Android reports controller buttons and sticks to the app as key
  and motion events, which shin-input would map instead.
- **PC-only settings.** Window mode, window size, V-Sync, F11, and the keyboard bindings and their prompts have no
  meaning on a phone. The PC tab of the config menu needs a reduced version.

**The launcher does not fit Android as it is.** It is a desktop program that starts `shin` as a separate process
(`launcher/src/main.rs`, `launch`), installs and updates itself, and reads the player's NSPs and `prod.keys` from
paths. An Android app cannot start another program, and files the player picks come as `content://` links rather than
paths. The parts that are plain Rust can be reused as they are: the dump reader (`launcher/src/nx/`), setup, language
downloads and the save manager.

## Movies

A fourth decoder, next to Media Foundation, GStreamer and VideoToolbox, uses Android's own MediaCodec. It goes
through the NDK, from Rust, with the `ndk` crate's `media_codec`:

- H.264 samples from the MP4 track go in, as with VideoToolbox.
- Decoded pictures come out in presentation order.
- The output format varies by phone: NV12 (which `nv12.rs` already converts) or planar YUV 4:2:0. Both must be
  handled.

The risk is phone-specific behaviour, so it needs testing on more than the emulator.

## The order of work

### 1. The game builds and runs in the emulator

- A small `cdylib` with `android_main`, which starts shin's app with the folders above.
- Built with `cargo-ndk` (or `xbuild`) into an APK, for `arm64-v8a` (phones) and `x86_64` (the emulator).
- The game data is copied to the app's folder by hand (`adb push`) for now, from a data folder the PC launcher set up.
- Locked to landscape, full screen with the system bars hidden.

### 2. Background and return

- A `suspended` handler: drop the surface, pause the sound and the game's time, write the save. Then `resumed`
  continues as it does now.
- Replace the two `todo!()`s with waiting for the surface again.
- The system Back button acts as the game's cancel.

### 3. Touch and controllers

- Feed winit's touch events into the gesture recognizer the mouse uses now.
- Map Android's controller key and motion events to shin-input's actions.
- Button prompts follow the last input, as on PC.

### 4. Movies

The MediaCodec decoder, with the same test as on Linux and macOS: decode the ffmpeg B-frame movie and check the frame
count and order.

### 5. Setup on the phone

See decision 1.

### 6. Packages, updates and CI

- **Signing:** the APK is signed with a project key, kept like the updater key (GitHub secrets).
- **Building:** a CI job builds it on Ubuntu with the NDK, uploads it to the release, and the site's Downloads page
  lists it.
- **Updates:** the launcher's updater does not exist on Android. An update is a newer APK from the release page;
  apps such as Obtainium can watch GitHub releases for players who want automatic updates.

## Testing

- **Builds:** CI can build the APK, but it cannot run it.
- **Emulator:** Android Studio's emulator runs on the maintainer's PC. Driving it needs the screen, so only when the
  maintainer agrees.
- **A real phone:** touch, speed, movies and GPU drivers need one, ideally a few different ones. The maintainer's own,
  or players willing to test.

## Risks

- **Storage:** the set-up data folder is about 11 GB (the test `data.rom` is 11,086,969,856 bytes). It must sit in
  the app's own folder, because the game reads it with normal file reads, which `content://` files do not allow.
  Android deletes that folder when the app is uninstalled, together with the saves unless they are exported first.
- **GPU drivers:** phone Vulkan drivers vary in quality. The OpenGL ES fallback may need its own fixes in the shaders.
- **Being ended in the background:** anything not saved before `suspended` is lost.
- **Distribution:** a Play Store listing is unlikely for a port that needs the player's own dump, so the APK is
  installed directly ("install unknown apps").

## Decisions (for the maintainer)

1. **Setup on the phone.**
   - (a) None at first: the player copies a data folder set up by the PC launcher. Least work, but needs a PC.
   - (b) The existing launcher UI, built with Tauri 2's Android support, and the game as a second screen of the same
     app. It reuses everything, but needs a small Kotlin part to switch between the two, and two UI stacks in one app.
   - (c) Setup screens drawn by the game itself. One app, but a second setup UI to maintain.

   Suggested: (a) first, then (b).
2. **The oldest Android supported.** Suggested: Android 8.0 (API 26), where AAudio and the NDK media API are
   available; wgpu falls back to OpenGL ES where Vulkan is missing or broken.
3. **Phones only, or tablets and Chromebooks too.** Same APK; mostly a question of what gets tested.
4. **The package name and signing key.** Changing either later breaks updates for installed copies, so they are fixed
   once. Tauri's Android name, `io.github.vvkamivv.opencatbox`, would fit.
