# Alignment audit: systems (menus, modes, audio/video, assets, main loop, input, settings)

Scope: everything outside the adventure screen's commands, layers and text. Original = Switch v1.0.3 `main`
(Ghidra base 0x7100000000). shin = `H:\Umineko PC Port\shin` as of this audit. Logs and decompiles:
`$S\audit\systems\` (a1.c .. a8.c, d1.txt, lambdas.txt, py\xref.py and py\calls.py: ADRP+ADD and BL scanners over
the NSO, used where Ghidra had no references).

New evidence found in this audit (not in the docs before):
- **Method names** of almost every menu come from `std::function` lambda typeinfo strings in the executable
  (`$S\audit\systems\lambdas.txt`), e.g. `shin::CreateTitle(int)`, `shin::ADV::JumpPrev()`, `shin::SaveMenu::Delete()`,
  `shin::MessageBrowser::QuickLoad(...)`, `shin::ConfigMenu::ResetDefault()`. They give the scene factories, too:
  `CreateFirstBoot`, `CreateTitle(int)`, `CreateTitleDemo`, `CreateADV(GameData const&, int)`, `CreateScenarioSelect`,
  `CreateScenarioSelectBridge(int)`, `CreateBGMMode`, `CreateBustupMode`, `CreateCGMode`, `CreateMovieMode`.
- **Main loop** `0x7100026d50` (each frame): dt = `0x71000fc2b0` (in 60 Hz frames); poll pads `0x71000c4300`; run the
  main-thread callbacks queued under a mutex (`0x710022b8d8`); if a scene factory is pending (`0x710022b930`) destroy the
  current scene (`0x710022b8d0`, vfunc +8) and build the new one (factory +0x30); `scene.update(dt)` (vfunc +0x10);
  **BGM player update `0x7100033eb0` (global, outside the scene)**; TouchEffect update `0x7100115d70`; `0x7100117220`
  (empty); play time; clear to black; `scene.render()` (vfunc +0x18); present `0x71000cf5c0`.
- **Audio volume model** (all confirmed from code):
  - BGM gain = `v0 * 0.01 * duck * track_tween` (`0x7100033eb0`; 0.01 at `0x710015a928`).
  - SE gain = `v1 * 0.01 * tween`, times `duck` only for looping channels (`+0x10 == 0`, i.e. no_repeat = 0)
    (`0x71000f3210`).
  - Voice gain = `v2 * 0.01 * fade` (`0x710011ab70`).
  - System sound gain = `volume * v3 * 0.01` (`0x71000ff620`).
  - Movie gain = `setting * 0.01 * local_volume`, the setting picked by the movie table's volume source: 1 = v0, 2 = v1,
    3 = v2, else 1.0 (`MovieLayer::Shared` slot 4 `0x71000c29c0`).
  - All gains are linear amplitudes (`Audio::VoiceImpl` slot 8 `0x710002bb40` stores the float, clamped at >= 0).
  - **Voice focus**: when the ADV's task pool is empty, ADV update `0x7100010fb0` sets duck target = 0.5 if v4 is set
    and the voice status has bit 2, else 1.0, for BGM (`0x71000337d0`, player +0x2c) and SE (`0x71000f23f0`, +8). The
    duck factor moves toward the target by 0.04 per frame (`0x710015a800` / `0x710015a700`), so 1.0 -> 0.5 takes 12.5
    frames.
- **BGMPLAY** `0x71000337f0(volume, player, id, ?, fade)`: if that id is already the active track, only its volume
  tween is retargeted (no restart, no banner, no unlock). Else every active track fades to 0 over max(fade, 4) frames
  and is dropped when silent (crossfade); the new track's volume tweens 0 -> volume over `fade` frames; the BGM id and
  its linked id are unlocked (Global+0x80); returns 1, and BGMPLAY's execute (`0x7100006bf0`) then shows the
  song-title banner (`0x710009e160`, ADV+0x2e0). In skip mode the fade is forced to 0.
- **SePlayer::play** `0x71000f2580(volume, pan, speed, player, slot, id, fade, no_repeat)`: if the slot already
  plays the same id and both old and new are looping, only volume/pan/speed are retargeted over `fade`; otherwise
  the slot is stopped and the new sound tweens 0 -> volume over `fade`. SEPLAY's execute `0x7100006ec0` passes its
  `no_repeat` as the last argument; **in skip mode a one-shot SEPLAY stops the slot and plays nothing**. Stops
  (`0x71000f2b30`) always fade over at least 4 frames (`max(fade, 4)`).
- **SE status bit 1** (`0x71000f2fd0`): set while the channel's `+0xc` state is 0 = "stream not started yet" (the
  update `0x71000f3210` sets it to 1 when the stream is ready). The doc's guess "not looping" is wrong; shin's
  `AudioWaitStatus::FADING` should be named `LOADING`. Behaviour matches anyway: shin starts sounds synchronously.
- **VoicePlayer** (`0x710011ab70` update, `0x710011a6b0` stop, `0x710011a980` status): a list of voices with state 0
  loading / 1 playing / 2 stopping. All loading voices start together once all are ready; stop moves them to state 2,
  a fade of 0.2 per frame (5 frames, `0x710015a954`); state 2 at 0 or a finished stream is removed.
- **Controller** `0x71000c4300` -> engine bit mask (held +0, pressed +4, repeat +8 / +0xc): bit 0-3 d-pad U/D/L/R,
  4 A, 5 B, 6 X, 7 Y, 8 ZL, 9 ZR, 10 L, 11 R, 12/13 ZL/ZR analog, 14/15 stick clicks, 16-19 left stick, 20-23 right
  stick, 24-27 "any up/down/left/right" (d-pad or left stick, opposite directions cancel), **28 confirm, 29 cancel**
  (A/B, swapped when `0x71000fc1f0` is true). Repeat: first press, then every 4 frames from frame 24, every 2 from 64.
- **ADV buttons** `0x7100011f08` (confirmed): L toggles auto (ADV+0x47c bit 0; sounds "son"/off), R toggles skip
  (bit 1), cancel/X/Y while auto or skip is on turns both off; ZL `0x7100012c80` (JumpPrev, confirmation, setting
  v11 changes its text), ZR `0x7100012f80` (JumpNext); inside an event: confirm shows a hint overlay (+0x4a0: in 32,
  hold 120, out 32), cancel skips the event (`0x710000f600`); otherwise **Y opens the SystemMenu** (`0x7100013320`,
  task 0x298 bytes, ctor `0x710001c100`, sound "page"), **X opens the backlog** (`0x7100013400`, MessageBrowser
  built from the Context's GameData via `0x71000aaf90`; "error" if not possible), **B hides the window**
  (`0x7100013720`, HiddenWindow task 0x88, ctor `0x710001bd80`), **Up (repeat) rolls back to the previous save
  point** (`0x71000137f0`: checkpoint `0x710000f220`, loads it with mode 1, "decide"; "error" if none), confirm or
  Down advances the message (`0x71000b1b10`) and also ends an interruptible WAIT.
- **isSkipping** `0x7100012430` = skip bit on, not in an event, message state allows it, and (v11 == 0 or the
  current message is read, Context+0x2dd9). Auto mode test `0x71000122f0` is the same with bit 0 and no read test.
- **Text speed** (`0x71000b04a0`): the layouter's default draw speed is 100 for message box type 5, else **v9**.
  **Auto-advance** (`0x71000b1ff0` around the `0xba` load): delay = `wait_delay * (100 - v10) * 0.01`.

---

## 1. Table

Status: matches / diverges / stub / missing / structural. Impact on what the player sees or hears: high / medium /
low / none. Fix: S < 1 h, M < 1 day, L more.

### 1a. Main loop, scene and task framework, input

| Original (address) | shin counterpart | Status | What differs / evidence | Impact | Fix |
|---|---|---|---|---|---|
| Main loop `0x7100026d50` | `shin/src/app.rs` `App::update` | diverges | No current-scene/pending-factory switch (App owns one `Adv`); BGM player is updated inside the ADV, not globally; no TouchEffect. Play time counting matches (`adv/mod.rs:314`, same /60 and clamp). | medium (no way to leave the ADV) | M |
| `shin::Scene` (typeinfo `0x71001d3e20`) + factories `Create*` (lambda names, `lambdas.txt`); switch `0x7100027370` (31 call sites) | none | structural / missing | shin has no scene abstraction; see Structural 1 | high (blocks every menu) | M |
| `shin::Task`, `TaskPool` (vtable `0x7100223378`: add `0x7100100930`, isEmpty `vfunc_2`, update `vfunc_3`, render `vfunc_4`), `UITask`, `UpdateTask` | none | structural / missing | Menus are Tasks pushed on the current scene's pool; an open task takes input and freezes voice-focus updates (ADV `0x7100010fb0`) | high | M |
| `AsyncQueue` / `AsyncQueue::Task`, `X::First` loader tasks | `shin-tasks`, `AssetServer::load` | matches (functionally) | Each `First` loads `/x.txa` asynchronously then builds the menu; shin's async asset loads do the same job | none | - |
| Pad input `0x71000c4300`, copy `0x71000c4900` | `shin-input` (gilrs + keyboard + mouse), `app.rs` `AppAction` | diverges | Repeat timings match (`ActionData60Fps` is tick-exact; the dynamic variant approximates). But `AppAction::lower` reads keyboard and mouse only: the gamepad state gilrs collects is never mapped; no confirm/cancel swap; no L/R/ZL/ZR/X/Y equivalents | high for controller players | S |
| ADV button handling `0x7100011f08` (see above) | `adv/mod.rs` `handle_input` | diverges | shin: Enter/Space/click advance, Ctrl **held** = fast-forward of everything (read or not), F5 save. Missing: auto mode, skip toggle, v11 read-only skip, rollback (Up), backlog (X), system menu (Y), hide window (B), JumpPrev/Next (ZL/ZR), event skip, Down = advance | high | M |
| `GestureRecognizer::Listener`, `ADV::OnTap` | `PointerTracker` (`layer/user/minigame.rs`), click = advance | diverges | Touch gestures (swipes for backlog/menus) not modelled; click-to-advance roughly matches a tap | low on PC | M |
| `TouchEffect` (vtable `0x7100224e58`, update `0x7100115d70`, v17) | none | missing | Particle ring at touch points (FirstBoot `0x7100027300` creates it) | low | S |
| `SystemEventDelegate` | `shin-window` focus handling | n/a | Suspend/resume on Switch | none | - |
| `Trophy::Installer`, TROPHY | `adv/command/trophy.rs` (stub) | stub | Nothing to install on PC; could map to achievements | none | - |
| `ScenarioSelectBridge` + `CreateScenarioSelectBridge(int)` | none | missing | Transitional scene that starts an ADV for a scenario id | high (part of game flow) | S |

### 1b. Menus and modes (60 RTTI classes; grouped with their `First`/`Viewer` helpers)

Common shape (confirmed from vtables and lambda names): `X::First` (5-slot Task) loads `/x.txa` (load site below) and
other assets, then replaces itself with `X`; `X` is a 13-slot Task (overlay inside the current scene) or a 12-slot
Scene; lambda names give its methods (`Start` = open animation, `Close`/`Exit` = close animation, `Decide`, `Main`).
Sounds go through `SysSePlayer::play` with names from pointers: "decide" `*0x7100226740`, "move" `*0x71002267e0`,
"error" `*0x7100226748`, "cancel" `*0x71002267d8`, "page" `*0x7100226750`, "son" `*0x7100226738`, "notify1/2"
`*0x7100226718/20`. shin has none of these menus; status is **missing** for every row unless noted.

| Original (address) | What it is / does | Entered from | Depends on | Impact | Fix |
|---|---|---|---|---|---|
| `FirstBoot` (Scene; vtable `0x71001d90b0`; update `0x7100065300`, render `0x7100065720`; `CreateFirstBoot`) | Boot: system assets (`/systex.txa` load `0x7100065d24`, `/system.fnt` `0x7100065ae8`), logo sequence from a table at `*0x7100226cb8` (0x18-byte entries, frame count at +8; 60-frame fades, `0x710015a778`), then the `LoadGame` task (`0x71000d5290`); its callback `0x71000664d0` sets the play-time clock; then `CreateTitle` (`0x7100066530`) | process start | LoadGame | medium | S |
| `LoadGame` (Task `0x7100220860`; read job `0x71000d4890`; `SubErrorAlert`, `InheritAlert` lambdas) | Reads SAVEDATA; corrupt / newer-version dialogs | FirstBoot | dialogs | low | S |
| `SaveGame` (Task `0x7100220828`; job `0x71000d3ce0`; create `0x71000d52f0`) | Writes SAVEDATA (>= 1 s, with an indicator, `SaveGame::Update` lambdas) | EXIT `0x71000147e0`, title return, config close if changed `0x71000609b0`, manual save `0x71000daafc`, delete `0x71000db6c0` | - | low | S |
| shin: `savedata.rs` | **diverges**: writes on F5, EXIT, window close; unreadable file moved aside instead of an alert | | | low | - |
| `Title` (no RTTI for the class itself; `Title::First` `0x7100223fe0` vfunc_3 `0x71001112d0`; `CreateTitle(int)`; methods `StartMenu`, `MenuItemOut`, `Start`, `Decide` (3 lambdas), `Continue`, `DebugMenu`) | Title screen: `/title.txa` (load `0x7100113be4`), background `/title/bg%02d.pic` (`0x7100113df0`, number chosen at runtime); fade-in 60 frames, or 90 frames with property 66 (blur) tweened to 0 when `+0x22c < 0`. Continue = new ADV from `Global.auto` (`0x7100112670`). DebugMenu writes persist slots ("unlock all", `0x7100112890..`). Scene switches from title decisions at `0x710010e368..0x710010e588` | FirstBoot, EXIT, system menu "return to title", TitleDemo exit | SaveMenu, ConfigMenu, ScenarioSelect, extras, ManualViewer | high | M |
| `Title::SubMenu` (Task `0x7100223f08`; `MenuItemIn/Out`, `Decide` x3) | Second-level title menu (inferred: the extras list) | Title | modes below | medium | S |
| `TitleDemo` (Scene `0x7100224a80`; `CreateTitleDemo`; `Start`, `Exit`; 2 `First` lambdas) | Attract demo after idle on the title (inferred) | Title idle | movie playback | low | S |
| `ScenarioSelect` (Scene `0x71002215a8`, update `0x71000de100`; `CreateScenarioSelect`; `Main`, `Decide`, `Close`) | Saku's story-piece selector: `/snrsel.txa` (`0x71000e35b0`); entries' states are persist slots `scenario_id + 1` (spec_save 6.1) | Title (new game), EXIT | persist data, ScenarioSelectBridge | high | M |
| `ADV::SystemMenu` (Task `0x71001d45a0`, ctor `0x710001c100`, update `0x710001d000`; `Decide` + 8 `Decide2` lambdas) | In-game menu: vertical list (0xb8-byte items, up/down with wrap, "move"; confirm `0x710001e010`; cancel/Y "cancel"); shows play time `%d時間 %d分` (`0x710015b467`, capped 999:59) at x 756, size 38. Items lead to save, load, config, title (`0x710001ff98` -> SaveGame) etc. (inferred from the lambdas) | ADV Y | SaveMenu, ConfigMenu, SaveGame | high | M |
| `ADV::HiddenWindow` (Task `0x71001d44c8`, ctor `0x710001bd80`) | Hides the message window until a press | ADV B | - | medium | S |
| `SaveMenu` + `First` (Task `0x7100220c28`; ctor `0x71000d5b40`; `Save`, `Load`, `Delete`, `Decide`, `Start`, `Close`, `Exit`) | 100 manual slots + auto, `/saveload.txa` (`0x71000dc290`); cursor = `save_menu_position`, scroll `clamp(pos*186 - 279, 0, 17856)`; save `0x71000da920`, load `0x71000db120` -> new ADV; thumbnails are **rendered by replaying each slot** (`ThumbMaker::StateBuilder`, spec_save 4.5) | system menu, title | ThumbMaker (replay + off-screen render), SaveGame, ADV restart from GameData | high | L (M without thumbnails) |
| `ConfigMenu` + `First` + `MenuItem` / `ButtonMenuItem` (0x80) / `SliderMenuItem` (0xd0) (Task `0x71001d7dc0`; `InitMenu` `0x7100058e90` with 14 item lambdas; `ResetDefault`; `Close`/`_Close`) | Edits Global settings (ids in spec_save section 7); `/config.txa` (`0x7100060d90`); on close compares with a copy (`0x71000d13c0`) and writes the save if changed | system menu, title | Settings plumbing (Structural 2) | high | M |
| `MessageBrowser` + `First` + `ChapterSelector` + `LogBuilder` + `Layouter` (Task `0x71001dc010`; `Main`, `QuickLoad`, `OnTap`) | Backlog: `/logview.txa` (`0x71000ad9b0`); LogBuilder (ctor `0x71000a3280`, a `Scripter::Listener`) **re-runs the VM over a copy of the GameData and persist data** to rebuild past lines (SGET `0x71000ab130`, SELECT `0x71000ab4a0`, QUIZ `0x71000abed0`); voices can be replayed (v4 read `0x71000a3f84`, v8 read `0x71000a493c`); QuickLoad jumps to a line by starting a new ADV from a GameData it builds (`0x71000ad068`) | ADV X | VM replay without side effects (shin's `replay_command` is the base), message layouter, voice player, ADV restart | high | L |
| `ManualViewer` + `First` (Task `0x71001da678`) | Paged manual images `/manual.txa` (`0x7100087850`) | title, system menu (inferred) | - | low | S |
| `TipsMode` + `First` (Task `0x71002239d0`) | Tips list `/tips.txa` (`0x710010af64`); entries unlocked by TIPSGET (stub in shin) | title / system menu (inferred) | TIPSGET state | medium | M |
| `CharsMode` + `First` (Task `0x71001d76b0`; `ChangePage`, `ChangeChar`, `ChangeStatus`, `ChangeDress`) | Character profiles `/chars.txa` (`0x7100056230`); state from CHARS/SHOWCHARS (stubs in shin) | title / system menu (inferred) | CHARS state | medium | M |
| `CGMode` + `First` + `Viewer` (Scene `0x71001d6ee8`; `CreateCGMode`) | CG gallery `/cgmode.txa` (`0x710004d2a0`) | title extras | `unlocked_cgs` (never written by shin, see 1d) | medium | M |
| `BGMMode` + `First` (Scene `0x71001d5df0`; `CreateBGMMode`; `Close`) | Music room `/bgmmode.txa` (`0x7100033180`); calls `0x71000fc3b0` (v18 related) | title extras | `unlocked_bgms`, BGM player | medium | M |
| `MovieMode` + `First` + `Viewer` (Scene `0x71001daa88`; `CreateMovieMode`) | Movie gallery `/moviemode.txa` (`0x710008b340`) | title extras | `unlocked_movies`, movie playback | low | M |
| `BustupMode` + `First` + `Viewer` (Scene `0x71001d63d0`; `ChangeBustup`, `ChangeBg`, `Hidden`) | Sprite viewer `/bupmode.txa` (`0x710003f164`) | title extras | bustup layer | low | M |
| `CharsMode` typeinfo copies (`0x710005580c` ...) | inventory artefacts (string matches inside code), not classes | - | - | none | - |

### 1c. Audio and video (13 RTTI classes + the RTTI-less SePlayer)

| Original (address) | shin counterpart | Status | What differs / evidence | Impact | Fix |
|---|---|---|---|---|---|
| `VoicePlayer` (vtable `0x71002251b8`; play `0x710011a000`, update `0x710011ab70`, stop `0x710011a6b0`, status `0x710011a980`) | `shin/src/audio/voice_player.rs` | **stub** | `play` returns false, `stop` and status do nothing; VOICEPLAY (`voiceplay.rs`) only logs TODO. **39,649 of 72,998 MSGSETs carry `@v`**: the game is silent where the Switch is voiced. Missing too: 5-frame stop fade, grouped start, v2 gain, v5 pan, v19 per-character filter (`0x71000eadc0`), lip-sync amplitude (`shin-audio` `get_amplitude` unused) | **high** | M-L |
| `BGMPlayer` (vtable `0x71001d61d8`; play `0x71000337f0`, update `0x7100033eb0`, status `0x7100033e00`, position `0x7100033e70`) | `audio/bgm_player.rs` | diverges | (1) v0 never applied (music 1/0.7 = 1.43x louder than default); (2) no voice-focus duck; (3) BGMPLAY of the playing id restarts the track (original: volume retarget only; 4 such sites in linear script order); (4) old track is cut in 15 ms instead of fading over max(fade, 4) frames; (5) no BGM unlock; (6) no song-title banner (v15). Status bits and position match | medium | S |
| SePlayer (no RTTI; ADV+0xc0; play `0x71000f2580`, update `0x71000f3210`, stop `0x71000f2b30`, stopAll `0x71000f2d20`, status `0x71000f2fd0`) | `audio/se_player.rs`, `seplay.rs`, `sestop.rs`, `sevol.rs` | diverges | (1) v1 never applied; (2) no duck of looping channels; (3) **re-issuing the playing looping SE on the same slot restarts it** (original continues and retargets; ~750 such sites in linear order: ambient loops audibly restart); (4) stops with fade 0 cut instantly (original >= 4 frames); (5) **fast-forward still plays every one-shot SEPLAY** (original: skip mode plays nothing, stops the slot); (6) SEONCE/SESTOPALL use fast-forward instead of `isSkipping` (shortcut noted in code). Pan formula in `shin-audio/src/sound.rs:226` is wrong (`pan.sqrt()` is NaN for pan < 0, centre is 0.5) but every SEPLAY/SEONCE in main.snr has pan 0 and speed 1000 | medium | S |
| `SystemSound` (vtable `0x7100223318`; play `0x71000ff620` via `SysSePlayer::play` `0x71000272b0`) | `audio/sys_se.rs` | diverges | v3 not applied (1/0.55 = 1.8x too loud). Only SYSSE uses it in shin; all menu sounds are missing with the menus | low | S |
| `StreamSound` (vtable `0x7100222f80`; set gain `0x71000f5390`) | `shin-audio` `AudioSound` (kira) | matches | Linear gain; loop from the file's loop point; bit 1 "not started" never set because shin starts synchronously (equivalent) | none | - |
| `Audio::Delegate`, `Audio::Voice`, `Audio::Voice::Delegate`, `Audio::VoiceImpl` (slot 8 `0x710002bb40` = set gain) | `shin-audio` + kira/cpal | matches | Linear gain clamp >= 0 matches `Volume` (0..1000 -> 0..1 linear) | none | - |
| `AudioDecoder` / `AudioDecoderImpl` | `shin-core/src/format/audio` | matches | Plays every BGM/SE in use | none | - |
| `Movie`, `MovieImpl` (vtable `0x71001dd4c0..`), `MovieImpl::StreamReader`, `movie::ClientStreamReader`, `MovieLayer::Shared` (`0x71000c29c0`) | `shin-video`, `layer/user/movie_layer.rs` | diverges | (1) settings gain not applied (`movie_layer.rs:59` TODO; original uses v0/v1/v2 by volume source); (2) **repeat unsupported** (8 LAYERLOADs with repeat = 1: the clip stops instead of looping); (3) transparency modes unsupported (warn); (4) AAC seek `todo!` (`shin-video/src/audio.rs:103`). 758 movie loads pass volume 0 (silent in both) and 69 pass 1000 | medium | M |

### 1d. Data, files and assets (20 RTTI classes)

| Original | shin counterpart | Status | Evidence | Impact | Fix |
|---|---|---|---|---|---|
| `SourceMedia`, `MountableSourceMedia(Impl)`, `DataSource`, `FileDataSourceImpl`, `File::Handle(Impl)` | `asset/system/locate.rs` (`LayeredAssetIo`), `shin-core/src/format/rom.rs` | matches | patch dir, patch.rom, data dir, data.rom in that order; engine-internals section 3 (patch.rom first) and section 4 (ROM2 confirmed) | none | - |
| `CachedAssets::CacheDataSource(Impl)`, `RangedCacheDataSourceImpl`, `Data`, `DataImpl`, `MutableData(Impl)`, `RangedDataImpl` | `asset/system/cache.rs`, `server/accessor.rs` | matches (different shape) | Caching and ranged reads; shin's accessor reads synchronously (TODO async) | none | - |
| `TXA4` | `asset/texture_archive.rs` | matches | used by the minigames and messagebox | none | - |
| `Font` | `asset/font.rs` | matches | text area audit | none | - |
| `MaskTexture` | `asset/mask.rs` | matches | MASKLOAD works | none | - |
| `RenderTexture`, `DepthStencil` | `shin-render` | matches | | none | - |
| PIC4, bustup, sysse.bin, scenario, NXA formats | `shin-core/src/format/*` | matches | engine-internals sections 4, 5 confirmed | none | - |
| Global SaveData (`0x7100229218`), file format | `shin-core/src/format/save`, `savedata.rs` | matches (format) / **diverges (contents)** | Format, obfuscation, defaults `0x71000d2110` match (spec_save 6.5, 7). But shin never writes **seen messages** (+0x20; written by the MessageLayer listener `0x7100014f50`/`0x7100014fa0` in the original) nor **unlocked CGs/BGMs/movies** (+0x68/+0x80/+0x98; `0x7100020470`, `0x71000337f0`, UNLOCK): grep finds no writer in `shin/src`. A Switch save keeps its old sets; new play adds nothing | medium now, high once skip/galleries exist | S |
| Settings (Global+0xb0) | `Settings` in `format/save/mod.rs` | **diverges: parsed, never applied** | See 1e | high | S (after Structural 2) |

### 1e. Settings: what each one does and whether shin applies it

| # | Meaning (spec_save 7 + this audit) | Original reader | shin | Status / visible effect |
|---|---|---|---|---|
| v0 70 | BGM volume | `0x7100033eb0`, movies | ignored | diverges: BGM 1.43x louder |
| v1 70 | SE volume | `0x71000f3210`, movies | ignored | diverges: SE 1.43x louder |
| v2 90 | voice volume; 0 = voices not started | `0x710011ab70`, MessageLayer `0x71000b1eb8`/`0x71000b2290` | hard-coded 90 (`message_layer/mod.rs:292`) | stub (no voices) |
| v3 55 | system sound volume | `0x71000ff620` | ignored | diverges: 1.8x louder |
| v4 0 | voice focus (BGM and looping SE x0.5 while a voice plays, 0.04/frame) | ADV `0x7100011670` | missing | missing (no effect at default) |
| v5 1 | voice pans with the speaker | VoicePlayer | missing | stub |
| v6 0 | keep the voice playing past a last/auto-click wait when 1 | MessageLayer `0x71000b1cb4` | hard-coded false (`message_layer/mod.rs:608`) | matches at default |
| v7, v8, v12, v16 | unknown (v8 read by backlog `0x71000a493c`) | - | - | open |
| v9 50 | text speed (layouter default speed; 100 for box type 5) | `0x71000b04a0` | **hard-coded 80** (`message_layer/mod.rs:413`) | diverges: every line types faster than the Switch default |
| v10 50 | auto-advance: delay x (100 - v10)/100 | `0x71000b1ff0` | **hard-coded 80** (`message_layer/mod.rs:789`) | diverges: auto waits 2.5x shorter than default |
| v11 1 | skip only read lines (also JumpPrev/Next text) | `isSkipping` `0x7100012430`, MSGSET `0x71000052b4` | ignored; Ctrl skips everything | diverges |
| v13 85 | message window opacity | `0x71000b47fc` | hard-coded 0.85 (`messagebox.rs:72`) | matches at default |
| v14 1 | scene-title banner (AUTOSAVE) | `0x7100064480` | banner missing | missing |
| v15 1 | song-title banner (BGMPLAY) | `0x710009e590` | banner missing | missing |
| v17 1 | touch effect | FirstBoot -> `0x7100027300` | missing | missing (low on PC) |
| v18 0 | unknown; ADV and BGMMode call `0x71000fc3b0` (sets a float at `0x7100242010` to 55.0) | ADV `0x71000113b0` | - | open |
| v19 all | voice filter bit mask per character | `0x71000eadc0` | missing | stub |

---

## 2. Top divergences (ordered by what the player hears and sees)

1. **No voices** (39,649 voiced lines). Implement `audio/voice_player.rs` after `0x710011a000/ab70/a6b0/a980`: a
   `Vec<Voice { handle, state: Loading|Playing|Stopping, fade: f32 }>`; start all loading voices together; gain =
   `v2/100 * fade`; stop = state Stopping, `fade -= 0.2 * dt_frames`, drop at 0; status 3/2/0 as `0x710011a980`.
   Wire VOICEPLAY (`0x7100007420`: skipped in skip mode, volume x 0.001, flags) and the MessageLayer voice blocks
   (already call `VoicePlayer::play`). Apply v19 (`0x71000eadc0`: play if any of the line's characters has its bit
   set in v19) and v5 panning; lip-sync from amplitude after that. Size M-L (file naming of `voice/*` and segment
   start/duration need a check against the assets).
2. **Settings are never applied** (text speed 80 instead of v9, auto delay 80 instead of v10, volumes ignored, v11
   ignored). Pass the Global settings to the message layer and audio players (Structural 2), then: draw speed =
   `if box type 5 {100} else {v9}`; auto delay `* (100 - v10) / 100`; BGM gain `v0/100`, SE `v1/100`, sys SE
   `v3/100`, movies by volume source (1 v0, 2 v1, 3 v2); voice only if `v2 != 0`. Size S each.
3. **No menus and no game flow** (title, save/load, config, backlog, system menu, scenario select). Build order in
   section 3. Size L overall.
4. **Looping SEs restart when re-issued, and skip plays every SE.** In `SePlayer::play` keep the channel when
   `slot` already plays the same `se_id` and both are looping: just `set_volume/set_panning` over `fade`
   (`0x71000f2580`); otherwise stop the old one with `max(fade, 4)` frames. In SEPLAY/SEONCE/SESTOPALL/BGMPLAY/
   BGMSTOP/BGMVOL/SESTOP/VOICEPLAY use an `is_skipping` that follows `0x7100012430` (skip toggle, not in an event,
   v11 read test) instead of "Ctrl held": one-shot SEPLAY stops the slot and plays nothing, fades become 0. Size S.
5. **Read lines and unlocks are not recorded.** Insert `msg_id` into `seen_messages` when the message layer reports
   a line finished (`0x7100014f50`; mind the id - 1 convention, engine-internals section 10); insert CG/BGM/movie ids
   on layer instantiation, BGMPLAY of a new track and UNLOCK (spec_save 6.3). Needed by v11 skip, the read colour,
   the galleries and the title. Size S.
6. **ADV controls**: map keyboard and gamepad to the engine bit mask (confirm/cancel 28/29, X/Y, L/R, ZL/ZR, "any
   direction" 24-27 with the existing repeat), then port `0x7100011f08`: L auto toggle (bit 0 of a mode word), R skip
   toggle (bit 1), cancel/X/Y stops both, Up = rollback to the previous checkpoint (`0x71000137f0`, the load
   machinery shin already has), X backlog, Y system menu, B hide window, ZL/ZR JumpPrev/Next, cancel in an event =
   event skip (`0x710000f600`), Down = advance. Auto mode itself: the auto-click logic exists in the message layer;
   it needs the mode bit. Size M.
7. **BGM semantics**: same-id BGMPLAY = volume retarget only; crossfade old tracks over `max(fade, 4)` frames;
   voice-focus duck 0.5 at 0.04/frame for BGM and looping SE when v4 is set and no task is open
   (`0x7100010fb0`, `0x7100033eb0`, `0x71000f3210`). Size S.
8. **Movies**: loop when LAYERLOAD's repeat bit is set (8 uses), apply settings gain by volume source, alpha
   transparency modes (check the movie table for how many use them). Size M.
9. **Overlays the script triggers**: the notification pop-up (ADV queue at +0x900; plays "notify1"/"notify2" by bit 4
   of the queued byte; tweens +0x930: hold 30 frames, flag 0x1000, 180 frames; +0x988: 0 -> 1 over 30, hold, 120,
   -> 0 over 60; fast-forwarded in skip) used by NOTIFYSET (524), CHARS (511), TIPSGET (21); the song-title banner
   (BGMPLAY, ADV+0x2e0, v15) and the scene-title banner (AUTOSAVE, ADV+0x1f0, v14). Size M.
10. **Boot sequence**: FirstBoot logos and LoadGame instead of starting straight in Episode 1 (`app.rs` builds a new
    game or `--load`). Size S once the scene framework exists.

---

## 3. Structural

1. **Scene + TaskPool framework, in the original's shape.** The original has exactly one current `Scene` and a
   pending factory (`std::function<Scene*()>`, set by `0x7100027370`, consumed at the top of the next frame by the
   main loop `0x7100026d50`); each scene owns a `TaskPool` of UI tasks (menus) that get input first and whose
   emptiness gates the ADV's input and its voice-focus update. Proposal: in `app.rs`,
   `trait Scene { fn update(&mut self, ctx, input) ; fn render(&self, pass) }`, `scene: Box<dyn Scene>`,
   `next_scene: Option<Box<dyn FnOnce(&mut SceneCtx) -> Box<dyn Scene>>>`; `Adv` becomes the `ADV` scene;
   `struct TaskPool(Vec<Box<dyn Task>>)` with `trait Task { fn update(&mut self, ctx, has_focus) -> bool /*done*/;
   fn render(&self, pass) }`. Scenes: FirstBoot, Title, TitleDemo, ScenarioSelect, ScenarioSelectBridge, ADV,
   BGMMode, BustupMode, CGMode, MovieMode. Tasks: SaveMenu, ConfigMenu, MessageBrowser, TipsMode, CharsMode,
   ManualViewer, Title::SubMenu, ADV::SystemMenu, ADV::HiddenWindow, the Viewers, SaveGame/LoadGame (with a job on
   `shin-tasks`). Without this, every menu would be bolted into `Adv::handle_input` as another early return (the
   select layer and QUIZ already are), which is how input-priority bugs start.
2. **One Global (SaveData) owned by the app, read by every subsystem.** The original reads settings straight from
   `Global` (`*0x71002264f0`) in the audio players, message layer, movies, backlog and menus. shin keeps the
   SaveData inside `VmState.global` (owned by `Adv`) and the subsystems hard-code values with "TODO: need a settings
   handle". Proposal: `App` owns `SaveData`; `UpdateContext` carries `&Settings` (or `Arc<RwLock<SaveData>>` if menus
   edit it while the ADV runs); writers (seen messages, unlocks, slots) go through the same handle. This removes the
   whole class "setting parsed but not applied" and lets the config menu take effect immediately.
3. **Audio players compute gain every frame on the main thread, like the original.** Today a sound's volume is baked
   when it starts and changed only by script tweens inside the kira thread. The original keeps per-track tweeners on
   the main thread and each frame pushes `setting * duck * tween` (`0x7100033eb0`, `0x71000f3210`,
   `0x710011ab70`) to the stream. Proposal: `BgmPlayer { tracks: Vec<Track { id, active, volume: Tweener, handle }>,
   duck: f32, duck_target: f32 }` and `SePlayer { channels: Vec<Channel { slot, id, looping, volume/pan/speed
   tweeners, handle }> }` with an `update(dt, &Settings)` that sets the handle's gain without a tween; keep kira only
   as the mixer. Volume changes from settings, ducking and crossfades then follow the original's numbers by
   construction.
4. **Input as the engine's bit mask.** Replace the ad hoc `AppAction` set with the original's 32-bit
   held/pressed/repeat masks (layout under "Controller" at the top), filled from keyboard, mouse and gilrs with a binding table, and
   let each scene/task test the same bits the original tests. Then ported input code reads like the decompile
   (`(pressed >> 28) & 1` = confirm), and the A/B swap is one flag.
5. **A side-effect-free VM replayer reused by LogBuilder and ThumbMaker.** The backlog, save thumbnails, rollback
   and JumpPrev/Next all replay the route on a copy of the GameData (spec_save 4.5). shin's load replay
   (`replay_command`) is the seed; make it a standalone `Replayer` over `(Scenario, GameData, PersistData)` with
   per-command hooks so MessageBrowser and SaveMenu can drive it without touching the live ADV.

### Suggested build order

1. Structural 2 (settings handle) + top divergences 2, 4, 5, 7 (all S): audible and visible on every line.
2. Voice player (1).
3. Structural 1 (scene/task framework) + Structural 4 (input mask) + ADV controls (6) + HiddenWindow.
4. ConfigMenu (needs 2), SystemMenu, SaveMenu without thumbnails (save/load to slots, SaveGame write moments).
5. FirstBoot -> Title -> ScenarioSelect -> Bridge -> ADV, and EXIT back to the menus.
6. MessageBrowser with LogBuilder (Structural 5), then rollback/JumpPrev/Next on the same replayer; SaveMenu
   thumbnails (ThumbMaker) on it too.
7. Overlays (notification pop-up, song/scene title banners), then TipsMode, CharsMode (need TIPSGET/CHARS state),
   galleries (CG, BGM, Movie, Bustup), ManualViewer, TitleDemo, TouchEffect.

---

## 4. Open questions

- v7, v8, v12, v16, v18: meanings (v18: what the 55.0 float at `0x7100242010` controls; probably an idle/sleep
  timer, **inferred**).
- SePlayer `+0` flag: when set the SE update `0x71000f3210` is skipped entirely (pause while a menu is open?).
- `VoicePlayer::play` `0x710011a000` was not decompiled: pan formula (v5), how segments map to files, the second
  argument (Context+0x2520, a voice mapping table?).
- `TaskPool::vfunc_2` returning true is read here as "empty"; the voice-focus update depends on it (**inferred**).
- Which SystemMenu / Title / Title::SubMenu item does what (8 `Decide2` lambdas, 3 + 3 `Decide` lambdas): needs the
  item tables or an Eden session.
- When `0x71000fc1f0` (confirm/cancel swap) is true (system language or region setting?).
- The movie table: how many movies use a transparency mode, and whether volume 0 movies are meant to be silent
  (758 loads; both engines treat them as silent).
- ADV+0x4f8/+0x500 (also cleared by SYSCALL 0 and the menu openers): something that fades out 60 s after auto/skip
  starts; still unknown.
