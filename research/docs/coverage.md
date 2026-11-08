# Documentation coverage

Is all of the engine documented? This table lists every crate and subsystem of the repository and where it is
described. "Before" is the state at v0.2.0 (November 2026), "Now" after the documentation pass that added
[architecture.md](architecture.md), engine-internals §19-§22 and the module docs listed here; rows added for later
parts have "–" before.

- **documented**: what it does and how it fits is written down (doc section, README, or module doc comment).
- **partial**: mentioned, or only part of it is described.
- **missing**: nothing beyond the code itself.

Paths: `EI §n` is [engine-internals.md](engine-internals.md); `audit/…` is [audit/](audit/); module docs are the `//!`
comments at the top of the named file.

## The game (`shin`)

| Subsystem | Where documented | Before | Now |
|---|---|---|---|
| App, scenes and scene switching (`app.rs`) | EI §15 (the original's scenes); [architecture.md](architecture.md) (Scenes, One frame); `app.rs` module doc | partial | documented |
| Frame loop: input → update → render → present | [architecture.md](architecture.md) (One frame); `shin-window/src/lib.rs` module doc | missing | documented |
| Story screen (`adv/mod.rs`): VM loop, task pool, story buttons | EI §10, §15; [audit/commands.md](audit/commands.md); `adv/mod.rs` module doc | documented | documented |
| Script commands (`adv/command/`, 49 used) | EI §10 (inventory, semantics); audit/commands.md (every command); `adv/command/mod.rs` module doc | documented | documented |
| Scene model (`adv/vm_state/`) | EI §14 (context fields); `vm_state/mod.rs`, `save.rs` module docs | partial | documented |
| Saving, loading, replay, restore (`adv/vm_state/save.rs`, `adv/restore.rs`, `savedata.rs`) | EI §14; module docs | documented | documented |
| HUD: scene and song titles, icons, notifications (`adv/hud.rs`) | audit/commands.md, audit/text.md; alignment-plan #7; module doc | documented | documented |
| Menus: dialog, system menu, hidden window, config, save/load, thumbnails, jump dialog, backlog, chapter list (`adv/menu/`) | EI §15, §17.1; module docs | documented | documented |
| Characters, Tips, manual (`adv/menu/chars.rs`, `tips.rs`, `manual.rs`) | EI §15, §17.1; module docs | documented | documented |
| Title flow: opening movie, title, scenario select, bridge | EI §15; module docs | documented | documented |
| Galleries: Picture, Music, Movie, Character Box | EI §15, §17.1; module docs | documented | documented |
| Gallery export (`export.rs`, port-only) | EI §19; `export.rs` module doc (its keys corrected) | partial | documented |
| Layer tree and containers (`layer/`: root group, screen, page, planes) | [audit/layers.md](audit/layers.md); [architecture.md](architecture.md); `layer/mod.rs` module doc | partial | documented |
| Layer properties, tweeners, wobblers | EI §2; audit/layers.md | documented | documented |
| Picture, bustup, tile and movie layers | audit/layers.md; EI §8, §16 | documented | documented |
| Rain layer | EI §6 | documented | documented |
| Quiz, cake game, culprit game, last choice | EI §13; module docs | documented | documented |
| Choice menu (`select_layer.rs`) | EI §12; module doc | documented | documented |
| Layer effects (blur, mosaic, raster, ripple, dissolve, ghosting) | EI §11; audit/layers.md | documented | documented |
| Transitions (`wiper/`) | EI §7; `wiper/mod.rs` module doc | documented | documented |
| Message window and text layout | [audit/text.md](audit/text.md); EI §10; `message_layer/mod.rs` module doc | documented | documented |
| Fonts (`asset/font.rs`, FNT format) | audit/text.md (which fonts, where); EI §22 (format) | partial | documented |
| BGM, sound effects, system sounds (`audio/`) | EI §10 (status bits, SEONCE, SYSSE), §15 (shared BGM); audit/systems.md; `audio/mod.rs` module doc | documented | documented |
| Voices, panning, lip-sync | EI §16 | documented | documented |
| Movies (`layer/user/movie_layer.rs`) | EI §8, §15 (transparent movies) | documented | documented |
| Assets: server, cache, ROM layering, `..` paths (`asset/`) | EI §3, §20; [architecture.md](architecture.md) (Assets); `asset/mod.rs` module doc | partial | documented |
| Asset location and `--patch-dir` languages (`asset/system/locate.rs`) | EI §20; README; launcher.md | partial | documented |
| Executable texts and IPS (`asset/executable.rs`) | EI §3, §20; module docs | documented | documented |
| Bindings and Controls screen (`bindings.rs`, `config_menu/controls.rs`) | EI §18; module docs | documented | documented |
| Button prompts (`ui/prompts.rs`) | EI §18; module doc | documented | documented |
| Confirm button option (`bottom_confirms`) | EI §18; `shin-input/src/lib.rs` module doc; README | missing | documented |
| Touch gestures and scrolling (`ui/gesture.rs`) | EI §17, §17.1; module doc | documented | documented |
| Touch effect (`ui/touch_effect.rs`) | EI §17.2; module doc | documented | documented |
| Port settings and the PC tab (`port_settings.rs`) | EI §18; module doc | documented | documented |
| Window modes, supersampling, frame cap | EI §18 (Rendering); `shin-window/src/lib.rs` module doc | documented | documented |
| Frame-rate independence | EI §18 | documented | documented |
| `--capture-messages` (`capture.rs`) | EI §21; alignment-plan.md; reverse-engineering-guide.md; module doc | partial | documented |
| Dev overlays and time control (`--dev`, `render/overlay.rs`) | EI §21; README | partial | documented |
| Other command-line flags (`cli.rs`) | EI §21; `shin --help` | partial | documented |

## Engine crates

| Crate / subsystem | Where documented | Before | Now |
|---|---|---|---|
| `shin-core` formats: ROM2, PIC4 | EI §4, §5; module docs | documented | documented |
| `shin-core` formats: BUP4, NXA1, SYSSE, SNR tables, executable, save, route, text encoding | module docs; EI §14, §22 | documented | documented |
| `shin-core` formats: MSK4, TXA4, FNT4 | one-line module docs → EI §22 | partial | documented |
| `shin-core` VM (`vm/`) | `vm/mod.rs` module doc; EI §10, §14 (random generator, replay) | documented | documented |
| `shin-core` text parser and layouter (`layout/`) | audit/text.md | documented | documented |
| `shin-core` time, ticks and tweens (`time/`) | EI §2 (tweeners), §18 (60 Hz frames); no prose on the easing types beyond audit/layers.md | partial | partial |
| `shin-render` (render passes, programs, pipelines, dynamic buffer, render textures) | crate doc (one line); [architecture.md](architecture.md); render programs in EI §7, §11 and audit/layers.md | partial | documented |
| `shin-render-shaders` (WGSL, build-time bindings) | module doc; [architecture.md](architecture.md); reverse-engineering-guide.md (Shaders) | partial | documented |
| `shin-render-shader-types` | module doc | missing | documented |
| `shin-audio` | module doc; [architecture.md](architecture.md) | partial | documented |
| `shin-video` (MP4, AAC, H.264 decoders) | `shin-video/README.md` (now lists Media Foundation); module docs of the decoders; EI §8 | partial | documented |
| `shin-input` | module doc; [architecture.md](architecture.md) | missing | documented |
| `shin-window` | module doc; [architecture.md](architecture.md) | missing | documented |
| `shin-tasks` and the threads | `shin-tasks/README.md`; module doc; [architecture.md](architecture.md) (Threads) | partial | documented |
| `shin-primitives` | crate doc; [architecture.md](architecture.md) | partial | documented |
| `shin-derive` | per-macro docs; crate doc | partial | documented |
| `shin-asm` | `shin-asm/README.md` (work in progress) | documented | documented |
| `sdu` and its subcommands | README; crate doc; `sdu --help` (every subcommand); [architecture.md](architecture.md) | documented | documented |

## Launcher

| Subsystem | Where documented | Before | Now |
|---|---|---|---|
| Setup from the dump (`setup.rs`) | [launcher.md](launcher.md); module doc | documented | documented |
| Switch dump reader: PFS0, NCA, keys, RomFS, BKTR (`nx/`) | launcher.md; module docs | documented | documented |
| Translations and languages (`langs.rs`) | launcher.md; module doc | documented | documented |
| Starting the game (`main.rs` `launch`) | launcher.md (now with `--fullscreen`) | partial (stale) | documented |
| Install, update, uninstall (`install/`), the move in from `%APPDATA%` (`home.rs`) | launcher.md; launcher/README.md; module docs | documented | documented |
| Auto-updater (signature check, `latest.json`) | launcher.md; launcher/README.md | documented | documented |
| Version history tab (`history.rs`) | launcher.md (Version history); launcher/README.md; module doc | missing | documented |
| Releases (`scripts/`) | launcher/README.md | documented | documented |
| Music player (`music.rs`, `ui/music.js`, v0.2.7) | launcher.md (Music); launcher/README.md; module doc | – | documented |
| Saves: slots, backups, several saves (`saves.rs`, v0.3.1) | launcher.md (Settings and logs); launcher/README.md; module doc | – | documented |
| Linux and macOS: packages, home, updates (`install/other.rs`, `release-unix.yml`, v0.3.0) | launcher.md (Linux and macOS); [linux-macos-port.md](linux-macos-port.md); launcher/README.md | – | documented |
| UI (`ui/`) and design | launcher.md (Design); README ("Wanted: a designer") | documented | documented |

## Research tooling

| Subsystem | Where documented | Before | Now |
|---|---|---|---|
| Ghidra scripts, symbol map, class inventory | [reverse-engineering-guide.md](reverse-engineering-guide.md); [class-inventory.md](class-inventory.md) | documented | documented |
| Comparison with the original (`tools/compare/eden_capture.py`) | reverse-engineering-guide.md; alignment-plan.md | documented | documented |

## Totals

| | Documented | Partial | Missing |
|---|---|---|---|
| Before | 42 | 20 | 6 |
| Now | 70 | 1 | 0 |

71 rows, 68 of them at v0.2.0; the launcher's "Starting the game" counts as partial before (its fullscreen note was
stale).

## Not implemented, and documented as such

The original's layers that Umineko never loads (Animation, Effect, FocusLine), the six unused wipers, and the nine commands the script never uses: EI §2 and §10, audit/layers.md.

## Still open

- `shin-core`'s time module (ticks, tweens, easings) has no prose of its own; the rules it implements are in
  audit/layers.md and EI §2.
- What the shared asset folder in `locate_assets` was meant to be (EI §20).
- The open questions of the original kept in EI §9 and each audit report's last section.
