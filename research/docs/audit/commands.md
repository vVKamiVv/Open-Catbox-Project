# Audit: script commands, the VM and the adventure screen's command layer

Area: every `shin::ADV::*` class (class-inventory "Script commands"), the VM run loop, and how `shin::ADV` drives
commands, input, skip/auto and read flags. shin side: `shin/src/adv/{mod.rs,command/*.rs,vm_state/*.rs}`,
`shin-core/src/vm`, `shin/src/app.rs`. Logs and decompiles: `$S\audit\commands\` (d1.c = all 59 listeners + slots
1-4 of every command; d2-d5 = ADV update/render/input, Context helpers, HID; adv_dis.txt = disassembly
0x7100010000-0x7100039f2c). Save format not re-audited (see `$S\mur\spec_save.md`, cited as *spec_save*).

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
  - System enabled, not in an event: Plus → "jump" dialog (`0x7100012c80` → `0x710000f4f0`: look-ahead over
    skippable commands to the next stop, text depends on v11; `error` sound if nothing to jump to); Minus → another
    dialog (`0x7100012f80`, purpose not identified); Y → system menu (`0x7100013320`, `page`); X → backlog
    (`0x7100013400`, `page` / `error`); cancel → hide window (`0x7100013720`, `shin::ADV::HiddenWindow`).
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
| VM run loop `0x71000ef450`, `rnd` `0x71000ed3c0` | `shin-core/src/vm/mod.rs` | M | PRNG steps once per instruction before it runs, dispatch until a command; rnd formula fixed today. Unary ops other than zero/negate `todo!()` (only negate used, engine-internals §10) | none | – |
| ADV task update `0x7100010fb0` (command loop) | `Adv::update` | M/D | Same loop shape. WAIT ends 1 frame early: the original decrements the timer after the command loop, shin inside WAIT's update on its start frame (`wait.rs`) | L | S |
| ADV input `0x7100011ea0` | `Adv::handle_input`, `app.rs` | D | shin: Enter/Space/LMB advance, Ctrl **held** = fast-forward; no skip/auto toggles, no stop-skip keys, no backlog, system menu, hide window, rollback, jump, event skip. Interruptible WAIT is ended even when the press advanced the text (original: only if the message layer did not take it) | H | M (+ menus L) |
| `isSkipping` `0x7100012430` | `Adv::is_fast_forwarding` | D | shin = key held or debug seek; ignores events (`+0x2ddc`), system flag (`+0x913c`), read-only setting v11 (default 1) and the read flag; skips **unread** text and **inside events** | H | M |
| `isAuto` `0x71000122f0`, auto toggle (L) | – (`MessageLayer::autoplay_requested` never set) | X | No auto mode | H | M |
| Read flags: MSGSET ctor `+0x2dd9` (`0x71000d19f0` on seen set), writer `0x7100014f50`/`0x7100014fa0` (spec_save 6.2) | – (`SaveData::seen_messages` never written; MessageLayer "TODO: notify the listener") | X | Nothing is ever marked read; MSGSET does not compute the read flag | H | S–M |
| skip/auto icons, touch HUD `+0x500`, event hint `+0x4a0` (render `0x71000125a0`) | – | X | No skip/auto indicator (blinking icon top-left) | M | M |
| Notification queue `0x71000148f0`, banner in ADV update/render | – | X | See CHARS/TIPSGET | M | M |
| `ADV::Context` (`+0x2530..+0x9170`) | `VmState` (+`SaveState`, `LayersState`, `AudioState`, `MessageState`, `SaveInfo`) | T | Missing model fields: `+0x913c` system enabled, `+0x9118/+0x911c` CHARS, `+0x9120` tips vector, `+0x9138` notify level, `+0x2dd9` line read, `+0x2db4/+0x2db8` previous/last line save points (rollback), `+0x2dbc/+0x2dc0` autosave points, `+0x2dcc/+0x2dd0` line counts, `+0x90f8..+0x9118` SAVEINFO pending/committed pairs | M | M |
| `ADV::First` (`0x7100019930`) | `Adv::new` | M | New game or load start | none | – |
| `ADV::SceneLoader` (`0x710001b340`, `0x7100019ea0`) | `Adv::new` + `restore.rs` | M | Save agent's area; replay used only for loads, not for rollback / jump / event skip | – | – |
| `ADV::HiddenWindow` (ctor `0x710001bd80`) | – | X | Hide-window mode (B / swipe) | M | M |
| `ADV::SystemMenu` (ctor `0x710001c100`) | – | X | Menus area | H | L |
| `ADV::Command` slot 3 "skippable" | – | X | Needed only for jump / event skip | M | M |
| **EXIT** listener `0x7100000230`, execute `0x7100004b10` | `exit.rs` | D | Original: `syscall0Fade`; hide both banners (`0x71000643c0`, `0x710009e4a0`); input off `+0x480=0`, modal `+0x470=1`, skip/auto off; animated message-box close; voice stop `0x710011a6b0`; SE fade-to-silence + stopAll; 30-frame wait; then when the wait, HUD tween, box close and task pool are done: flush SSETs (`0x710000fab0`, only on the live context `+0x9170`), write the save file and go to the menus (`0x71000147e0` → `0x71000d52f0`). Never returns 1. shin stops SE, checkpoints, stops the script; box, voice, wait and menus missing | M | S (+menus) |
| SGET `0x7100004d40` / SSET `0x7100004e60` | `sget.rs`, `sset.rs`, `save.rs` | M | Pending-SSET lookup, route write (spec_save) | none | – |
| WAIT `0x7100005090` | `wait.rs` | M/D | Skip cancels; interrupt rule and 1-frame timing above | L | S |
| MSGINIT listener `0x7100000690` | `msginit.rs` | M | Model `+0x90ec`; MSGSET reads it with clamps (type < 6 via table `0x7100226518`, layout < 4 via `0x7100226520`, else 0) | L | S |
| **MSGSET** ctor `0x7100005140`, execute `0x71000052e0`, slot 3 `0x71000052a0` | `msgset.rs` | D | Execute passes flags `read | (event ? 2 : 0)` (shin's `MessageFlags::UNUSED_FLAG` / `IGNORE_INPUT`) and "animate" = `!isSkipping`; shin passes **no flags** and always `true`. Effect: the 72 lines inside EVBEGIN..EVEND (e.g. `0x1cfce8` Battler, `0x46993a`) wait for a click instead of playing by themselves; no read colour. Ctor's read flag and rollback points missing (above) | H | S |
| MSGWAIT listener `0x71000008b0`, exec `0x7100005630` | `msgwait.rs` | M | For -1 the original is also done when the box is closing (`isClosing`); shin not | L | S |
| MSGSIGNAL `0x7100005710` | `msgsignal.rs` | M | – | none | – |
| MSGSYNC `0x71000057c0` (`0x71000b1910` voice sync) | – (`todo!()` panic) | X | Unused by main.snr | none | S |
| **MSGCLOSE** listener `0x7100000c10`, exec `0x7100005880` | `msgclose.rs` | D | Ctor copies the pending flag `+0x90e8` (set by an un-waited MSGSET, cleared by `MSGWAIT -1`) into `+0x19`; **if set, execute returns 1 without closing the box**. shin always closes. 35 MSGCLOSEs in main.snr follow an un-waited line, e.g. `0x31ce57` (Maria/Beatrice umbrella line `0x31cdcf`): shin slides the box out mid-line. Skip closes without animation (shin: same via `is_fast_forwarding` only in update, start always animates) | H (35 scenes) | S |
| SELECT `0x7100005ba0` (+ slot 3 `+0x9c > 0`) | `select.rs`, `select_layer.rs` | M | engine-internals §12 (compared on screen), spec_save | none | – |
| WIPE listener `0x7100000f10`, exec `0x71000064b0` | `wipe.rs` | M | PAGEBACK flag `+0x3dd0` ↔ `is_page_back_started`; exec uses `isSkipping` (shin: root fast-forward each skipped frame) | L | – |
| WIPEWAIT `0x7100006b30` | `wipewait.rs` | M | §10 | none | – |
| BGMPLAY listener `0x71000011c0`, exec `0x7100006bf0` | `bgmplay.rs` | D | Model matches (looping only, `+0x8edc/+0x8ee0/+0x8ee4`). Missing: fade-in 0 while skipping; **song-title banner** (`0x710009e160` on `ADV+0x2e0`, only when a new track really starts, setting v15); BGM unlock (`+0x80`, spec_save 6.3) | M | M |
| BGMSTOP / BGMVOL `0x7100006ca0` / `0x7100006d20` | `bgmstop.rs`, `bgmvol.rs` | D | Fade 0 while skipping | L | S |
| BGMWAIT `0x7100006db0` | – (`todo!()`) | X | `(mask & BgmPlayer status) == 0`; unused | none | S |
| BGMSYNC `0x7100006e20` | `bgmsync.rs` | M | §10 | none | – |
| SEPLAY listener `0x7100001810`, exec `0x7100006ec0` | `seplay.rs` | D | While skipping: fade-in 0, and a **one-shot SE is not played** (the slot is stopped instead, `0x71000f2b30`). shin plays every SE during skip. play_speed ignored (all 21,036 uses are 1000) | M | S |
| SESTOP / SEVOL / SEPAN `0x7100006fa0` / `0x71000070a0` / `0x7100007140` | `sestop.rs`, `sevol.rs`, `sepan.rs` | D | Fade 0 while skipping | L | S |
| SESTOPALL `0x7100007020` | `sestopall.rs` | D | Fade 0 while skipping (§10) not applied | L | S |
| SEWAIT `0x71000071e0`, SEONCE `0x7100007320` | `sewait.rs`, `seonce.rs` | M | §10 | none | – |
| VOICEPLAY listener `0x7100002140`, exec `0x7100007420` | `voiceplay.rs` | S | Plays the named voice (`0x710011a000`, volume, flags) unless skipping; 84 uses (e.g. "02/10800000") | M | M (voice player) |
| VOICESTOP `0x7100007700` | – (`todo!()`) | X | Stops the voice; unused | none | S |
| VOICEWAIT `0x7100007760` | `voicewait.rs` | M | Correct, but the voice player is a stub | – | – |
| SYSSE `0x71000077d0` | `sysse.rs` | M | §10 | none | – |
| SAVEINFO listener `0x7100002590` | `saveinfo.rs` | D | Original writes a *pending* text per level (`+0x90f8 + 16·level`); AUTOSAVE commits it (`+0x9100 ← +0x90f8`, `+0x9110 ← +0x9108`); the save slot and the banner use the committed copy. shin sets it at once; `set_save_info` accepts level 4 but the array has 4 entries (panics; script uses 0 and 1 only) | L | S |
| **AUTOSAVE** listener `0x71000026d0`, exec `0x71000078d0` | `autosave.rs` | S | Counter/snapshot match. Missing: commit of SAVEINFO texts; **scene-title banner** (`0x7100064240` on `ADV+0x1f0`): the committed level-1 SAVEINFO text (188 chapter titles, e.g. "Letter and Umbrella"), font 48, layout width 1920, anchor (0, 1) at (4, 1076) = bottom-left; alpha tween in (0.03, flag 0x40), hold 240 (flag 0x1080), out (0.03, flag 0x40); skip finishes it; draw `0x7100064480` gated by v14. This answers engine-internals §9 "how chapter titles are drawn" | H | M |
| EVBEGIN / EVEND `0x71000027e0` / `0x7100002960` | `evbegin.rs`, `evend.rs` | M | Model matches; executes return 1 (shin's `warn!("TODO")` is noise). Event semantics live in MSGSET flags, isSkipping and event skip | none | S (drop warn) |
| RESUMESET / RESUME `0x7100007b00` / `0x7100007b40` | – (`todo!()`; replay stops on RESUME then panics) | X | Unused | none | – |
| **SYSCALL** listener `0x7100002ce0`, exec `0x7100007c10` | `syscall.rs` | D | Id 0: the **listener** sets system-enabled `+0x913c = (argument != 0)` (gates skip, auto, menus, HUD); execute hides the touch HUD (`syscall0Fade`: `+0x500` → 0 if shown) and turns skip and auto off (`+0x47c = 0`). The one use (`0xa1456`, argument 0, in the ending helper before EXIT) disables skipping and menus for the rest of that sequence. shin: warning only | M | S |
| TROPHY `0x7100007c80` | `trophy.rs` | M | Switch build does nothing (`0x71001171f0` locks/unlocks a mutex); shin's warn is noise | none | S |
| **UNLOCK** `0x7100007cd0` | `unlock.rs` | S | Type 0/1/2 add each index to the global CG `+0x68` / BGM `+0x80` / movie `+0x98` sets (`0x71000d1930`). shin has `unlocked_cgs/bgms/movies` in `SaveData` but never writes them (38 uses) | M (gallery) | S |
| LAYERINIT `0x7100008120` (ctor `0x7100007dd0`) | `layerinit.rs` | M | Model shape matches | none | – |
| LAYERLOAD / LAYERUNLOAD / LAYERCTRL / LAYERWAIT | `layerload.rs` ... | M | Layer agent's area; LAYERLOAD and LAYERWAIT executes use `isSkipping` (shin relies on the per-frame root fast-forward) | L | – |
| LAYERSELECT listener `0x7100003740` | `layerselect.rs` | D | Original clamps both ids to 0..255 then swaps; shin swaps, does not clamp | none | S |
| LAYERSWAP `0x710000b8c0` (ctor `0x710000b7b0`) | – (`todo!()`; `create_back_layer_group_if_needed` disabled) | X | It render-clones the root group into `ADV+0xb0` (the back layer group); unused | none | – |
| MOVIEWAIT `0x710000bd60` | `moviewait.rs` | M | No skip test in the original either | none | – |
| TRANSSET / TRANSWAIT `0x710000c220` / `0x710000c260` | – (`todo!()`) | X | TRANSSET only stores params in the model (`+0x37d8..+0x3800`); TRANSWAIT waits on layers of a selection (`0x710007b690`). Unused | none | – |
| PAGEBACK / PLANESELECT / PLANECLEAR | `pageback.rs`, `planeselect.rs`, `planeclear.rs` | M | Model writes match (`+0x3dd0`, `+0x3dcc`) | none | – |
| MASKLOAD listener `0x71000040c0` | `maskload.rs` | M | Plane mask `+0x3380 + 0x170·plane` | none | – |
| MASKUNLOAD `0x710000d7f0` | – (`todo!()`) | X | Sets the plane mask to -1; unused | none | S |
| **CHARS** listener `0x7100004370`, exec `0x710000da00` | `chars.rs` | S | Model: `+0x9118/+0x911c` = (arg1, arg2) (used by the character screen). Execute: if notify level `+0x9138 > 0`, queue notification `16·(level−1)` (`0x71000148f0`). 110 banners in main.snr | M | M |
| **TIPSGET** ctor `0x710000da40`, exec `0x710000dc80` | `tipsget.rs` | S | Ctor adds each tip id to the playthrough tip vector `+0x9120` (unique). Execute queues notification `16·(level−1) | 1`. 13 banners | M | M |
| **NOTIFYSET** listener `0x7100004830` | `notifyset.rs` | S | Only sets `+0x9138` (0 none, 1, 2). Banner: one id at a time from a byte queue (deduplicated per kind), texture row `(id & 1)·0.5` (characters / tips), 640×164 at x = 1380 − 100·slide, y = 100; slide tween 0→1 over 30 frames, hold; alpha 0→1 30, hold 30+120, →0 over 60; sound `notify1`, or `notify2` when bit 4 is set (level 2) | M | M |
| SHOWCHARS listener `0x7100004710`, exec `0x710000e3c0` | `showchars.rs` | S | Saves and clears `+0x913c`, turns skip/auto off, opens the character screen (`0x7100055060` with the scenario and CHARS state) as a task and waits for it; the dtor restores `+0x913c`. 1 use | L | L (menus) |
| QUIZ `0x710000e020` (+ slot 3 `+0x24 > 0`) | `quiz.rs` | M | §13, spec_save | none | – |
| DEBUGOUT | `debugout.rs` | M | – | none | – |
| NOP | – | M | No opcode reaches it | none | – |

## 2. Top divergences (ordered by what the player sees and hears)

1. **Skip is a held key with none of the original's rules** (`Adv::is_fast_forwarding`, `app.rs`). Original: R
   (on release) toggles a skip mode, L toggles auto, X/Y/cancel stop both (`cancel` sound), on/off sounds `son` /
   `cancel`, a blinking icon top-left; skipping stops at unread lines when v11 (default 1) is on, never runs inside
   EVBEGIN..EVEND, and is off while `+0x913c` is cleared. Fix: keep `skip_auto: u32` (bit 1 skip, bit 0 auto) in
   `Adv`, implement `is_skipping(&AdvState, &VmState)` exactly as `0x7100012430` and `is_auto` as `0x71000122f0`,
   and map PC keys to the Switch roles (e.g. R → Ctrl toggle, L → A key; keep "hold Ctrl" as a PC extra only if
   wanted). Depends on 2 for the read gate.
2. **Read flags are never recorded or used.** Fix: (a) MessageLayer: where it says "TODO: notify the listener that
   the message is done", report the message id; `Adv` inserts it into `global.save_vectors.seen_messages` if >= 0
   (`0x7100014f50`; mind the script-id − 1 convention, engine-internals §10); (b) `MSGSET::apply_state` computes
   `vm_state.save.line_read = msg_id < 0 || seen_messages.contains(msg_id)` (`0x7100005140`). This unlocks
   skip-read-only, the read colour, and keeps Switch saves' read state meaningful.
3. **MSGSET passes no flags** (`msgset.rs`, the commented-out block). Fix: `flags = (read ? UNUSED_FLAG : 0) |
   (vm_state.save.event.is_some() ? IGNORE_INPUT : 0)` and pass `!is_skipping` instead of the constant `true`
   (`0x71000052e0`). The 72 event lines then play by themselves as on the Switch.
4. **MSGCLOSE closes the box after an un-waited line.** Fix: return the pending flag from `apply_state`
   (`state.save.message_pending` before the update) as `StateInfo`; in `start`, if it was set, finish without
   calling `close` (`0x7100000c10` / `0x7100005880`). 35 places, all mid-sentence closes today.
5. **Chapter-title banner (AUTOSAVE) and song-title banner (BGMPLAY) are missing.** Fix: an ADV overlay object
   for each (`ADV+0x1f0` `0x7100064240`/`0x7100064480`; `ADV+0x2e0` `0x710009e160`/`0x710009e590`), drawn after the
   layers; AUTOSAVE commits SAVEINFO and shows level 1 (values in the table); BGMPLAY shows the BGM's display name
   only when the player reports a new track. Both respect settings v14/v15 and are finished instantly while skipping.
6. **Skip-time audio rules are not applied** (BGMPLAY/BGMSTOP/BGMVOL/SEPLAY/SESTOP/SESTOPALL/SEVOL/SEPAN fade 0;
   SEPLAY one-shots suppressed; VOICEPLAY not played; MSGCLOSE without animation). Fix: once 1 exists, read
   `is_skipping` in each `start` (needs `AdvState` access, which `start` already has) — S each.
7. **Notifications (NOTIFYSET/CHARS/TIPSGET)**: 123 "characters/tips updated" banners with `notify1`/`notify2`
   sounds are missing. Fix: model fields `notify_level`, `chars: (i32, i32)`, `tips: Vec<i32>` in `VmState`
   (set in `apply_state`); a byte queue + banner in the ADV overlay as `0x71000148f0` and the ADV update/render.
8. **SYSCALL 0** should set `system_enabled = argument != 0` in `apply_state` and turn skip/auto off in `start`
   (the "fade" is only the touch HUD). **UNLOCK** should insert into `SaveData` unlock sets. **EXIT** should close the
   box, stop the voice, wait 30 frames, then write the file and leave (menus permitting). All S.
9. **Auto mode** (L): wire `MessageLayer::autoplay_requested` from `is_auto` each frame (original: gated like skip,
   not in events). M.
10. **Missing ADV features that reuse the replay**: roll back one line (up), jump to next stop (Plus), event skip
    (cancel in an event), hide window (cancel), backlog (X), system menu (Y). The first three only need a cloneable
    `Scripter`+`VmState` and a `skippable()` mirror of slot 3, since shin's replay already exists. M each, menus L.

## 3. Structural

- **Skip/auto/system state belongs to the ADV, not to a per-frame bool.** shin passes `is_fast_forwarding` only to
  yielding commands' `update`, so instant commands cannot behave like the original, and the gates (event, system
  flag, read flag, modal) have no home. Proposed shape: `AdvState { skip_auto: u32, modal: bool, input_enabled:
  bool, wait: WaitTimer { interruptible, frames }, hud: AdvHud }` mirroring `ADV+0x470/+0x47c/+0x480/+0x490/+0x494`,
  with `is_skipping()`/`is_auto()` methods; WAIT and EXIT use the shared `WaitTimer` (`0x71000138d0..0x7100013900`),
  decremented once per frame after the command loop as in `0x7100010fb0` (fixes the 1-frame drift and the interrupt
  rule in one place).
- **Context model fields.** Everything a listener writes must live in `VmState` and be written in `apply_state`,
  because loads rebuild it by replay: add `system_enabled (+0x913c, initial value per Context ctor)`, `line_read
  (+0x2dd9)`, `prev_line_point/last_line_point (+0x2db4/+0x2db8)`, `notify_level`, `chars`, `tips`, SAVEINFO
  pending/committed. Today several stubs only `warn!` in `apply_state`, so their state is lost on load.
- **A `skippable()` per command** next to `replay_command` (slot 3, values in the table / spec_save 4.3) plus
  `Clone` on `Scripter`/`VmState` gives rollback, jump and event skip with the original's exact stopping rules.
- **One ADV overlay (`AdvHud`)** rendered after the layers in the original order (`0x71000125a0`): fade quad,
  scene-title banner, song-title banner, skip/auto icons, notification banner, touch HUD, event hint. Each feature
  above then has a single place to go instead of ad-hoc layers.
- **Unknown opcodes panic** (`apply_command_state_and_start` `_ => todo!()`; RESUME is a replay stop and would then
  panic). Map the nine unused commands to their simple behaviour (most are one line) or at least `warn!` + finish.

## 4. Open questions

- Initial value of `Context+0x913c` (system enabled) in the Context ctor `0x710000ea30` (not located in the
  decompile; the ADV update treats it as normally on).
- Minus button dialog `0x7100012f80` (callbacks `0x71001d4000` / `0x71001d4090`): return to title? 
- The banner tweens' flag 0x40 durations (0.03, 0.1, 0.2): seconds or per-frame rate; check `0x71000bb960`.
- Whether the read flag also changes the text colour in shin's MessageLayer (`UNUSED_FLAG` name suggests shin never
  found its reader).
- `isAuto`'s interaction with voices (MessageLayer waits for the voice) and the auto speed v10.
- Which texture archive `ADV+0x150` holds the icons, notification and hint images.

What I did not check: the layer commands' execute bodies beyond their skip tests (layer agent), the SELECT/QUIZ
internals (documented), HiddenWindow/SystemMenu internals, touch gestures in detail.
