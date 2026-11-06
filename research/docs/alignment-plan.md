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
| 1 | **Done** (voices play, pan to their bustups with setting v5 and drive the lip-sync; engine-internals §16). **No voices**: `VoicePlayer::play` is an empty stub and VOICEPLAY only logs; 39,649 lines carry `@v`. Checked | systems, text | M-L |
| 2 | **Done**, voice and movie volumes included. **Settings are parsed but never applied**: text speed 80 instead of v9 (default 50; text types 2.5 x too fast; checked), auto delay 80 instead of v10, no volume setting (BGM/SE about 1.4 x, system sounds 1.8 x louder than the Switch default), window opacity, voice-on-advance | systems, text | S each |
| 3 | **Done** (blinking is ported but Umineko's bustups have no eye frames; the mouth level is the original's 50 ms table, and voices pan to their bustups with setting v5). **Bustups never blink or move their lips**: property 27 (set 154 times) is ignored; the original blinks every `(1 − r²)·240 + 80` ticks and moves the mouth with the voice level (`0x7100036a00`) | layers | M |
| 4 | **Done** (rollback, the chapter jumps and event skip too; event skip checked in play in an Episode 1 event, by B and by a long press). **Skip has none of the original's rules**: a held Ctrl instead of the R/L toggles with sounds and icon; it ignores events, unread lines (v11) and the system flag; read lines are never recorded; instant commands can't see it (fades not zeroed, one-shot SEs still play). Checked (held Ctrl) | commands, systems | M |
| 5 | **Done.** **Event lines wait for a click**: MSGSET passes no flags (`read | event << 1`), so the 72 event lines don't play by themselves. Checked (flags empty) | commands | S |
| 6 | **Done.** **MSGCLOSE closes the box mid-line** after an un-waited line (35 places, e.g. `0x31ce57`) | commands | S |
| 7 | **Done** (the scene and song titles, skip/auto icons, notifications and the event-skip hint; the touch-screen buttons are not drawn). **Banners missing**: the scene title (AUTOSAVE, bottom left, size 48, 240 frames), the song title (BGMPLAY, top left with ♪), and 123 "characters/tips updated" notifications with their sounds | commands, text, systems | M |
| 8 | **Done**, voice focus included (BGM and loops ducked while a voice plays, setting v4). **Looping sound effects restart** when re-issued (about 750 places); the original keeps them and changes the volume. BGM crossfades and stops need at least 4 frames; voice focus (BGM and loops ducked to 0.5) is missing | systems | S |
| 9 | **Done.** **Layer containers clear to opaque black** instead of transparent, so plane 1's 407 dissolves show black where plane 0 should show. Checked (`layer_group.rs:417/424`, `page_layer.rs:109`) | layers | S |
| 10 | **Done** (confirmed in code). **Screen shake and zoom applied twice** while a screen or page effect (ghosting, ripple, blur) runs: `screen_layer.rs`/`page_layer.rs` pass their own transform into the effect chain instead of the parent's (`0x71000e67f0`, `0x710009f360`). To verify on screen | layers | S |
| 11 | **Done** (layer banks still break ties between layers without an id, such as the screen and the message window). **Layers with equal RenderPosition stack in reverse** (`0x7100081110`: lower layer id below) | layers | S |
| 12 | **Done**: the Check screen matches Eden line for line. **Check and choice texts**: wrong font (system.fnt, not newrodin-medium; the 1.3 % width) and not justified | text | S |
| 13 | **Done.** Read lines, CG/BGM/movie unlocks and UNLOCK are not recorded (needed by skip, galleries, the title) | systems, commands | S |
| 14 | **Done**, transparent movies included (compared: the Episode 8 fire, below). Movies ignore their repeat flag (8 uses) and the volume settings | systems | M |
| 15 | **Done**: the opening movie, title, scenario select with its door bridge and curtain wipe, system menu, config, save/load with thumbnails, hidden window, the shared dialog, the backlog and its chapter list, the Picture, Music, Movie and Character Boxes, Characters, Tips and the manual, all compared with Eden; the original's touch gestures (drags, flicks, taps, long presses) on every one of them with the mouse; the shared BGM player. No menus or game flow: title, save/load, config, backlog, system menu, scenario select, galleries | systems | L |
| 16 | **Done** (one noted shortcut: EXIT does not clear the layer banks, so the title fades in over the last scene). Smaller: wobbler phase reset, wobble slots 44-55 rewired, negative shader `× param.xyz`, blend type 2 equation, MaskWiper scale flag 0x8, TextObject outline clipped by 1 px, SYSCALL 0's system flag, EXIT's teardown, auto mode, the nine unused commands that would panic | all | S each |

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

## Effects compared with the original

