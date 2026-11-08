# How the port is put together

The Open Catbox Project is a Cargo workspace built on [DCNick3/shin](https://github.com/DCNick3/shin). This page
gives the overall shape: the crates, what happens in one frame, how assets are loaded and which threads run. The
original engine's behaviour is in [engine-internals.md](engine-internals.md); the port-only additions are its
§18-§21. Everything here was read from the code; file paths are relative to the repository root.

## Crates

| Crate | Role |
|---|---|
| `shin` | The game: the app and its scenes (`app.rs`), the adventure screen with the VM's commands and menus (`adv/`), layers (`layer/`), transitions (`wiper/`), audio players (`audio/`), assets (`asset/`), galleries (`picture_box.rs`, `music_box.rs`, `movie_box.rs`, `character_box.rs`), title flow (`title_demo.rs`, `title.rs`, `scenario_select.rs`), shared UI (`ui/`: gestures, button prompts, the touch effect), port settings and bindings, export, capture. The binary is still called `shin` |
| `shin-core` | Everything without a GPU or a speaker: the data formats (`format/`: ROM, PIC, BUP, MSK, TXA, FNT, NXA, SNR, SYSSE, save, executable, text encoding), the script VM (`vm/`: `Scripter`, the command types), the message text parser and layouter (`layout/`), time and tweens (`time/`), fixed-point rationals |
| `shin-render` | A thin layer over wgpu shaped like the original's render abstraction over NVN: `RenderPass`, the render programs with their arguments (`RenderProgramWithArguments`), the pipeline cache, a per-frame dynamic buffer, render textures that follow the canvas size, GPU textures |
| `shin-render-shaders` | The WGSL programs (`wgsl/`). Its build script writes `types.wgsl` from the Rust types of `shin-render-shader-types`, validates every shader with naga, compiles it to SPIR-V and generates typed bindings (`shaders.rs`); the WGSL source is the fallback where SPIR-V is not supported |
| `shin-render-shader-types` | Uniform, vertex, buffer and texture types shared by Rust and the shaders, and `RenderClone` (deep copies that duplicate GPU buffers and textures, used to copy layers) |
| `shin-audio` | Audio over kira (cpal output): NXA streams with loop points, volume/pan/stop tweens, the original's linear pan balance, the wait status bits, and each stream's peak level for lip-sync |
| `shin-video` | Movies: MP4 demuxing, AAC through symphonia, H.264 through each system's own decoder (Media Foundation on Windows, GStreamer on Linux, VideoToolbox on macOS), an ffmpeg pipe when that cannot start, else no movies; frames uploaded as NV12 textures |
| `shin-input` | Raw input (winit keyboard and mouse, gilrs gamepads), per-action states (held, clicked, repeats), the pointer on the 1920x1080 canvas, the "Confirm button" swap |
| `shin-window` | The winit event loop and wgpu setup, the `ShinApp` trait the game implements, the frame loop (frame cap, update, render, supersampling, present), display settings, tracing |
| `shin-tasks` | The thread pools: a rayon compute pool and an async I/O executor (`compute::spawn`, `async_io::spawn`, `block_on`) |
| `shin-primitives` | Small shared pieces: bit readers and writers, colours, `Exclusive`, a stateless (positional) file reader, frame ids |
| `shin-derive` | Procedural macros: the VM command types (`Command`), typed texture archives (`TextureArchive`), vertices, `RenderClone`, `rat!` rationals, and the syntax-tree helpers of `shin-asm` |
| `shin-asm` | A work-in-progress assembler for a text form of SNR bytecode (lexer, parser, HIR, SNR writer) |
| `sdu` | Command-line tool: extract and convert every format (`rom`, `scenario`, `picture`, `mask`, `font`, `bustup`, `texture-archive`, `audio`, `savedata`, `sysse`, `assembler`) |
| `launcher` | The Open Catbox launcher (Tauri 2; Windows, Linux, macOS): setup from the player's NSPs, translation downloads, the saves, starting the game, installing (on Windows) and updating itself. Separate from the engine; see [launcher.md](launcher.md) |

`shin` uses every engine crate and nothing depends on it; the lower crates share `shin-primitives`. The launcher
does not link the engine: it only starts `shin` (`shin.exe` on Windows).

## Start-up

`shin/src/main.rs`: tracing, the command line (`cli.rs`), a deadlock watchdog thread, the task pools, then
`shin_window::run_window::<App>`. `App::init` (`app.rs`) creates the audio manager, finds the assets and the
executable's texts (engine-internals §20), loads the save file and `settings.json`, the fonts and the shared textures,
and opens the first scene: the opening movie (or the title if it cannot play), or with `--new-game`, `--load`,
`--fast-forward-to`, `--unsafe-entry-point` or `--capture-messages` the story at once.

## Scenes

The app holds one scene at a time (`app.rs`, `enum Scene`): `TitleDemo` (the opening movie), `Title`, `ScenarioSelect`,
`Bridge` (the doors into a new game), `Adv` (the story), and the galleries `PictureBox`, `MusicBox`, `MovieBox`,
`CharacterBox`. A scene reports where to go (`take_exit`, `is_done`, a load request); `App::switch_scene` builds the
next one and crossfades from a snapshot of the old screen (or wipes through a mask). One BGM player is handed from
scene to scene. Characters, Tips, the manual, Config, the save/load screens, the backlog and the dialogs are not
scenes but tasks: windows in a scene's task pool, the top one taking the input (engine-internals §15).

## One frame

`shin-window` (`RedrawRequested` in `shin-window/src/lib.rs`) drives everything on the main thread:

1. **Pace.** With a frame cap, sleep until the next frame is due. The time since the last frame is the frame's
   `elapsed`.
2. **Input.** The winit events gathered since the last frame and the gamepad events polled from gilrs become a
   `RawInputState`. `App::filter_input` records held keys for the Controls screen, notes the device for the button
   prompts and lets the `--dev` overlay take the clicks it wants. The bindings lower the raw state to `AppAction`s
   (`bindings.rs`), whose held/clicked state is tracked over time (`shin-input`), and the mouse becomes a `Pointer` in
   1920x1080 canvas pixels.
3. **Update** (`App::update`, one command encoder). `--dev` time control may scale or stop `elapsed`; it becomes
   `delta_ticks` (60 Hz frames, engine-internals §18). The current scene updates. For the story (`Adv::update`): the
   task pool first (an open menu takes the input), then the touch gestures and the story's buttons, then the
   **VM loop**: the running command updates; when it finishes, `Scripter::run` executes bytecode until the next
   command, which is applied to the scene model (`apply_state`) and started; a command that needs time yields to the
   next frame (`adv/command/mod.rs`, engine-internals §10). Then the layer tree updates its tweens and animations, and
   **pre-renders**: layers with effects and transitions draw into their own render textures now, into the update
   encoder (`PreRenderContext`). The HUD, the voice focus and the sound players follow.
4. **After the update.** The save file is written if something asked for it; a scene change is made; the touch effect
   updates and, while a ripple plays, renders the finished frame once more to refract it; `--capture-messages` queues
   a readback.
5. **Render.** New display settings (window mode, size, V-Sync, supersampling) are applied. `App::render` draws the
   scene, the scene fade and the touch effect into the canvas: the window's largest 16:9 area, or with supersampling a
   larger texture filtered down afterwards. For the story the order is: the layer tree (root group: the screen layer
   with its four planes in a page, and the message layer), the choice menu, the system menu's blurred backdrop, the
   HUD (banners, icons, notifications), then the tasks (menus) on top. `--dev` overlays are drawn last with egui.
6. **Submit and present.** The dynamic buffer's uploads, the update encoder and the render encoder are submitted
   together; the surface texture is presented and the next redraw requested.

## The story's layer tree

`AdvState::root_layer_group` (`layer/root_layer_group.rs`) holds two layers: the **message layer** (the text window,
`layer/message_layer/`) and the **screen layer** (`layer/screen_layer.rs`). The screen layer shows a **page layer**
(`page_layer.rs`) through a transition: PAGEBACK snapshots the page and WIPE plays a wiper (`wiper/`) from it to the
new one. The page has four **planes**, each a `LayerGroup` (`layer_group.rs`) of layer banks holding the script's
**user layers** (`layer/user/`: picture, bustup, tile, movie, rain, the quiz family and an empty layer). Each drawable
layer has the original's numbered properties with a tweener each (`properties.rs`, LAYERCTRL), wobblers
(`wobbler/`) and the effect chain (`new_drawable_layer/`: blur, mosaic, raster, ripple, dissolve, ghosting).
Engine-internals §6, §7 and §11-§13 and [audit/layers.md](audit/layers.md) give the original's rules.

## Assets

`AssetServer` (`shin/src/asset/system/server/`) reads files through the layered I/O (patch, then data:
engine-internals §20) and turns them into typed assets (`Asset::load`, an async function): pictures, bustups,
masks, texture archives, fonts, audio, movies, the scenario. A load first looks for a live copy (assets are cached as
weak references by path and arguments), then reads the bytes and decodes them; heavy decoding goes to the compute pool.
Callers either spawn the load on the async I/O executor and poll it each frame (LAYERLOAD, MASKLOAD, WIPE, the save
restore) or block on it (`load_sync`) for small system assets at start-up. The scenario's info tables map the script's
ids to file names (`shin-core/src/format/scenario/info.rs`).

## Saving

The save file (`savedata.rs`, the Switch's own `SAVEDATA` format, `shin-core/src/format/save/`) is read at start-up and
written whole when the original writes it (engine-internals §14). A save point is a script position reached by replay,
not a snapshot of the scene: loading replays the script from the start without drawing, then rebuilds the scene
(`adv/restore.rs`). The same replay serves the save list's thumbnails, the backlog, rollback and the chapter jumps.
The port's own settings live apart in `settings.json` (`port_settings.rs`).

## Threads

| Thread | What runs there |
|---|---|
| Main | The winit event loop, every update and render, the VM, all game state |
| Compute pool (rayon, one thread per core) | Picture, bustup, mask, texture-archive and glyph decoding; voice lip-sync tables; save-list thumbnails' replays; gallery export; restarting looping movies |
| Async I/O (one thread, `async-io`) | Asset loads spawned by commands and the restore; the ffmpeg pipes' readers and writers |
| Audio (kira's cpal stream) | Mixing; game code only sends commands to it through `AudioManager` |
| Video (`media-foundation-h264` on Windows, `video-toolbox-h264` on macOS, one per movie; GStreamer's own threads on Linux) | H.264 decoding, up to 8 frames ahead; with the ffmpeg fallback, an `ffmpeg` child process per movie |
| Watchdog | Every 10 s, logs `parking_lot` deadlocks with backtraces |

Game state is only touched on the main thread; the other threads hand results back through tasks
(`ComputeTask`/`AsyncTask`, polled each frame) or channels.

## What the port adds

Not in the original, all marked as such in the code and in engine-internals: the PC settings, window modes,
supersampling and frame cap, frame-rate independence, rebindable controls, controller button prompts and the confirm
button choice (§18); saving from the galleries (§19); the asset location, `--patch-dir` and `--exefs` (§20); the
developer tools (§21); the mouse hover over choices and the hold-to-skip key (§17.2, `AppAction::HoldSkip`).
