# Audit: script commands, the VM and the adventure screen's command layer

Status checked against the code on 2026-11-07.

> The audit was written in November 2026, before skip and auto, read flags, rollback, the chapter jumps, event skip,
> the banners, the notifications and the menus were ported. The table and lists below now give the current state;
> the notes on the original are kept (Minus's dialog has since been identified). Project-wide status and measured accuracy:
> [alignment-plan.md](../alignment-plan.md).

Area: every `shin::ADV::*` class (class-inventory "Script commands"), the VM run loop, and how `shin::ADV` drives
commands, input, skip/auto and read flags. shin side: `shin/src/adv/{mod.rs,hud.rs,touch_buttons.rs,command/*.rs,
vm_state/*.rs,menu/*.rs}`, `shin-core/src/vm`, `shin/src/app.rs`. Logs and decompiles: `$S\audit\commands\` (d1.c = all
59 listeners + slots 1-4 of every command; d2-d5 = ADV update/render/input, Context helpers, HID; adv_dis.txt =
disassembly 0x7100010000-0x7100039f2c). Save format not re-audited (see `$S\mur\spec_save.md`, cited as *spec_save*).

## How the original is built (confirmed, d1.c / d2.c)

- **Listener = constructor = model write.** `shin::ADV::Context` (the Scripter listener) has one slot per opcode in
  the `shin::ADV` vtable: slot `4 + i` for the i-th command of shin's `Command` enum order (EXIT 4 ... DEBUGOUT 62;
  checked against the known MSGSET 9 / SELECT 14). Each handler destroys the current command (`+0x2590`, dtor
  `+0x2598`), constructs the new one in place at `+0x25a0` (fields: `+8` the ADV task, `+0x10` the Context, args from
  `+0x18`) **and writes the scene model in the Context** (e.g. BGMPLAY `+0x8edc..`, MSGINIT `+0x90ec`, PLANESELECT
  `+0x3dcc`). This is shin's `apply_state`.
- **Command vtable**: slot 1 dtor, slot 2 replay (spec_save 4.3), **slot 3 "skippable"** (only used by the look-ahead
  jumps `0x710000f4f0` / `0x710000f600`, see below), slot 4 execute (called every frame until it returns 1).
- **ADV update** `0x7100010fb0`: while no modal task: `Context::vfunc_2` (`0x710000ee60` → VM `0x71000ef450`) runs to
  the next command, then its execute; loop while execute returns 1. Then: skip-mode fast-forward of layers/voice/SE/
  message box, the wait timer (`+0x494 -= dt`), the HUD tweeners, the notification queue, voice focus (BGM/SE × 0.5
  while a voice plays if setting v4). Render `0x71000125a0`: layers, scene-title banner (`+0x1f0`), song-title banner
  (`+0x2e0`), skip/auto icons (`+0x47c` bits 1/0, blinking), notification banner (`+0x928`), touch HUD (`+0x500`),
  event-skip hint (`+0x4a0`).
- **`isSkipping`** `0x7100012430` = no modal (`ADV+0x470 == 0`) && system enabled (`Context+0x913c`) && not in an
  event (`Context+0x2ddc < 0`) && skip toggle on (`ADV+0x47c` bit 1) && the top interactive layer allows it &&
  (setting v11 `+0xbb` off || current line read `Context+0x2dd9`). **`isAuto`** `0x71000122f0`: same gates with bit 0,
  no read-flag test.
- **Input** (`ADV` keys handler `0x7100011ea0`; buttons from `0x71000c4300`, engine bits: 4 A, 5 B, 6 X, 7 Y,
  0x100 Plus, 0x200 Minus, 0x400 L, 0x800 R, 0x1000 ZL, 0x2000 ZR, bit 28 "decide" = A (B on a swapped layout,
  `0x71000fc1f0`), bit 29 "cancel"; word `+0` held, `+4` pressed, `+8` repeat, `+0xc` released):
  - L released → toggle auto (sound `son` on / `cancel` off); R released → toggle skip (same sounds). Gated by
    the HUD-button-visible flags `+0x55c` / `+0x614`, which ADV update sets to "system enabled" (`0x7100011de0`).
  - While skip/auto is on, X, Y or cancel turn both off (`cancel` sound) and are consumed.
  - System enabled and **in an event**: decide shows the event-skip hint (`+0x4a0`: in 32 frames, hold 120, out 32);
    cancel **skips the event** (`0x710000f600`: look-ahead to EVEND, then rebuild there; sound `decide`).
  - System enabled, not in an event: Plus → "Skip this chapter?" (`ADV::JumpNext` `0x7100012c80` → `0x710000f4f0`:
    look-ahead over skippable commands to the next stop, text depends on v11; `error` sound if nothing to jump to);
    Minus → "Return to the last chapter?" (`ADV::JumpPrev` `0x7100012f80`, target `0x71000182c0`); Y → system menu
    (`0x7100013320`, `page`); X → backlog (`0x7100013400`, `page` / `error`); cancel → hide window (`0x7100013720`,
    `shin::ADV::HiddenWindow`).
  - Then the interactive layer (choice menu etc., vfunc +0x88), then: repeat-up → **roll back one line**
    (`0x71000137f0`: rebuild at the previous line's save point `+0x2db4`/`+0x2db8`, needs 2+ lines since start;
    `decide` / `error` sound); decide (or bit 25) → advance the message (`0x71000b1b10`).
  - Only if nothing consumed the press: decide/bit 25 ends an **interruptible WAIT** (`+0x490 == 1` → `+0x494 = 0`).
  - Touch (`0x71000157d0`, `0x7100015c00`, `0x7100015f40`): tap = advance / end interruptible wait / event hint;
    swipes open/close the touch HUD and hide the window.

## 1. Table

Status: M matches, D diverges, S stub, X missing, T structural. Impact: H/M/L/none. Fix: S < 1 h, M < 1 day, L more.

| Original (address) | shin | St | What differs / evidence | Impact | Fix |
|---|---|---|---|---|---|
| VM run loop `0x71000ef450`, `rnd` `0x71000ed3c0` | `shin-core/src/vm/mod.rs` | M | PRNG steps once per instruction before it runs, dispatch until a command; rnd formula matches. All four unary ops (zero, not, negate, abs) are in; only negate is used (engine-internals §10) | none | – |
| ADV task update `0x7100010fb0` (command loop) | `Adv::update` | M | Same loop shape. WAIT checks its timer before counting it down, as the original counts after the command loop (`wait.rs`) | none | - |
| ADV input `0x7100011ea0` | `Adv::handle_input`, `Adv::handle_mode_buttons` | M | L/R toggles with sounds, X/Y/cancel stop the modes, event hint and event skip (B and long press), Plus/Minus jumps, Y system menu, X backlog, B hide window, Up rollback, the touch gestures, and confirm or down advancing; an interruptible WAIT ends only on a press the text did not take (`0x7100012194`). Held Ctrl skips as a PC extra | none | - |
| `isSkipping` `0x7100012430` | `Adv::is_fast_forwarding` | M | Skip bit (or held Ctrl), system enabled, no event, no task/choice/quiz, v11 read test (`modes_allowed`) | none | – |
| `isAuto` `0x71000122f0`, auto toggle (L) | `Adv::is_auto` → `MessageLayer::request_autoplay` | M | Same gates without the read test; auto does not advance event lines (`IGNORE_INPUT`) | none | – |
| Read flags: MSGSET ctor `+0x2dd9` (`0x71000d19f0` on seen set), writer `0x7100014f50`/`0x7100014fa0` (spec_save 6.2) | `msgset.rs` (`line_read`), `Adv::update` (`take_finished`) | M | MSGSET computes the read flag (script id − 1); a finished line is added to `seen_messages` | none | – |
| skip/auto icons, touch HUD `+0x500`, event hint `+0x4a0` (render `0x71000125a0`) | `hud.rs`, `touch_buttons.rs` | M | Icons, touch buttons and hint drawn in the original's order. The hint takes row 1 when confirm and cancel are swapped (`0x71000fc1f0`; the port's Confirm button option), else row 0 | none | – |
| Notification queue `0x71000148f0`, banner in ADV update/render | `AdvHud::queue_notification` | M | One pending per kind, `notify1`/`notify2`, tweens as in the NOTIFYSET row | none | – |
| `ADV::Context` (`+0x2530..+0x9170`) | `VmState` (+`SaveState`, `LayersState`, `AudioState`, `MessageState`, `SaveInfo`) | T | System enabled, CHARS, tips, notify level, line read, line and chapter save points and line counts are now model fields written in `apply_state`. The SAVEINFO pending/committed pairs `+0x90f8..+0x9118` are in `SaveInfo` | L | S |
| `ADV::First` (`0x7100019930`) | `Adv::new` | M | New game or load start | none | – |
| `ADV::SceneLoader` (`0x710001b340`, `0x7100019ea0`) | `Adv::new`, `replay`, `restore.rs` | M | The same replay serves loads, rollback, the chapter jumps, event skip, the backlog and save thumbnails | none | – |
| `ADV::HiddenWindow` (ctor `0x710001bd80`) | `menu/hidden_window.rs` | M | B / downward flick; nothing paused | none | – |
| `ADV::SystemMenu` (ctor `0x710001c100`) | `menu/system_menu.rs` | M | Compared with Eden (alignment-plan row 15) | none | – |
| `ADV::Command` slot 3 "skippable" | `look_ahead` in `adv/mod.rs` | M | Not a per-command slot: `look_ahead` decides the stops (AUTOSAVE, SELECT/QUIZ, unread line with v11, EVEND) | none | – |
| **EXIT** listener `0x7100000230`, execute `0x7100004b10` | `exit.rs`, `Adv::update` | M | The touch buttons slide out, the banners hide (`0x71000643c0`, `0x710009e4a0`), input turns off, skip/auto off, box closes, voice and SE stop, SSETs flushed, 30-frame wait, then the save is written and the title opens. (An earlier note that it clears the layer banks misread the banner calls) | none | - |
| SGET `0x7100004d40` / SSET `0x7100004e60` | `sget.rs`, `sset.rs`, `save.rs` | M | Pending-SSET lookup, route write (spec_save) | none | – |
| WAIT `0x7100005090` | `wait.rs` | M | Skip cancels; a wait of N frames ends on frame N + 1; an interruptible one ends on a confirm or down the text did not take | none | - |
| MSGINIT listener `0x7100000690` | `msginit.rs` | M | Model `+0x90ec`. An unknown type (6 or more) or layout (4 or more) is 0, as the original's tables (`0x7100226518`, `0x7100226520`) | none | - |
| **MSGSET** ctor `0x7100005140`, execute `0x71000052e0`, slot 3 `0x71000052a0` | `msgset.rs` | M | Flags `read | (event ? 2 : 0)` and animate = `!isSkipping` as in the original; ctor's read flag and rollback points kept. The message layer does not use the read flag (`UNUSED_FLAG`), see open questions | none | – |
| MSGWAIT listener `0x71000008b0`, exec `0x7100005630` | `msgwait.rs` | M | A close clears the line, so a waiting `MSGWAIT -1` finishes as with the original's `isClosing` | none | – |
| MSGSIGNAL `0x7100005710` | `msgsignal.rs` | M | – | none | – |
| MSGSYNC `0x71000057c0` (`0x71000b1910` voice sync) | – (finishes at once with a warning) | X | Unused by main.snr | none | S |
| **MSGCLOSE** listener `0x7100000c10`, exec `0x7100005880` | `msgclose.rs` | M | Pending flag `+0x90e8` from `apply_state`; set → returns at once without closing. While skipping the wait ends at once (start still asks for the animated close; the per-frame fast-forward finishes it) | none | – |
| SELECT `0x7100005ba0` (+ slot 3 `+0x9c > 0`) | `select.rs`, `select_layer.rs` | M | engine-internals §12 (compared on screen), spec_save | none | – |
| WIPE listener `0x7100000f10`, exec `0x71000064b0` | `wipe.rs` | M | PAGEBACK flag `+0x3dd0` ↔ `is_page_back_started`; exec uses `isSkipping` (shin: root fast-forward each skipped frame) | L | – |
| WIPEWAIT `0x7100006b30` | `wipewait.rs` | M | §10 | none | – |
| BGMPLAY listener `0x71000011c0`, exec `0x7100006bf0` | `bgmplay.rs` | M | Model (looping only, `+0x8edc/+0x8ee0/+0x8ee4`), fade-in 0 while skipping, song-title banner only when a new track starts (setting v15), BGM unlock | none | – |
| BGMSTOP / BGMVOL `0x7100006ca0` / `0x7100006d20` | `bgmstop.rs`, `bgmvol.rs` | M | Fade 0 while skipping | none | – |
| BGMWAIT `0x7100006db0` | – (finishes at once with a warning) | X | `(mask & BgmPlayer status) == 0`; unused | none | S |
| BGMSYNC `0x7100006e20` | `bgmsync.rs` | M | §10 | none | – |
| SEPLAY listener `0x7100001810`, exec `0x7100006ec0` | `seplay.rs` | M | While skipping: fade-in 0, and a one-shot SE stops its slot instead of playing. play_speed ignored (all 21,036 uses are 1000) | none | – |
| SESTOP / SEVOL / SEPAN `0x7100006fa0` / `0x71000070a0` / `0x7100007140` | `sestop.rs`, `sevol.rs`, `sepan.rs` | M | Fade 0 while skipping | none | – |
| SESTOPALL `0x7100007020` | `sestopall.rs` | M | Fade 0 while skipping | none | – |
| SEWAIT `0x71000071e0`, SEONCE `0x7100007320` | `sewait.rs`, `seonce.rs` | M | §10 | none | – |
| VOICEPLAY listener `0x7100002140`, exec `0x7100007420` | `voiceplay.rs` | M | Plays the named voice with volume and flags unless skipping (84 uses, e.g. "02/10800000") | none | – |
| VOICESTOP `0x7100007700` | – (finishes at once with a warning) | X | Stops the voice; unused | none | S |
| VOICEWAIT `0x7100007760` | `voicewait.rs` | M | Voice player status | none | – |
| SYSSE `0x71000077d0` | `sysse.rs` | M | §10 | none | – |
| SAVEINFO listener `0x7100002590` | `saveinfo.rs`, `vm_state/mod.rs` `SaveInfo` | M | Writes the *pending* text for its level (`+0x90f8 + 16·level`; levels 0 and 1, others are logged and ignored); AUTOSAVE commits it. The save slots, the banner and the system menu use the committed copy | none | - |
| **AUTOSAVE** listener `0x71000026d0`, exec `0x71000078d0` | `autosave.rs`, `hud.rs` | M | Counter/snapshot, the SAVEINFO commit (`+0x9100 ← +0x90f8`, `+0x9110 ← +0x9108`) and the scene-title banner (`0x7100064240`: level-1 text, size 48, 1920 wide, bottom-left at (4, 1076), in/out 0.03, hold 240, finished by skip, gated by v14) | none | - |
| EVBEGIN / EVEND `0x71000027e0` / `0x7100002960` | `evbegin.rs`, `evend.rs` | M | Model matches; executes return 1. Event semantics live in MSGSET flags, isSkipping and event skip | none | – |
| RESUMESET / RESUME `0x7100007b00` / `0x7100007b40` | – (finish at once with a warning; RESUME stops a replay) | X | Unused | none | – |
| **SYSCALL** listener `0x7100002ce0`, exec `0x7100007c10` | `syscall.rs` | M | Id 0: `apply_state` sets system enabled `+0x913c = (argument != 0)`; start slides the touch buttons out and turns skip and auto off. One use (`0xa1456`) | none | – |
| TROPHY `0x7100007c80` | `trophy.rs` | M | Switch build does nothing (`0x71001171f0` locks/unlocks a mutex); a no-op here too | none | - |
| **UNLOCK** `0x7100007cd0` | `unlock.rs`, `unlocks.rs` | M | Type 0/1/2 add each index to the CG / BGM / movie sets (38 uses) | none | – |
| LAYERINIT `0x7100008120` (ctor `0x7100007dd0`) | `layerinit.rs` | M | Model shape matches | none | – |
| LAYERLOAD / LAYERUNLOAD / LAYERCTRL / LAYERWAIT | `layerload.rs` ... | M | Layer agent's area; LAYERLOAD and LAYERWAIT executes use `isSkipping` (shin relies on the per-frame root fast-forward) | L | – |
| LAYERSELECT listener `0x7100003740` | `layerselect.rs` | M | Both ids clamped to 0..255, then put in order | none | - |
| LAYERSWAP `0x710000b8c0` (ctor `0x710000b7b0`) | – (finishes at once with a warning; `create_back_layer_group_if_needed` disabled) | X | It render-clones the root group into `ADV+0xb0` (the back layer group); unused | none | – |
| MOVIEWAIT `0x710000bd60` | `moviewait.rs` | M | No skip test in MOVIEWAIT itself; skipping ends it through the movie layer's fast-forward (`0x71000c33d0`, checked in Eden), as in the port | none | – |
| TRANSSET / TRANSWAIT `0x710000c220` / `0x710000c260` | – (finish at once with a warning) | X | TRANSSET only stores params in the model (`+0x37d8..+0x3800`); TRANSWAIT waits on layers of a selection (`0x710007b690`). Unused | none | – |
| PAGEBACK / PLANESELECT / PLANECLEAR | `pageback.rs`, `planeselect.rs`, `planeclear.rs` | M | Model writes match (`+0x3dd0`, `+0x3dcc`) | none | – |
| MASKLOAD listener `0x71000040c0` | `maskload.rs` | M | Plane mask `+0x3380 + 0x170·plane` | none | – |
| MASKUNLOAD `0x710000d7f0` | – (finishes at once with a warning) | X | Sets the plane mask to -1; unused | none | S |
| **CHARS** listener `0x7100004370`, exec `0x710000da00` | `chars.rs` | M | Model `chars` = (arg1, arg2); execute queues notification `16·(level−1)` when the notify level is above 0 | none | – |
| **TIPSGET** ctor `0x710000da40`, exec `0x710000dc80` | `tipsget.rs` | M | Tip ids join `VmState::tips` (unique); execute queues `16·(level−1) | 1` | none | – |
| **NOTIFYSET** listener `0x7100004830` | `notifyset.rs`, `hud.rs` | M | Sets the notify level (0 none, 1, 2). Banner: texture row `(id & 1)·0.5`, 640×164 at x = 1380 − 100·slide, y = 100; slide 30 frames; alpha in 30, hold 30+120, out 60; `notify1`, or `notify2` with bit 4 | none | – |
| SHOWCHARS listener `0x7100004710`, exec `0x710000e3c0` | `showchars.rs`, `Adv::update_show_chars` | M | Skip/auto off, character screen opened with the CHARS state as a task, command waits for it. shin does not clear `+0x913c`; the open task blocks the ADV's input and modes instead. 1 use | none | – |
| QUIZ `0x710000e020` (+ slot 3 `+0x24 > 0`) | `quiz.rs` | M | §13, spec_save | none | – |
| DEBUGOUT | `debugout.rs` | M | – | none | – |
| NOP | – | M | No opcode reaches it | none | – |

## 2. What still differs

1. **The nine unused commands** (MSGSYNC, BGMWAIT, VOICESTOP, RESUMESET, RESUME, LAYERSWAP, TRANSSET, TRANSWAIT,
   MASKUNLOAD) finish at once with a warning. None is used by main.snr. The VM's four unary ops (zero, not16,
   negate, abs) are all in.
2. **Checked against the scripts** (English, Catbox X and Japanese `main.snr`, 2026-11-07): the only unary op used is
   negate; real division appears 4 times (the clock hands, non-negative inputs) and real multiplication never;
   SAVEINFO uses levels 0 and 1; MSGINIT's values (4, 5, 20, 36, and a register set to 1 or 2) are all valid;
   LAYERSELECT's ids are 0–28.

## 3. Structural

- **Skip/auto state lives in the ADV** (`AdvState::skip_auto`, `AdvState::skipping`, set each frame from
  `is_fast_forwarding`), so instant commands read it in `start`. The wait state is not shared yet: WAIT keeps its own
  counter and EXIT its own `exit_wait`.
- **Context model fields** are written in `apply_state` and rebuilt by replay, except the SAVEINFO pairs.
- **Look-ahead and replay**: `look_ahead` and `replay` in `adv/mod.rs` give rollback, the chapter jumps and event
  skip the original's stopping rules without a per-command slot 3.
- **One ADV overlay** (`AdvHud`, `hud.rs`) draws after the layers in the original order (`0x71000125a0`).

## 4. Open questions

- Initial value of `Context+0x913c` (system enabled) in the Context ctor `0x710000ea30` (not decoded; shin starts
  with it set, which the game plays as).
- The banner tweens' flag 0x40 values (0.03, 0.1, 0.2): shin reads them as per-frame rates (`hud.rs`); unverified
  against `0x71000bb960` or Eden timing.
- Whether the read flag changes the text colour: shin passes it (`UNUSED_FLAG`) but the message layer never reads it.
- `isAuto` timing: shin waits for the voice, then the v10 delay; not compared with the original frame by frame.

What I did not check: the layer commands' execute bodies beyond their skip tests (layer agent), the SELECT/QUIZ
internals (documented), HiddenWindow/SystemMenu internals, touch gestures in detail.