Captured in the original (Eden, `research/tools/compare/eden_capture.py` stopping on the line, then a burst of
window grabs for transitions) and in the port (`--episode N --capture-messages <id>`, through the episode's own
set-up; `--unsafe-entry-point` skips the start-up block, so sprites and the text box come out wrong there).

| Effect | Where | Result |
|---|---|---|
| Blur (property 66) on the background | Ep2 line 7652 | Match: mean difference 0.43 in the background, 99.9 % of the upper half within 32 |
| Whirl wipe (WIPE 8, with ghosting 900) | Ep2 after line 11555 | Match: same spiral and unwinding; the line after it, 11556, mean difference 0.81, 99.6 % within 32 |
| Butterfly summoning (pictures 862, 854, 856 and the golden glow) | Ep2 lines 11555-11556 | Same stages and look frame by frame |
| Glass wipe (WIPE 9) | Ep5 after line 32648 | Match: same flash, lighting (lit faces up, dark sides) and fade at the end; shard paths are random in both |
| Mosaic (property 67, 255 to 0 over 60 frames) | Ep5 after line 34483 | Match: the same block sizes frame by frame over the same 10 frames, the grid anchored top left; the finished pictures within 0.3 |
| Ripple (property 82) with ghosting on the screen layer | Ep8 line 61681 | Match: mean difference 1.36, 99.3 % within 32 |
| Looping transparent movie (no0011, the fire) | Ep8 line 64886 | Match: the same looping flames over the garden and the same shake (the capture tool's own screenshot came before Eden's first movie frame; live grabs show the fire) |
| Halloween party quiz screen | Ep8, question 2 | Match: same layout, no offset; mean difference 6.4 against a small window grab of Eden (the rest is the grab's resampling) |
| Dissolve (properties 68/69, mode 1) on the plane | Tea party 1 after line 7409 | Match: the same left-to-right sweep into blown-away dots over white, at the same pace; the dots themselves differ (random in both) |
| Raster (properties 76-78) with ghosting on the screen layer | Tsubasa story 1 after line 66579 | Match: the same fade to white and the same wavy edges building with the same curve (edge darkness 0 to 67 in both); matched frames within 3.6 |

Still to compare: the cake game, the culprit game's screens and the last choice. The cake and culprit games were
played through in Eden but not captured; the last choice has not been looked at in Eden. The culprit game's
testimony text was compared (it breaks lines mid-word in English as the original does, engine-internals §13).

## Accuracy

**99.7 %**: the mean, over every side-by-side comparison with a recorded per-pixel figure, of the share of pixels
whose difference from the original is within 32 of 255 (the largest channel difference):

`accuracy = (1/N) × Σ share_i`, where `share_i` = pixels at most 32/255 away from the original's ÷ pixels compared,
for comparison i, and N = 6.

| Comparison | Within 32 | Original captured at |
|---|---|---|
| Blur (property 66), Ep2 line 7652, the upper half (the blurred background) | 99.90 % | 3x, downscaled |
| Whirl wipe, Ep2: line 11556 right after it | 99.60 % | 3x, downscaled |
| Ripple with ghosting on the screen layer, Ep8 line 61681 | 99.30 % | 3x, downscaled |
| Characters screen, Episode 1 | 100.00 % | 1x |
| Tips screen, Episode 1 | 99.94 % | 1x |
| Tips episode list on the title | 99.40 % | 1x; the header band's slowly drifting floral pattern was caught at a different moment |

(99.9 + 99.6 + 99.3 + 100 + 99.94 + 99.4) / 6 = 99.69, shown as 99.7 %.

Capture the original with Eden at **1x** (Umineko's per-game graphics setting; Eden numbers it 3), which renders the
Switch's native 1920x1080, and take Eden's own screenshot (`eden_capture.py --shot` for menus, or the line capture).
At 3x the screenshot has to be downscaled, and the resampling alone cost one to two points on text-heavy screens:
the same three menus measured 98.0-99.1 % from 3x captures and 99.4-100 % at 1x. The three story effects above are
still 3x captures and will likely gain the same way when recaptured. Two screens compared at 1x match pixel for pixel
except for animation timing; no layout, colour or text difference remains in them. The quiz (95.7 % from a small
window grab) and the title (its menu items depend on the save) are not in the number.

Compared with the original and found to match, but left out of the number because no within-32 share was recorded
for them (only a mean difference, a frame-by-frame look, or random patterns that differ in both by design): the
glass wipe, the mosaic (finished pictures within 0.3), dissolve, raster (matched frames within 3.6), the butterfly
summoning, the looping fire movie, the Character Box (mean difference under 2), the story choices, the Check text,
the backlog (rows within 2 px), the title, scenario select, bridge, system menu, config, save/load, Picture, Music
and Movie Boxes and the manual (see engine-internals §12 and §15). To add a comparison: capture both sides as
described above, measure its within-32 share and add a row here.
