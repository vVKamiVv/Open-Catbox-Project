# Changelog

What changed in each release.

## v0.3.1 — 2026-11-07

- **Several saves:** pick which save to play with on the Play screen. **New save…** starts the game from the
  beginning without touching your other saves. Window and control settings are shared by all of them.
- **Saves** in the launcher: each save's slots and play time, backups you can restore, export or delete, and an
  automatic backup each time you press Play. Import a save from there too; files the game cannot read are refused.
- Config menu: switching tabs after scrolling the PC tab no longer leaves the other tabs scrolled down.

## v0.3.0 — 2026-11-07

- **Linux and macOS:** the game and the launcher run there too. Linux: an AppImage, a `.deb` and an `.rpm`, with movies
  through GStreamer. macOS 11 or later (Apple Silicon and Intel): a `.dmg`, with movies through VideoToolbox; the app
  is not signed by Apple, so it needs allowing once in System Settings (see the README).
- **Please note:** Linux and macOS should work, but have not been tested as thoroughly as Windows, and the macOS
  version has not been tried on a real Mac at all yet. Problems are welcome as issues.
- A movie that has ended keeps its last frame on screen, as on the Switch, instead of going blank.
- Random screen shakes move independently left-right and up-down again, instead of along a diagonal.
- Wipes start on the frame they are called, as on the Switch, instead of one frame later.
- Skipping no longer waits for movies to play out, as on the Switch.
- A computer without a working sound output plays silently instead of the game stopping at the start.
- When the system cannot decode the movies, they are skipped instead of the game stopping.
- Website, and issue forms for bugs, differences from the Switch version and ideas.

## v0.2.7 — 2026-11-07

- Launcher music player: tracks no longer loop forever, and it has shuffle, repeat (list, one track or off) and playlists.
- The music player has its own settings in Settings → Music: how many times a track loops, the fade-out at its end, and whether it keeps playing when the game starts.

## v0.2.6 — 2026-11-07

- System menu: long scene titles wrap between words instead of mid-word.
- Down advances the story, as on the Switch.
- A damaged or unreadable save file is now explained on the title screen instead of only in the log.
- Play time also counts on the title and in the galleries.
- With the Confirm button set to Bottom, the "skip this event" hint shows the right button.
- The speaker's name box is the right width again.
- In Tips, Config and the backlog, text fades out under the window's edge as on the Switch.
- The mouse cursor hides after 3 seconds without moving.
- Launcher music player: a search box, a seek bar, and crossfades between tracks (set the length in Settings).
- The launcher checks for updates while it stays open, and Version history has a Check for updates button.

## v0.2.5 — 2026-11-07

- Launcher: a music playlist beside Play, with the game's soundtrack from your game files. Hide it any time.
- Picture Box: button help for Zoom, Hide UI and Previous, and L goes back to a picture's previous version.

## v0.2.4 — 2026-11-07

- Story swipes, as on the Switch: right opens the menu, down hides the text box, left shows the touch buttons
  (auto, skip, backlog, previous and next chapter).
- Overwriting a save updates its picture right away.

## v0.2.3 — 2026-11-07

- Lip-sync fixed: mouths open fully when speaking and close when silent, as in the original.

## v0.2.2 — 2026-11-07

- **Exit** in the title menu.
- The PC tab and Controls screen now match the game's lettering.
- Launcher: import a save file from Settings.
- Launcher: "The game is starting…" no longer stays on screen.

## v0.2.1 — 2026-11-07

- The launcher keeps everything in its own folder: settings, game data and saves. Your existing setup moves in
  automatically.
- Setup can use a game folder you already have.
- Shorter, clearer text in the launcher.
- Release notes display properly.

## v0.2.0 — 2026-11-07

- The launcher now installs itself: no administrator rights, with shortcuts and an uninstaller.
- New **PC** tab in Config: window mode and size, V-Sync, frame-rate cap, supersampling, button prompts and the
  confirm button.
