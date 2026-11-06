# Audit: text (message window, parser, layouter, fonts, TextObject)

> **Snapshot.** This report describes shin as it stood at the November 2026 audit; its Status and shin columns
> are kept as written so the evidence stays readable. Since then: voices play (with panning and lip-sync), the Check and choice texts use the original's font and match
> Eden line for line, and the scene-title and song-title banners and the backlog are in. Current status of every finding:
> [alignment-plan.md](../alignment-plan.md), which also lists what was compared with the original and the port's
> measured accuracy.

Logs and decompiles: `$S\audit\text\` (dec1–dec6.c, fontload.txt, setmsg_pre.txt, fntcmp.py, gotscan.py, fonts\*.fnt).
All original addresses are from the v1.0.3 `main`.

## 0. The open issue first: the Check text is 1–2 % wider than the original (fixed since: it now matches Eden)

**Root cause: wrong font.** The engine loads three fonts at boot (`0x7100065ae8`..`0x7100065cb0`, the adrp/add
pairs found by scanning the code):

| file | stored in (GOT → global) | used by |
|---|---|---|
| `/system.fnt` | `0x71002267c8` → `0x71002291d8` | every `TextObject` outside the choice menu: the culprit game's Check text (`0x7100094480`), the song-title banner (`0x710009e160`), the AUTOSAVE scene-title banner (`0x7100064240`), save menu (`0x71000d6640`), backlog chapter selector (`0x71000a96d0`), manual, CG mode, and others (`gotscan.py` lists 18 sites) |
| `/newrodin-medium.fnt` | `0x71002268b8` → `0x71002291e0` | message box regular (`0x71000af208`), backlog (`0x71000a3524`, `0x71000a5b24`, `0x71000af208`), **SelectLayer** (`0x71000f027c`) |
| `/newrodin-bold.fnt` | `0x71002268c0` → `0x71002291e8` | message box bold, backlog bold |

So `*0x71002267c8` is **system.fnt**, a different font from shin's AdvFonts medium/bold. `TextBlock`
(`shin/src/layer/text_block.rs:56`) always lays out with `medium_font`/`bold_font`.

Metrics (from the ROM, `fntcmp.py`): system.fnt ascent 48 + descent 12 = 60; newrodin-medium 78 + 18 = 96. The glyph
scale is `size / (ascent + descent)` in both the original (`0x71000b4980`: `+0x70 / (asc+desc) * +0xac`, then
`* +0x74` horizontal) and shin (`message_text_layouter/mod.rs:271`), so the formula is right; only the advances
differ. For `- First twilight` at size 32: system 252.3 px, medium 257.3 px (+2.0 %); for `"A great tumult
followed.` 447.5 vs 454.3 (+1.5 %). Measured in `check_text_compare2.png`: 274 vs 279 px (+1.8 %) and 486 vs 491 px
(+1.0 %, ink not advance). That is the whole difference: no rounding, no horizontal-scale difference (TextObject
passes horizontal scale 1.0, `0x7100101360`), and the glyph shapes in the screenshot also differ slightly (the dash).

**Second difference found in the same screenshot: justification.** `TextObject::layout` (`0x7100101360`) builds the
layouter params with alignment **0 = Justify** hard-coded (the low word of `local_cc` is 0); shin's Check and
SelectLayer use `MessageTextLayout::Left`. In Eden every soft-wrapped Check line ends at the same right edge (lines
3, 4 and 6 of the screenshot end at the same x) — that is the justification of `0x71000b3f30` (alignment 0, not a
hard break, and `width − line < width × 0.05`, `DAT_710015a704` = 0.05). Fixing only the font would still leave
ragged right edges.

**The leading space** on a wrapped line is a consequence of the width, not a separate bug: the original's
layouter also only eats U+3000 at a line start (`0x71000b3f30`, check against `0x3000`, same as shin). With the
narrower font the space fits at the end of the previous line, as in Eden.

**The message box does not have this error.** Its layouter (`0x71000af1f0`) takes medium + bold, exactly like shin,
and its params (`0x71000b0350`..`0x71000b04ac`) are width 1500, alignment = MSGSET's, padding 0/0, between 4.0,
rubi 20.0, size 50.0, horizontal scale `0x3f783e42` (= shin's 0.9697 bit for bit), kinsoku/always-rubi/soft-breaks
all 1, defaults colour 999, fade 200 — identical to `message_layer/mod.rs:395`. Measured on the existing captures
(original 5760×3240 scaled to 1920, port 1920): capture 301, the two dialogue lines span x 213–1585 / 211–495 in the
original and 213–1586 / 212–496 in the port; capture 420 is identical to the pixel. The name line and the
quotation hanging-indent mixin (`0x71000af5d0`, `0x71000af630`, `0x71000af390`) match `mixins.rs` line by line.

## 1. Table

| Original class / system (address) | shin counterpart | Status | What differs / evidence | Impact | Fix |
|---|---|---|---|---|---|
| Fonts: system.fnt / newrodin-medium / newrodin-bold, loaded at `0x7100065ae8` into `0x71002291d8/e0/e8` | `adv/assets.rs` `AdvFonts` (all three loaded) | matches (loading) | same three files, same ROM | none | – |
| `shin::TextObject` font choice (`0x71001011d0` setText takes one font; `0x71000b3c70` stores it as **both** regular and bold, `+0x48`/`+0x50`) | `layer/text_block.rs` (`TextBlock::new`) | **diverges** | always medium + bold; the original passes one font: system.fnt for Check (`0x7100094480`), banners etc.; medium for SelectLayer (also for its bold) | **high** for the Check screen (wrong font, 1–2 % wider, different line breaks); low for choices (only `@{` bold differs) | S |
| `TextObject::layout` alignment (`0x7100101360`, hard-coded 0 = Justify) | `check.rs:413`, `select_layer.rs:155` use `Left` | **diverges** | soft-wrapped lines are not justified in the port | **medium** (Check text ragged instead of flush right; wrapped choices) | S |
| `TextObject::layout` quad overdraw = outline (3 px for mode 1; `scale·bearing − outline`, `+ 2·outline`) | `TextBlock` uses `layout::Char::quad`, fixed 2 px overdraw | diverges | the outline shader samples 3 px out but the quad stops 2 px out, so the outer pixel of the 3 px outline is cut at the glyph box edges | low (slightly thinner outline at glyph extremes on Check/choices) | S |
| `TextObject` defaults (`0x7100101360`: colour 999, speed 50, fade 200) | `TextBlock` 999/100/0 | matches in effect | text object shows fully, so speed/fade unused | none | – |
| `TextObject` position = pad + char position (no left-trim) | `TextBlock` subtracts the min x (`left`) | diverges (harmless today) | identical while every line starts at x 0; differs for centred text | none/low | S |
| `MessageTextParser` (`0x71000b76d0`) | `shin-core/src/layout/parser.rs` | matches except one | same command letters `+ - / < > [ ] a b c e k o r s t v w x y z { \| }`, same `$hex`/decimal int parsing; **`@u<code>.` is lowercase in the original (case 0x75), shin parses `@U`** | none in Umineko (`@u`/`@U` never appear in the English `main.snr`; counts there: @r 69216, @v 36900, @k 26175, @y 2083, `@\|` 2072, @w 428, @c 286, @[ 123, rubi 23, @x 0, @/ 0) | S |
| `MessageTextLayouter` base (`0x71000b3e60` ctor, on_char `0x71000b4980`, on_newline `0x71000b4d90`, finalize `0x71000b3f30`) | `message_text_layouter/mod.rs` | matches | glyph scale formula, kinsoku tables (56 and 14 entries at `0x7100173054`/`0x71001730c4`), punctuation delays (。×4, 、×2 of size·speed), 5 % overflow squish, 5 % justify, U+3000 eating, rubi reflow; one order difference: the original clears both kinsoku flags *after* the rubi rule when kinsoku is off, shin before (rubi lines keep the no-break) | none (kinsoku is always on) | S |
| `MessageLayer::Layouter` (ctor `0x71000af1f0`, slots 4/5/29 above) | `mixins.rs` `MessageLayerLayouterMixin` | matches | name line at scale 90, centred in 360 px, `y −= rubi`, `pos.y = y − rubi − between + 20`; quotation indent for 「（『 | none | – |
| MessageLayer layout params / fonts (`0x71000b0350`) | `message_layer/mod.rs:395` | matches | see §0; screenshot-confirmed | none | – |
| MessageLayer text speed default: `type == 5 ? 100 : settings[+0xb9]` (`0x71000b049c`–`0x71000b04ac`, v9 default 50) | `mod.rs:413` hard-codes 80 | **diverges** | `(100 − s)·0.0001·0.25` s/px: 50 → 0.00125, 80 → 0.0005, so the port types text **2.5× faster** than the original's default | **high** (every line) | S |
| MessageLayer auto-advance delay `delay · (100 − settings[+0xba]) · 0.01` (`0x71000b1ff0`, v10 default 50) | `mod.rs:789` hard-codes 80 | diverges | auto waits 2.5× shorter than default (but auto mode itself is never triggered, below) | low today | S |
| Voice volume 0 stops the voice (`settings[+0xb2]`, `0x71000b1ff0`), v6 keeps the voice across a click | `mod.rs:292`, `:608`, `:715` hard-coded 90 / false / TODO | stub | no settings handle in MessageLayer | low until voices play | S |
| Message window opacity v13 (default 85) (`MessageLayer::vfunc_9` `0x71000b2890`) | `messagebox.rs:72` hard-codes 0.85 | stub (default value right) | config slider would not work | none now | S |
| Voice playback from `@v` (MessageLayer → `VoicePlayer`) | `audio/voice_player.rs`: `play` returns false, `stop`/`get_wait_status` empty; `VOICEPLAY` command only logs TODO | **stub** | **no message voice is ever played** (36,900 `@v` in `main.snr`); auto mode's wait for the voice to finish also never happens | **high** (audible on almost every line) | L |
| Lip-sync hook (flag `ENABLE_CHARACTER_LIPSYNC`, helper `0x7100014eb0` per spec_save §7) | `bustup_layer.rs` `mouth_state` never changes; `layerload.rs:216` TODO | **missing** | mouths are frozen on whatever mouth block 0 is (capture 297: port's Rudolf has the mouth open, the original closed) | **medium–high** (every voiced bustup) | M |
| Click-wait icon (`0x71000b2890`: `sin(t/60·π)·0.4+0.6`, 30×30, `keywait` atlas halves by wait kind, at cursor + (210, y − 30); NoText at (1870, 1030)) | `message_layer/mod.rs:1010` | matches | all constants equal | none | – |
| Message box placement (`0x71000b2890`: types < 4 and 5: `(1 − slide)·64 + (1080 − h) − 32`; Novel: `max(32, (1080 − size.y)·0.35)`) | `mod.rs:997` | matches | constants `0x710015a8e0`=64, `a5c8`=−32, `a40c`=0.35 | none | – |
| Click-to-advance during a voice block (`try_advance`) | `mod.rs:625` `if self.blocks.len() >= self.current_block_index { return true; }` | diverges (bug in shin) | the condition is always true (the function already returned when `index >= len`), so the Voice/VoiceWait/VoiceSync branch after it is dead: a click while waiting on `@/` or `@x` never stops the voice or starts the next segment. Intended condition is surely `current_block_index >= blocks.len()` **inferred** (the original's input handler was not decompiled) | none in Umineko (`@/` and `@x` never appear) | S |
| Auto mode (`autoplay_requested`) | `mod.rs:686` reads it, nothing ever sets it | missing | no auto mode | medium | M |
| Skip (hold) | `adv/mod.rs:212` fast-forwards every frame while held | diverges | skips unread text too (original honours v11 "skip only read", per spec_save §7) — mostly the systems area | medium | M |
| Song-title banner (`ADV+0x2e0`, set up at `0x710009e160`, drawn by `0x710009e590` when `settings[+0xbf]`) | none | **missing** | two TextObjects in system.fnt, 1920 wide, size 36, outline mode 1: the title (looked up by `0x71000eaa00` in the table at `*0x7100226790`) and a `♪` (`0x7100162433`) that bobs up by `max(0, sin(t/40·π))·8` px and pulses its colour by `sin·−0.4+1` / `sin·0.3+1` when negative; alpha from the tweener at `+0x118`, both drawn in an ortho 1920×1080 projection, white text, black outline at the layer alpha | **medium** (seen every time a new BGM starts, Eden-visible) | M |
| AUTOSAVE scene-title banner (`0x7100064240`, draw `0x7100064480` when `settings[+0xbe]`) | `adv/command/autosave.rs:22` TODO | **missing** | TextObject, system.fnt, 1920 wide, size 48, outline mode 1, anchor (?, 1.0) at y 1076 (bottom edge); alpha tween 0 → 1 over 0.03 (flag 0x40), hold 240 (flags 0x1080), → 0 over 0.03 (as decompiled; the 0.03 duration looks odd and should be checked in Eden) | **medium** (every chapter/scene start) | M |
| `MessageBrowser` (backlog) + `Layouter` (`0x71001dc5b8`: overrides slots 1, 4, 5, 21, 29 of the message layouter), `LogBuilder`, `ChapterSelector` (system font) | none (`AdvMessageAction::Backlog` commented out) | **missing** | no backlog. Layout-wise it is a third `MessageTextLayouter` subclass on medium/bold, so it can reuse shin's layouter with a third mixin | medium | L |
| `MessageVoicePlayer::Layouter` (`0x71001dd2e8`), `PlainTextParser` (`0x71001ddd20`) | none | missing | used by backlog voice replay / plain strings **inferred** | low | M |
| `shin-core/src/layout/layouter.rs` | not compiled (not in `layout/mod.rs`) | dead code | 650 lines of an older layouter full of `todo!()` | none | S (delete) |

## 2. Top divergences (ordered by what the player sees and hears)

1. **Message voices never play** (`audio/voice_player.rs` is an empty stub, `VOICEPLAY` logs TODO). The layer side
   (`play_voice`, segments from `@x`, `VoicePlayFlags`) is already wired; what is missing is the player: resolve the
   `@v` spec through the scenario's voice tables, play `voice/*.nxa` on the voice channel at `volume × v2/100`,
   report PLAYING in `get_wait_status`, `stop`. Then fix `mod.rs:625` (see 7). (Probably shared with the audio area.)
2. **Text types 2.5× too fast.** `message_layer/mod.rs:413`: use `settings.v9_msgspeed` (default 50, already in
   `shin-core/src/format/save/mod.rs:331`) instead of 80; keep 100 for `NoText`. Same for `v10_skipspeed` at `:789`,
   `v2` at `:292`, `v6` at `:608`, `v13` in `messagebox.rs:72`. One settings handle passed to `MessageLayer` covers
   all five TODOs. Evidence `0x71000b049c`–`0x71000b04ac`, `0x71000b1ff0`.
3. **Check text uses the wrong font** (the known 1.3 % issue). Give `TextBlock::new` a font argument and lay out
   with the same font for regular and bold (as `0x71000b3c70` does): `fonts.system_font` from
   `murderer/mod.rs:390`, `fonts.medium_font` from `select_layer.rs:172`; load the glyphs from that font. Ascent and
   descent come from the font too, so the baseline moves (system: 32·48/60 = 25.6 vs 26.0).
4. **Check text is not justified.** Set `text_alignment: MessageTextLayout::Justify` in `check::text_layout()` and in
   the SelectLayer params (the TextObject hard-codes 0, `0x7100101360`). With 3 and 4 the Check screen should match
   Eden line for line.
5. **Lip-sync missing**: mouths never animate (`bustup_layer.rs` `mouth_state` constant, `layerload.rs:216`). Needs the
   voice player's amplitude per character (`lipsync_character_ids` of the voice mapping) driving `mouth_state`; the
   original helper is `0x7100014eb0` (not decompiled here).
6. **Song-title and scene-title banners missing** (constants in the table above). Both are plain `TextBlock`s in
   system.fnt with a 3 px outline; the song title needs BGMPLAY to start it and the `♪` bob; AUTOSAVE needs its
   level-1 SAVEINFO text. Gate them on v15/v14.
7. **Dead voice branch in `try_advance`** (`mod.rs:625`, harmless for Umineko since `@/` and `@x` are unused): change to `if self.current_block_index >= self.blocks.len()`.
8. **TextObject outline clipped by 1 px**: give `TextBlock` quads an overdraw equal to the outline (3) instead of
   `Char::quad`'s fixed 2 (`0x7100101360` vertex code: `scale·bearing − outline`, size `+ 2·outline`).
9. Auto mode: nothing sets `autoplay_requested`; add the toggle (systems area for the input, this layer is ready).

## 3. Structural

- **TextObject should be one shared type with the original's parameters.** The original has one `TextObject`
  (`0x7100100ea0`) used by ~15 screens with a param block `(width, size, line spacing, outline mode, soft-breaks,
  always-rubi, padding above, padding below)` and one font. shin's `TextBlock::new` takes shin's full
  `LayoutParams` plus a separate outline, so every caller re-derives width − 2·pad, alignment, rubi size and
  horizontal scale by hand — which is exactly how today's two Check bugs (rubi space/padding, fixed earlier) and the
  two found here (font, alignment) happened. Proposal: `TextBlock::new(ctx, font: &Arc<GpuFontLazy>, text,
  TextObjectParams { width, size, line_spacing, outline_mode, soft_breaks, always_rubi, pad_above, pad_below })`
  that builds `LayoutParams` the way `0x7100101360` does (alignment Justify, rubi 0 → 0.4·size, h-scale 1.0,
  kinsoku on, width − pad where pad = 6 for mode 1 / 3 for mode 2, outline 3 / 1.5 / none) and sizes the quads by
  the outline. The banners, save menu, manual and so on will then come out right by construction.
- **Settings reach no layer.** `MessageLayer` has five hard-coded settings values; the save format already
  decodes them. Pass a read-only settings handle (or a small `MessageSettings` copy updated each frame) the way the
  original reads `*0x71002264f0 + 0xb0..`.
- **Backlog**: when it is built, model it as the original does: a `MessageBrowser::Layouter` third mixin on the
  existing `MessageTextLayouterImpl` (it overrides the same slots 4/5/29 as the message layer's layouter), not a
  separate layout path.

## 4. Open questions

- AUTOSAVE banner fade: `0x71000bb960(1.0, 0.03, …, 0x40)` reads as a 0.03 s fade, which seems too short; confirm in
  Eden or re-read the float argument registers in the disassembly of `0x7100064240`.
- What the extra tween flag `0x1000` in the banner's hold step (`0x1080`) means.
- The original's click handler for MessageLayer (where it handles a click during `@/` and `@x`) was not decompiled;
  the fix in item 7 is inferred from shin's own control flow.
- `0x7100107960` (system.fnt TextObject, width 1222, size 48) and `0x71000223d0` were not identified (likely tips /
  characters / system menu screens, outside the ADV).
- Message-layer quad overdraw (shin: 2 px) and the v8 outline variant (`MessageLayer` checks `settings[+0xb8] == 1`
  and uses `_UNK_710015a5a0` for the outline distance) not compared.
