# Alignment audit: systems (menus, modes, audio/video, assets, main loop, input, settings)

Status checked against the code on 2026-11-07.

> The audit was written in November 2026, before the scenes, menus, voices and settings were in. The tables below
> now give the current state of each row; the reverse-engineering notes are kept as found. The port's measured
> accuracy and what was compared with the original: [alignment-plan.md](../alignment-plan.md).

Scope: everything outside the adventure screen's commands, layers and text. Original = Switch v1.0.3 `main`
(Ghidra base 0x7100000000). shin = `H:\Umineko PC Port\shin`. Logs and decompiles:
`$S\audit\systems\` (a1.c .. a8.c, d1.txt, lambdas.txt, py\xref.py and py\calls.py: ADRP+ADD and BL scanners over
the NSO, used where Ghidra had no references).

Evidence found in this audit:
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
  `AudioWaitStatus::FADING` (still so named) should be `LOADING`. Behaviour matches: shin starts sounds synchronously.
- **VoicePlayer** (`0x710011ab70` update, `0x710011a6b0` stop, `0x710011a980` status): a list of voices with state 0
  loading / 1 playing / 2 stopping. All loading voices start together once all are ready; stop moves them to state 2,
  a fade of 0.2 per frame (5 frames, `0x710015a954`); state 2 at 0 or a finished stream is removed.
- **Controller** `0x71000c4300` -> engine bit mask (held +0, pressed +4, repeat +8 / +0xc): bit 0-3 d-pad U/D/L/R,
  4 A, 5 B, 6 X, 7 Y, 8 +, 9 - (first read here as ZL/ZR; + and - per engine-internals §15 and the title demo's
  skip), 10 L, 11 R, 12/13 ZL/ZR (unverified), 14/15 stick clicks, 16-19 left stick, 20-23 right stick, 24-27 "any
  up/down/left/right" (d-pad or left stick, opposite directions cancel), **28 confirm, 29 cancel** (A/B, swapped
  when `0x71000fc1f0` is true). Repeat: first press, then every 4 frames from frame 24, every 2 from 64.
- **ADV buttons** `0x7100011f08` (confirmed): L toggles auto (ADV+0x47c bit 0; sounds "son"/off), R toggles skip
  (bit 1), cancel/X/Y while auto or skip is on turns both off; bit 8 (+) `0x7100012c80` (JumpNext, "Skip this
  chapter?", confirmation; setting v11 changes its text), bit 9 (-) `0x7100012f80` (JumpPrev); inside an event:
  confirm shows a hint overlay (+0x4a0: in 32, hold 120, out 32), cancel skips the event (`0x710000f600`); otherwise
  **Y opens the SystemMenu** (`0x7100013320`, task 0x298 bytes, ctor `0x710001c100`, sound "page"), **X opens the
  backlog** (`0x7100013400`, MessageBrowser built from the Context's GameData via `0x71000aaf90`; "error" if not
  possible), **B hides the window** (`0x7100013720`, HiddenWindow task 0x88, ctor `0x710001bd80`), **Up (repeat)
  rolls back to the previous save point** (`0x71000137f0`: checkpoint `0x710000f220`, loads it with mode 1,
  "decide"; "error" if none), confirm or Down (mask `0x12000000`, bits 25 and 28) advances the message
  (`0x71000b1b10`) and also ends an interruptible WAIT.
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
| Main loop `0x7100026d50` | `shin/src/app.rs` `App::update` | matches | Scene switch, one shared BGM player, the TouchEffect over every scene, and the play time counted every frame in every scene (`App::play_frames`) | none | - |
| `shin::Scene` (typeinfo `0x71001d3e20`) + factories `Create*` (lambda names, `lambdas.txt`); switch `0x7100027370` (31 call sites) | `shin/src/app.rs` `Scene`, `App::switch_scene` | matches | One scene at a time; a change crossfades from a snapshot of the old screen | none | - |
| `shin::Task`, `TaskPool` (vtable `0x7100223378`: add `0x7100100930`, isEmpty `vfunc_2`, update `vfunc_3`, render `vfunc_4`), `UITask`, `UpdateTask` | `shin/src/adv/menu/mod.rs` | matches | The top task takes the input; the ADV's buttons work only while the pool is empty | none | - |
| `AsyncQueue` / `AsyncQueue::Task`, `X::First` loader tasks | `shin-tasks`, `AssetServer::load` | matches (functionally) | Each `First` loads `/x.txa` asynchronously then builds the menu; shin's async asset loads do the same job | none | - |
| Pad input `0x71000c4300`, copy `0x71000c4900` | `shin-input` (gilrs + keyboard + mouse), `shin/src/bindings.rs`, `app.rs` `AppAction` | matches | Keyboard, mouse and gamepad go through a binding table (rebindable); repeat timings match. The confirm/cancel swap (`0x71000fc1f0`) is the Confirm button option (`bottom_confirms`), which also picks the event hint's row 1 | none | - |
| ADV button handling `0x7100011f08` (see above) | `shin/src/adv/mod.rs` | matches | Auto, skip, v11, rollback, backlog, system menu, hide window, +/- chapter jumps, event skip, and advance on confirm or down (mask `0x12000000`). Port extra: holding Ctrl skips under the same rules | none | - |
| `GestureRecognizer::Listener`, `ADV::OnTap` | `shin/src/ui/gesture.rs`, `shin/src/adv/mod.rs`, `shin/src/adv/touch_buttons.rs` | matches | Taps, drags and flicks with the mouse as one finger; the ADV's flicks and touch buttons are in | none | - |
| `TouchEffect` (vtable `0x7100224e58`, update `0x7100115d70`, v17) | `shin/src/ui/touch_effect.rs` | matches | Ripples and drag stars over every scene, gated by v17 | none | - |
| `SystemEventDelegate` | `shin-window` focus handling | n/a | Suspend/resume on Switch | none | - |
| `Trophy::Installer`, TROPHY | `shin/src/adv/command/trophy.rs` | matches | The Switch build does nothing either (`0x71001171f0`); a no-op | none | - |
| `ScenarioSelectBridge` + `CreateScenarioSelectBridge(int)` | `shin/src/scenario_select.rs` `Bridge` | matches | The door-opening transition that starts an ADV for a scenario id | none | - |

### 1b. Menus and modes (60 RTTI classes; grouped with their `First`/`Viewer` helpers)

Common shape (confirmed from vtables and lambda names): `X::First` (5-slot Task) loads `/x.txa` (load site below) and
other assets, then replaces itself with `X`; `X` is a 13-slot Task (overlay inside the current scene) or a 12-slot
Scene; lambda names give its methods (`Start` = open animation, `Close`/`Exit` = close animation, `Decide`, `Main`).
Sounds go through `SysSePlayer::play` with names from pointers: "decide" `*0x7100226740`, "move" `*0x71002267e0`,
"error" `*0x7100226748`, "cancel" `*0x71002267d8`, "page" `*0x7100226750`, "son" `*0x7100226738`, "notify1/2"
`*0x7100226718/20`. Each row ends with its shin counterpart and status. Impact and Fix give what is left.

| Original (address) | What it is / does | Entered from | Depends on | Impact | Fix |
|---|---|---|---|---|---|
| `FirstBoot` (Scene; vtable `0x71001d90b0`; update `0x7100065300`, render `0x7100065720`; `CreateFirstBoot`) | Boot: system assets (`/systex.txa` load `0x7100065d24`, `/system.fnt` `0x7100065ae8`), logo sequence from a table at `*0x7100226cb8` (0x18-byte entries, frame count at +8; 60-frame fades, `0x710015a778`; dead code in v1.0.3, engine-internals §15), then the `LoadGame` task (`0x71000d5290`); its callback `0x71000664d0` sets the play-time clock; then `CreateTitle` (`0x7100066530`). **shin:** `shin/src/app.rs` `App::new` (loads the save, then TitleDemo); matches | process start | LoadGame | none | - |
| `LoadGame` (Task `0x7100220860`; read job `0x71000d4890`; `SubErrorAlert`, `InheritAlert` lambdas) | Reads SAVEDATA; corrupt / newer-version dialogs. **shin:** `shin/src/savedata.rs`; matches: a corrupt save (case 1, `0x7100161ace`) or a newer version's (case 2, `0x710015c930`) is moved aside and the title shows the original's dialog; an unreadable file (`0x710015f8dc`) shows it too and nothing is saved that session. The English patch leaves these texts in Japanese, so English players get the port's wording; the original shows them before the opening movie, the port on the title | FirstBoot | dialogs | low | S |
| `SaveGame` (Task `0x7100220828`; job `0x71000d3ce0`; create `0x71000d52f0`) | Writes SAVEDATA (>= 1 s, with an indicator, `SaveGame::Update` lambdas). **shin:** `shin/src/savedata.rs`, `shin/src/adv/menu/save_notice.rs`; matches | EXIT `0x71000147e0`, title return, config close if changed `0x71000609b0`, manual save `0x71000daafc`, delete `0x71000db6c0` | - | none | - |
| `Title` (no RTTI for the class itself; `Title::First` `0x7100223fe0` vfunc_3 `0x71001112d0`; `CreateTitle(int)`; methods `StartMenu`, `MenuItemOut`, `Start`, `Decide` (3 lambdas), `Continue`, `DebugMenu`) | Title screen: `/title.txa` (load `0x7100113be4`), background `/title/bg%02d.pic` (`0x7100113df0`, number chosen at runtime); fade-in 60 frames, or 90 frames with property 66 (blur) tweened to 0 when `+0x22c < 0`. Continue = new ADV from `Global.auto` (`0x7100112670`). DebugMenu writes persist slots ("unlock all", `0x7100112890..`). Scene switches from title decisions at `0x710010e368..0x710010e588`. **shin:** `shin/src/title.rs`; matches (DebugMenu not ported; unverified whether retail can reach it) | FirstBoot, EXIT, system menu "return to title", TitleDemo exit | SaveMenu, ConfigMenu, ScenarioSelect, extras, ManualViewer | none | - |
| `Title::SubMenu` (Task `0x7100223f08`; `MenuItemIn/Out`, `Decide` x3) | The episode list of Characters and Tips on the title (`0x710010f0a0`). **shin:** `shin/src/title.rs` `SubMenu`; matches | Title | CharsMode, TipsMode | none | - |
| `TitleDemo` (Scene `0x7100224a80`; `CreateTitleDemo`; `Start`, `Exit`; 2 `First` lambdas) | The opening movie (movie table entry 0) before the title; confirm, + or a tap skips it. **shin:** `shin/src/title_demo.rs`; matches (whether the title also returns to it after idle is unverified) | boot | movie playback | none | - |
| `ScenarioSelect` (Scene `0x71002215a8`, update `0x71000de100`; `CreateScenarioSelect`; `Main`, `Decide`, `Close`) | Saku's story-piece selector: `/snrsel.txa` (`0x71000e35b0`); entries' states are persist slots `scenario_id + 1` (spec_save 6.1). **shin:** `shin/src/scenario_select.rs`; matches | Title (new game), EXIT | persist data, ScenarioSelectBridge | none | - |
| `ADV::SystemMenu` (Task `0x71001d45a0`, ctor `0x710001c100`, update `0x710001d000`; `Decide` + 8 `Decide2` lambdas) | In-game menu: vertical list (0xb8-byte items, up/down with wrap, "move"; confirm `0x710001e010`; cancel/Y "cancel"); shows play time `%d時間 %d分` (`0x710015b467`, capped 999:59) at x 756, size 38. Items: save, load, config, characters, tips, manual, title (`0x710001ff98` -> SaveGame). **shin:** `shin/src/adv/menu/system_menu.rs`; matches | ADV Y | SaveMenu, ConfigMenu, SaveGame | none | - |
| `ADV::HiddenWindow` (Task `0x71001d44c8`, ctor `0x710001bd80`) | Hides the message window until a press. **shin:** `shin/src/adv/menu/hidden_window.rs`; matches | ADV B | - | none | - |
| `SaveMenu` + `First` (Task `0x7100220c28`; ctor `0x71000d5b40`; `Save`, `Load`, `Delete`, `Decide`, `Start`, `Close`, `Exit`) | 100 manual slots + auto, `/saveload.txa` (`0x71000dc290`); cursor = `save_menu_position`, scroll `clamp(pos*186 - 279, 0, 17856)`; save `0x71000da920`, load `0x71000db120` -> new ADV; thumbnails are **rendered by replaying each slot** (`ThumbMaker::StateBuilder`, spec_save 4.5). **shin:** `shin/src/adv/menu/save_menu.rs`, `shin/src/adv/menu/thumbnail.rs`; matches (thumbnails leave out movie layers and render at window size before halving) | system menu, title | ThumbMaker (replay + off-screen render), SaveGame, ADV restart from GameData | low | M |
| `ConfigMenu` + `First` + `MenuItem` / `ButtonMenuItem` (0x80) / `SliderMenuItem` (0xd0) (Task `0x71001d7dc0`; `InitMenu` `0x7100058e90` with 14 item lambdas; `ResetDefault`; `Close`/`_Close`) | Edits Global settings (ids in spec_save section 7); `/config.txa` (`0x7100060d90`); on close compares with a copy (`0x71000d13c0`) and writes the save if changed. **shin:** `shin/src/adv/menu/config_menu.rs` (plus a port-only PC tab, `shin/src/port_settings.rs`); matches | system menu, title | settings read each frame | none | - |
| `MessageBrowser` + `First` + `ChapterSelector` + `LogBuilder` + `Layouter` (Task `0x71001dc010`; `Main`, `QuickLoad`, `OnTap`) | Backlog: `/logview.txa` (`0x71000ad9b0`); LogBuilder (ctor `0x71000a3280`, a `Scripter::Listener`) **re-runs the VM over a copy of the GameData and persist data** to rebuild past lines (SGET `0x71000ab130`, SELECT `0x71000ab4a0`, QUIZ `0x71000abed0`); voices can be replayed (v4 read `0x71000a3f84`, v8 read `0x71000a493c`); QuickLoad jumps to a line by starting a new ADV from a GameData it builds (`0x71000ad068`). **shin:** `shin/src/adv/menu/backlog.rs`, `shin/src/adv/menu/chapter_list.rs`, LogBuilder in `shin/src/adv/mod.rs`; matches (v8 is not read; its meaning is open) | ADV X | VM replay (`replay_command`), message layouter, voice player, ADV restart | none | - |
| `ManualViewer` + `First` (Task `0x71001da678`) | Paged manual images `/manual.txa` (`0x7100087850`). **shin:** `shin/src/adv/menu/manual.rs`; matches | title, system menu | - | none | - |
| `TipsMode` + `First` (Task `0x71002239d0`) | Tips list `/tips.txa` (`0x710010af64`); entries unlocked by TIPSGET. **shin:** `shin/src/adv/menu/tips.rs`; matches | title (per episode), system menu (collected tips) | TIPSGET state | none | - |
| `CharsMode` + `First` (Task `0x71001d76b0`; `ChangePage`, `ChangeChar`, `ChangeStatus`, `ChangeDress`) | Character profiles `/chars.txa` (`0x7100056230`); state from CHARS/SHOWCHARS. **shin:** `shin/src/adv/menu/chars.rs`; matches | title (per episode), system menu | CHARS state | none | - |
| `CGMode` + `First` + `Viewer` (Scene `0x71001d6ee8`; `CreateCGMode`) | CG gallery `/cgmode.txa` (`0x710004d2a0`). **shin:** `shin/src/picture_box.rs`; matches (the viewer pans with the d-pad and pointer, not the left stick) | title extras | `unlocked_cgs` (`shin/src/adv/unlocks.rs`) | low | S |
| `BGMMode` + `First` (Scene `0x71001d5df0`; `CreateBGMMode`; `Close`) | Music room `/bgmmode.txa` (`0x7100033180`); calls `0x71000fc3b0` (v18 related; read by the port as the console's keep-awake request, n/a on PC). **shin:** `shin/src/music_box.rs`; matches | title extras | `unlocked_bgms`, BGM player | none | - |
| `MovieMode` + `First` + `Viewer` (Scene `0x71001daa88`; `CreateMovieMode`) | Movie gallery `/moviemode.txa` (`0x710008b340`). **shin:** `shin/src/movie_box.rs`; matches | title extras | `unlocked_movies`, movie playback | none | - |
| `BustupMode` + `First` + `Viewer` (Scene `0x71001d63d0`; `ChangeBustup`, `ChangeBg`, `Hidden`) | Sprite viewer `/bupmode.txa` (`0x710003f164`). **shin:** `shin/src/character_box.rs`; matches (loads synchronously; the viewer does not pan with the left stick) | title extras | bustup layer | low | S |
| `CharsMode` typeinfo copies (`0x710005580c` ...) | inventory artefacts (string matches inside code), not classes | - | - | none | - |

### 1c. Audio and video (13 RTTI classes + the RTTI-less SePlayer)

| Original (address) | shin counterpart | Status | What differs / evidence | Impact | Fix |
|---|---|---|---|---|---|
| `VoicePlayer` (vtable `0x71002251b8`; play `0x710011a000`, update `0x710011ab70`, stop `0x710011a6b0`, status `0x710011a980`) | `shin/src/audio/voice_player.rs`, `shin/src/adv/command/voiceplay.rs` | matches | Grouped start, v2 gain, 5-frame stop fade, v5 pan to the speaker's bustup, v19 per-character switches (`0x71000eadc0`), lip-sync from a level table. Not done: `@x` voice segments (unused by Umineko) | none | - |
| `BGMPlayer` (vtable `0x71001d61d8`; play `0x71000337f0`, update `0x7100033eb0`, status `0x7100033e00`, position `0x7100033e70`) | `shin/src/audio/bgm_player.rs`, `shin/src/adv/command/bgmplay.rs` | matches | v0 gain, voice-focus duck, same-id retarget, crossfade over max(fade, 4) frames, unlock, song-title banner. The duck target also moves while a menu is open (original: only with an empty task pool); unverified whether audible | none | - |
| SePlayer (no RTTI; ADV+0xc0; play `0x71000f2580`, update `0x71000f3210`, stop `0x71000f2b30`, stopAll `0x71000f2d20`, status `0x71000f2fd0`) | `shin/src/audio/se_player.rs`, `seplay.rs`, `seonce.rs`, `sestop.rs`, `sestopall.rs`, `sevol.rs` | matches | v1 gain, duck of looping channels, looping re-issue keeps playing, 4-frame stop floor, skip-mode one-shots stop the slot, SEONCE follows `isSkipping`. Play speed is ignored (every SEPLAY in main.snr uses 1000); SEONCE's automatic channels stop without the 4-frame floor (unverified whether the original's stopAll applies it). The pan formula in `shin-audio/src/sound.rs` is fixed | none | - |
| `SystemSound` (vtable `0x7100223318`; play `0x71000ff620` via `SysSePlayer::play` `0x71000272b0`) | `shin/src/audio/sys_se.rs` | matches | v3 applied; the menus play their sounds through it | none | - |
| `StreamSound` (vtable `0x7100222f80`; set gain `0x71000f5390`) | `shin-audio` `AudioSound` (kira) | matches | Linear gain; loop from the file's loop point; bit 1 "not started" never set because shin starts synchronously (equivalent) | none | - |
| `Audio::Delegate`, `Audio::Voice`, `Audio::Voice::Delegate`, `Audio::VoiceImpl` (slot 8 `0x710002bb40` = set gain) | `shin-audio` + kira/cpal | matches | Linear gain clamp >= 0 matches `Volume` (0..1000 -> 0..1 linear) | none | - |
| `AudioDecoder` / `AudioDecoderImpl` | `shin-core/src/format/audio` | matches | Plays every BGM/SE in use | none | - |
| `Movie`, `MovieImpl` (vtable `0x71001dd4c0..`), `MovieImpl::StreamReader`, `movie::ClientStreamReader`, `MovieLayer::Shared` (`0x71000c29c0`) | `shin-video`, `shin/src/layer/user/movie_layer.rs` | matches | Settings gain by volume source, repeat and transparency are in. Open: the movie layer's fast-forward does nothing and AAC seek is a `todo!` (`shin-video/src/audio.rs`); whether either is ever reached is unverified. 758 movie loads pass volume 0 (silent in both) and 69 pass 1000 | low | S |

### 1d. Data, files and assets (20 RTTI classes)

| Original | shin counterpart | Status | Evidence | Impact | Fix |
|---|---|---|---|---|---|
| `SourceMedia`, `MountableSourceMedia(Impl)`, `DataSource`, `FileDataSourceImpl`, `File::Handle(Impl)` | `asset/system/locate.rs` (`LayeredAssetIo`), `shin-core/src/format/rom.rs` | matches | patch dir, patch.rom, data dir, data.rom in that order; engine-internals section 3 (patch.rom first) and section 4 (ROM2 confirmed) | none | - |
| `CachedAssets::CacheDataSource(Impl)`, `RangedCacheDataSourceImpl`, `Data`, `DataImpl`, `MutableData(Impl)`, `RangedDataImpl` | `asset/system/cache.rs`, `server/accessor.rs` | matches (different shape) | Caching and ranged reads; shin's accessor reads synchronously (TODO async) | none | - |
| `TXA4` | `asset/texture_archive.rs` | matches | used by the minigames, messagebox and menus | none | - |
| `Font` | `asset/font.rs` | matches | text area audit | none | - |
| `MaskTexture` | `asset/mask.rs` | matches | MASKLOAD works | none | - |
| `RenderTexture`, `DepthStencil` | `shin-render` | matches | | none | - |
| PIC4, bustup, sysse.bin, scenario, NXA formats | `shin-core/src/format/*` | matches | engine-internals sections 4, 5 confirmed | none | - |
| Global SaveData (`0x7100229218`), file format | `shin-core/src/format/save`, `shin/src/savedata.rs` | matches | Format, obfuscation, defaults `0x71000d2110` match (spec_save 6.5, 7). Seen messages are written when a line finishes (`0x7100014f50`, `shin/src/adv/mod.rs`); CG/BGM/movie unlocks (+0x68/+0x80/+0x98; `0x7100020470`, `0x71000337f0`, UNLOCK) in `shin/src/adv/unlocks.rs` | none | - |
| Settings (Global+0xb0) | `Settings` in `format/save/mod.rs` | matches | Read from the save data each frame by the players, message layer and HUD (`AdvState::apply_settings`); see 1e | none | - |

### 1e. Settings: what each one does and whether shin applies it

| # | Meaning (spec_save 7 + this audit) | Original reader | shin | Status / visible effect |
|---|---|---|---|---|
| v0 70 | BGM volume | `0x7100033eb0`, movies | `audio/bgm_player.rs`, `audio/sys_se.rs` (movies) | matches |
| v1 70 | SE volume | `0x71000f3210`, movies | `audio/se_player.rs`, `audio/sys_se.rs` (movies) | matches |
| v2 90 | voice volume; 0 = voices not started | `0x710011ab70`, MessageLayer `0x71000b1eb8`/`0x71000b2290` | `audio/voice_player.rs`, `layer/message_layer/mod.rs` | matches |
| v3 55 | system sound volume | `0x71000ff620` | `audio/sys_se.rs` | matches |
| v4 0 | voice focus (BGM and looping SE x0.5 while a voice plays, 0.04/frame) | ADV `0x7100011670` | `adv/mod.rs` `apply_settings` | matches |
| v5 1 | voice pans with the speaker | VoicePlayer | `audio/voice_player.rs` | matches |
| v6 0 | keep the voice playing past a last/auto-click wait when 1 | MessageLayer `0x71000b1cb4` | `layer/message_layer/mod.rs` | matches |
| v7, v8, v12, v16 | unknown (v8 read by backlog `0x71000a493c`) | - | not read | open |
| v9 50 | text speed (layouter default speed; 100 for box type 5) | `0x71000b04a0` | `layer/message_layer/mod.rs` | matches |
| v10 50 | auto-advance: delay x (100 - v10)/100 | `0x71000b1ff0` | `layer/message_layer/mod.rs` | matches |
| v11 1 | skip only read lines (also JumpPrev/Next text) | `isSkipping` `0x7100012430`, MSGSET `0x71000052b4` | `adv/mod.rs` | matches; holding Ctrl (port extra) skips under the same rule |
| v13 85 | message window opacity | `0x71000b47fc` | `layer/message_layer/messagebox.rs` | matches |
| v14 1 | scene-title banner (AUTOSAVE) | `0x7100064480` | `adv/hud.rs` | matches |
| v15 1 | song-title banner (BGMPLAY) | `0x710009e590` | `adv/hud.rs` | matches |
| v17 1 | touch effect | FirstBoot -> `0x7100027300` | `ui/touch_effect.rs` | matches |
| v18 0 | unknown; ADV and BGMMode call `0x71000fc3b0` (sets a float at `0x7100242010` to 55.0) | ADV `0x71000113b0` | config item only | n/a on PC if it is the keep-awake request (port's reading, inferred) |
| v19 all | voice filter bit mask per character | `0x71000eadc0` | `audio/voice_player.rs` | matches |

---

## 2. What still differs

1. **LoadGame's dialogs** show on the title rather than before the opening movie.
2. Small shortcuts noted in the module docs: thumbnails without movie layers, galleries without left-stick panning,
   synchronous loads in the Character Box, movie fast-forward and AAC seek not done.

---

## 3. Structural

The five structural proposals of the first audit are in place, in this shape:

1. **Scene + TaskPool**: `App` holds one `Scene` and switches with a crossfade (`shin/src/app.rs`); the ADV and the
   title run a task pool (`shin/src/adv/menu/mod.rs`), the top task taking the input.
2. **Settings**: the subsystems read the settings from the save data each frame (`AdvState::apply_settings`); the
   other scenes keep their own copy of the save data and pass it on.
3. **Audio gain**: each frame the players set `setting * duck` on their kira track; script tweens stay on the
   sounds inside kira. The result follows the original's numbers.
4. **Input**: not the engine's bit mask but `AppAction` in the Switch's button roles, filled from keyboard, mouse and
   gamepad through a binding table (`shin/src/bindings.rs`).
5. **Replayer**: `replay_command` drives the load, the backlog's LogBuilder, the thumbnails, rollback and the chapter
   jumps.

---

## 4. Open questions

- v7, v8, v12, v16, v18: meanings (v18: the port reads the 55.0 float at `0x7100242010` as a keep-awake/idle timer,
  **inferred**).
- SePlayer `+0` flag: when set the SE update `0x71000f3210` is skipped entirely (pause while a menu is open?).
- `TaskPool::vfunc_2` returning true is read here as "empty"; the voice-focus update depends on it (**inferred**).
- When `0x71000fc1f0` (confirm/cancel swap) is true (system language or region setting?).
- Whether volume 0 movies are meant to be silent (758 loads; both engines treat them as silent).