- **Controls**: rebind every action for keyboard, mouse and controller.
- Button prompts match your device: Xbox, PlayStation, Switch or keyboard.
- **Save from the galleries**: pictures, music, movies and character sprites.
- Runs at the right speed on 120 and 144 Hz screens.
- 99.7 % like the original.

## v0.1.0 — 2026-11-06

- First release: the whole game, played from your own Switch dump.
- Every menu, the galleries, voices, music, movies and visual effects.
- Mouse, touch and controller support.
- The launcher: sets up the game and the translations, and updates itself.

## Before the fork (upstream shin 0.8.1 and earlier)

The history of the upstream [shin](https://github.com/DCNick3/shin) engine up to 0.8.1, where this port forked
from it.

### Version 0.8.1

This version only contains some improvements to savedata handling in sdu.

- Fix a bug where some savefiles were not decoded properly due to several missing `.align` calls
- Update naming of some savefile fields to reflect latest RE
- Implement savefile encoding, accessible via `sdu save encode`. This allows one to edit the decoded savefile and to
  insert it back into the game.

### Version 0.8.0

- Implement wipes. Now instead of flickering on, the scene changes are animated, like in the original implementation.
- Implement novel mode messagebox.
- Implement LayerGroup rendering with masks (MASKLOAD command).
- Now we release builds for aarch64 linux (because why not).

- Add fallbacks from SPIR-V shaders to wgsl ones, allowing us to run on platforms not supporting SPIR-V shaders.
- Rewrite `shin-tasks`. Now it is no longer a fork of `bevy_tasks`, but a much simpler library setting up rayon &
  async-executor tasks pools. May support wasm in the future.

- Relax looping assertion in shin-core to allow more NXA files to be decoded.
- Add a new bustup extraction mode into `sdu`: `bake`. It will combine all the bustup parts, allowing to get a sprite
  that looks close to its in-game appearance without fiddling with files.
  This representation is, however, very inefficient in terms of storage space.

### Version 0.7.0

This is a release that contains big rewrites under the hood. They might not improve compatibility that much initially (I
would expect the compat to be worse, actually), but will make implementing some engine features possible at all.

- Rewrite of the rendering engine to actually use the shaders/render passes in an equivalent to the way original game
  does it.
- Addition of `shin-window`: a common framework for starting a winit window, initializing wgpu and handling input. This
  makes test apps (like `shin-video`'s play example) easier to maintain. This is also where most of the web support will
  be contained.
- Rewrite of the input handling to somewhat resemble what the original game does.
- Rewrite of the `MessageLayer` to include all the features the original game has. Now we have messagebox sliding
  animations, overflow handling, keywait animations and voice support (though this still needs implementation from
  the audio engine side).
- Rewrite of the `LayerGroup`, `PageLayer` and `ScreenLayer` classes, which are much closer to what the game does.
- Stub implementation of `NewDrawableLayer` framework, paving the way for implementing various funny effects the script
  uses sometimes.
- Rewrite of `PictureLayer` and `BustupLayer` to no longer use stitched together textures (RIP), but do it the same way
  game does, by rendering in blocks. This indirectly fixes the ugly face seams we were getting due to some rounding
  bugs.

### Version 0.6.1

This release adds support for getting raw opus audio from nxa files to `sdu`. This allows preserving audio data without
additional re-encoding losses.

It also updates various dependencies of the engine, but this shouldn't change the visible functionality.

### Version 0.6.0

This is just an update of the changes accumulated over the year. This doesn't improve compat, mostly small internal
changes

- Add a very basic linear scenario disassembler. The output format is not stable yet, probably should work on making it
  compatible with what `shin-asm` expects.
- Implement & expose in sdu `shin-asm`: a way-too-much over-engineered assembler for SNR files. It is still largely
  unfinished and can assemble only very basic files.
- Add an optional gstreamer backend to `shin-video`, allowing for hardware-accelerated decoding. Not built by default,
  unsure on how to distribute on windows yet.
- Use typed `NumberSpec` values, which will get lowered to a more concrete type than `i32` when computed.
- Various dependency updates.

### Before that

Before this, I did not keep any changelog, sorry. Looking at the commits is the best you will get
