# Aligning shin with the original engine: audit results and plan

An audit (November 2026) compared every part of shin with the Switch v1.0.3 executable, area by area. The full
reports, with addresses for every claim, are in [`audit/`](audit/):
[commands](audit/commands.md) (script VM, commands, the adventure screen),
[layers](audit/layers.md) (layers, effects, transitions, shaders),
[text](audit/text.md) (message window, layout, fonts, TextObject) and
[systems](audit/systems.md) (menus, audio, video, settings, input, assets).

**Verdict: align shin piece by piece instead of rewriting it.** Its core already matches the original: the VM run
loop, the command split into a model update (`apply_state`, the original's listener) and an execute part, the message
text layouter (line extents within 1 px of the original on captured lines), the layer programs 8-25 with their blend
states, colour and alpha maths, the effect chain (blur, mosaic, raster, ripple, dissolve, ghosting) and the mask
wiper. What diverges is local, or sits in a few structural spots listed below.

"Checked" marks a finding read again in shin's code while merging; the rest come from the reports' own reading of
both codebases.

## What the player notices most

| # | Finding | Area | Size |
|---|---|---|---|
| 1 | **Done** (voices play; panning by speaker position not yet). **No voices**: `VoicePlayer::play` is an empty stub and VOICEPLAY only logs; 39,649 lines carry `@v`. Checked | systems, text | M-L |
| 2 | **Done** except voice and movie volumes (still with voices and movies). **Settings are parsed but never applied**: text speed 80 instead of v9 (default 50; text types 2.5 x too fast; checked), auto delay 80 instead of v10, no volume setting (BGM/SE about 1.4 x, system sounds 1.8 x louder than the Switch default), window opacity, voice-on-advance | systems, text | S each |
| 3 | **Done** (blinking is ported but Umineko's bustups have no eye frames; the mouth level is the original's 50 ms table, and voices pan to their bustups with setting v5). **Bustups never blink or move their lips**: property 27 (set 154 times) is ignored; the original blinks every `(1 − r²)·240 + 80` ticks and moves the mouth with the voice level (`0x7100036a00`) | layers | M |
| 4 | **Done** (rollback, the chapter jumps and event skip too; event skip not yet checked in play). **Skip has none of the original's rules**: a held Ctrl instead of the R/L toggles with sounds and icon; it ignores events, unread lines (v11) and the system flag; read lines are never recorded; instant commands can't see it (fades not zeroed, one-shot SEs still play). Checked (held Ctrl) | commands, systems | M |
| 5 | **Done.** **Event lines wait for a click**: MSGSET passes no flags (`read | event << 1`), so the 72 event lines don't play by themselves. Checked (flags empty) | commands | S |
| 6 | **Done.** **MSGCLOSE closes the box mid-line** after an un-waited line (35 places, e.g. `0x31ce57`) | commands | S |
| 7 | **Done** (the scene and song titles, skip/auto icons, notifications and the event-skip hint; the touch-screen buttons are not drawn). **Banners missing**: the scene title (AUTOSAVE, bottom left, size 48, 240 frames), the song title (BGMPLAY, top left with ♪), and 123 "characters/tips updated" notifications with their sounds | commands, text, systems | M |
| 8 | **Done** except voice focus (needs voices). **Looping sound effects restart** when re-issued (about 750 places); the original keeps them and changes the volume. BGM crossfades and stops need at least 4 frames; voice focus (BGM and loops ducked to 0.5) is missing | systems | S |
| 9 | **Done.** **Layer containers clear to opaque black** instead of transparent, so plane 1's 407 dissolves show black where plane 0 should show. Checked (`layer_group.rs:417/424`, `page_layer.rs:109`) | layers | S |
| 10 | **Done** (confirmed in code). **Screen shake and zoom applied twice** while a screen or page effect (ghosting, ripple, blur) runs: `screen_layer.rs`/`page_layer.rs` pass their own transform into the effect chain instead of the parent's (`0x71000e67f0`, `0x710009f360`). To verify on screen | layers | S |
| 11 | **Done** (layer banks still break ties between layers without an id, such as the screen and the message window). **Layers with equal RenderPosition stack in reverse** (`0x7100081110`: lower layer id below) | layers | S |
| 12 | **Done**: the Check screen matches Eden line for line. **Check and choice texts**: wrong font (system.fnt, not newrodin-medium; the 1.3 % width) and not justified | text | S |
| 13 | **Done.** Read lines, CG/BGM/movie unlocks and UNLOCK are not recorded (needed by skip, galleries, the title) | systems, commands | S |
| 14 | **Done** (transparent movies still unsupported). Movies ignore their repeat flag (8 uses) and the volume settings | systems | M |
| 15 | **Mostly done**: the opening movie, title, scenario select with its door bridge, system menu, config, save/load with thumbnails, hidden window, the shared dialog, the backlog and its chapter list, the Picture, Music and Movie Boxes, Characters, Tips and the manual, compared with Eden (pointer drags and the Character Box remain); the scenario select's curtain wipe, the bridge's glow, transparent movies and the shared BGM player are in. No menus or game flow: title, save/load, config, backlog, system menu, scenario select, galleries | systems | L |
| 16 | **Mostly done** (wobble slots 44-55 and EXIT's teardown remain). Smaller: wobbler phase reset, wobble slots 44-55 rewired, negative shader `× param.xyz`, blend type 2 equation, MaskWiper scale flag 0x8, TextObject outline clipped by 1 px, SYSCALL 0's system flag, EXIT's teardown, auto mode, the nine unused commands that would panic | all | S each |

## Structural changes (prevent whole classes of bugs)

1. **One settings/Global handle owned by the app**, read by every subsystem as the original reads `*0x71002264f0`
   (systems). Today the save data lives inside `Adv`'s `VmState`, so the message layer and audio hard-code values.
2. **Skip/auto/wait state in the ADV** (`ADV+0x470..+0x494`) with `is_skipping`/`is_auto` exactly as
   `0x7100012430`/`0x71000122f0`, visible to every command's start (commands).
3. **One ADV overlay** drawn after the layers in the original order (`0x71000125a0`): fade, scene title, song title,
   skip/auto icons, notifications (commands).
4. **One TextObject type** with the original's parameter block and one font, building `LayoutParams` the way
   `0x7100101360` does (Justify, rubi 0.4 × size, width − padding) (text).
5. **One layer container implementation** instead of three drifting copies (`LayerGroup`, `PageLayer`,
   `ScreenLayer`), always passing the parent transform and clearing to transparent (layers).
6. **One property table and blend ids as in the original** (layers).
7. **Audio gain computed every frame** (`setting × duck × tween`) as the original does, kira only mixing (systems).
8. **A scene and task framework** (one current scene, a task pool of menus with input priority) before any menu,
   and **input as the engine's bit mask** with a binding table for keyboard, mouse and gamepad (systems).
9. **A side-effect-free replayer** reused by the backlog, save thumbnails, rollback and jump (systems, commands);
   the load replay (`replay_command`) is its seed.

## Order of work

1. **Quick wins** on every line: the settings handle (structural 1) with text speed, auto delay and volumes; MSGSET
   flags; MSGCLOSE; transparent container clears; the double transform; the layer sort; the looping-SE rule; read
   lines and unlocks recorded; the Check/choice font and justification.
2. **Voices**, then lip-sync and blinking on top of them.
3. **Skip and auto as on the Switch** (structural 2), with the ADV controls and gamepad input (structural 8's input
   half).
4. **Overlays**: scene and song titles, notifications, skip/auto icons (structural 3).
5. **Scene/task framework**, then config, system menu, save/load menus, title and scenario select, backlog on the
   replayer (structural 9), galleries.
6. The smaller items of row 16 alongside the areas they touch.

Each step: fix from the original's addresses in the reports, compare with Eden on screen, then update
`engine-internals.md`.
