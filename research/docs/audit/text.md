# Audit: text (message window, parser, layouter, fonts, TextObject)

Status checked against the code on 2026-11-07.

The project-wide status and the measured accuracy are in [alignment-plan.md](../alignment-plan.md).

Logs and decompiles: `$S\audit\text\` (dec1–dec6.c, fontload.txt, setmsg_pre.txt, fntcmp.py, gotscan.py, fonts\*.fnt).
All original addresses are from the v1.0.3 `main`.

## 0. The Check text was 1–2 % wider than the original (fixed: it now matches Eden)

**Root cause: wrong font.** The engine loads three fonts at boot (`0x7100065ae8`..`0x7100065cb0`, the adrp/add
pairs found by scanning the code):

| file | stored in (GOT → global) | used by |
|---|---|---|
| `/system.fnt` | `0x71002267c8` → `0x71002291d8` | every `TextObject` outside the choice menu: the culprit game's Check text (`0x7100094480`), the song-title banner (`0x710009e160`), the AUTOSAVE scene-title banner (`0x7100064240`), save menu (`0x71000d6640`), backlog chapter selector (`0x71000a96d0`), manual, CG mode, and others (`gotscan.py` lists 18 sites) |
| `/newrodin-medium.fnt` | `0x71002268b8` → `0x71002291e0` | message box regular (`0x71000af208`), backlog (`0x71000a3524`, `0x71000a5b24`, `0x71000af208`), **SelectLayer** (`0x71000f027c`) |
| `/newrodin-bold.fnt` | `0x71002268c0` → `0x71002291e8` | message box bold, backlog bold |

So `*0x71002267c8` is **system.fnt**, a different font from newrodin-medium/bold. `TextBlock::new`
(`shin/src/layer/text_block.rs`) now takes one font and uses it for regular and bold, and the Check screen passes
`system_font`.

Metrics (from the ROM, `fntcmp.py`): system.fnt ascent 48 + descent 12 = 60; newrodin-medium 78 + 18 = 96. The glyph
scale is `size / (ascent + descent)` in both the original (`0x71000b4980`: `+0x70 / (asc+desc) * +0xac`, then
`* +0x74` horizontal) and shin (`message_text_layouter/mod.rs`, `on_char`), so the formula is right; only the advances
differ. For `- First twilight` at size 32: system 252.3 px, medium 257.3 px (+2.0 %); for `"A great tumult
followed.` 447.5 vs 454.3 (+1.5 %). Measured in `check_text_compare2.png`: 274 vs 279 px (+1.8 %) and 486 vs 491 px
(+1.0 %, ink not advance). That was the whole difference: no rounding, no horizontal-scale difference (TextObject
passes horizontal scale 1.0, `0x7100101360`), and the glyph shapes in the screenshot also differ slightly (the dash).

**Second difference found in the same screenshot: justification.** `TextObject::layout` (`0x7100101360`) builds the
layouter params with alignment **0 = Justify** hard-coded (the low word of `local_cc` is 0). In Eden every
soft-wrapped Check line ends at the same right edge (lines 3, 4 and 6 of the screenshot end at the same x): the
justification of `0x71000b3f30` (alignment 0, not a hard break, and `width − line < width × 0.05`, `DAT_710015a704` =
0.05). The Check text (`check::text_layout`), SelectLayer and every other `TextBlock` caller now use `Justify`.

**The leading space** on a wrapped line was a consequence of the width, not a separate bug: the original's
layouter also only eats U+3000 at a line start (`0x71000b3f30`, check against `0x3000`, same as shin). With the
narrower font the space fits at the end of the previous line, as in Eden.

**The message box never had this error.** Its layouter (`0x71000af1f0`) takes medium + bold, exactly like shin,
and its params (`0x71000b0350`..`0x71000b04ac`) are width 1500, alignment = MSGSET's, padding 0/0, between 4.0,
rubi 20.0, size 50.0, horizontal scale `0x3f783e42` (= shin's 0.9697 bit for bit), kinsoku/always-rubi/soft-breaks
all 1, defaults colour 999, fade 200, identical to `MessageLayer::set_message` (`message_layer/mod.rs`). Measured on
the existing captures (original 5760×3240 scaled to 1920, port 1920): capture 301, the two dialogue lines span
x 213–1585 / 211–495 in the original and 213–1586 / 212–496 in the port; capture 420 is identical to the pixel. The
name line and the quotation hanging-indent mixin (`0x71000af5d0`, `0x71000af630`, `0x71000af390`) match `mixins.rs`
line by line.

## 1. Table

| Original class / system (address) | shin counterpart | Status | What differs / evidence | Impact | Fix |
|---|---|---|---|---|---|
| Fonts: system.fnt / newrodin-medium / newrodin-bold, loaded at `0x7100065ae8` into `0x71002291d8/e0/e8` | `adv/assets.rs` `AdvFonts` (all three loaded) | matches (loading) | same three files, same ROM | none | – |
| `shin::TextObject` font choice (`0x71001011d0` setText takes one font; `0x71000b3c70` stores it as **both** regular and bold, `+0x48`/`+0x50`) | `layer/text_block.rs` (`TextBlock::new(ctx, font, …)`) | matches | one font for regular and bold; Check, banners, dialog, tips, chapter list pass `system_font`, SelectLayer passes `medium_font` | none | – |
| `TextObject::layout` alignment (`0x7100101360`, hard-coded 0 = Justify) | `check.rs` `text_layout`, `select_layer.rs`, `hud.rs` `text_layout` | matches | every `TextBlock` caller passes `Justify` | none | – |
| `TextObject::layout` quad overdraw = outline (3 px for mode 1; `scale·bearing − outline`, `+ 2·outline`) | `TextBlock` uses `Char::quad_with_overdraw(line, outline)` | matches | quads grow by the outline width, so the 3 px outline is no longer cut | none | – |
| `TextObject` defaults (`0x7100101360`: colour 999, speed 50, fade 200) | `TextBlock` 999/100/0 | matches in effect | text object shows fully, so speed/fade unused | none | – |
| `TextObject` position = pad + char position (no left-trim) | `TextBlock` subtracts the min x (`left`) | diverges (harmless today) | identical while every line starts at x 0; would differ for centred or right-aligned text, which no caller uses | none | S |
| `MessageTextParser` (`0x71000b76d0`) | `shin-core/src/layout/parser.rs` | matches | same command letters `+ - / < > [ ] a b c e k o r s t v w x y z { \| }`, same `$hex`/decimal int parsing; `@u<code>.` lowercase as the original (case 0x75) | none in Umineko (`@u`/`@U` never appear in the English `main.snr`; counts there: @r 69216, @v 36900, @k 26175, @y 2083, `@\|` 2072, @w 428, @c 286, @[ 123, rubi 23, @x 0, @/ 0) | S |
| `MessageTextLayouter` base (`0x71000b3e60` ctor, on_char `0x71000b4980`, on_newline `0x71000b4d90`, finalize `0x71000b3f30`) | `message_text_layouter/mod.rs` | matches | glyph scale formula, kinsoku tables (56 and 14 entries at `0x7100173054`/`0x71001730c4`), punctuation delays (。×4, 、×2 of size·speed), 5 % overflow squish, 5 % justify, U+3000 eating, rubi reflow; lines break between any two letters like the original. One order difference: the original clears both kinsoku flags *after* the rubi rule when kinsoku is off, shin before (rubi lines keep the no-break). Port-only: `LayoutParams::word_wrap` keeps Latin words whole; only the HUD texts (`adv/hud.rs`) turn it on | none (kinsoku is always on); the HUD banners may wrap at other points than the original | S |
| `MessageLayer::Layouter` (ctor `0x71000af1f0`, slots 4/5/29 above) | `mixins.rs` `MessageLayerLayouterMixin` | matches | name line at scale 90, centred in 360 px, `y −= rubi`, `pos.y = y − rubi − between + 20`; quotation indent for 「（『 | none | – |
| MessageLayer layout params / fonts (`0x71000b0350`) | `message_layer/mod.rs` `set_message` | matches | see §0; screenshot-confirmed | none | – |
| MessageLayer text speed default: `type == 5 ? 100 : settings[+0xb9]` (`0x71000b049c`–`0x71000b04ac`, v9 default 50) | `message_layer/mod.rs` `set_message` | matches | uses `settings.v9_msgspeed`, 100 for `NoText`; `(100 − s)·0.0001·0.25` s/px | none | – |
| MessageLayer auto-advance delay `delay · (100 − settings[+0xba]) · 0.01` (`0x71000b1ff0`, v10 default 50) | `message_layer/mod.rs` update | matches | uses `settings.v10_skipspeed` | none | – |
| Voice volume 0 stops the voice (`settings[+0xb2]`, `0x71000b1ff0`), v6 keeps the voice across a click | `message_layer/mod.rs` (`set_settings`, `try_advance`, update) | matches | `v2_voicevol == 0` skips/stops the voice, `v6` keeps it past a click; settings arrive each frame from `AdvState::apply_settings` | none | – |
| Message window opacity v13 (default 85) (`MessageLayer::vfunc_9` `0x71000b2890`) | `messagebox.rs` | matches | reads `settings.v13_msgwinalpha` | none | – |
| Voice playback from `@v` (MessageLayer → `VoicePlayer`) | `audio/voice_player.rs`, `adv/command/voiceplay.rs` | matches except segments | voices play through the scenario's voice mapping, `get_wait_status` reports PLAYING, `stop` works; `@x` segments are played whole (logged) | none in Umineko (`@x` never appears) | M |
| Lip-sync hook (flag `ENABLE_CHARACTER_LIPSYNC`, helper `0x7100014eb0` per spec_save §7) | `voice_player.rs` `lipsync_level` (window levels `0x71000f55d0`), `bustup_layer.rs` update (`0x7100036a00`) | matches | mouth level 0–2 per 50 ms window with the original's thresholds drives `mouth_state` while property 27 is set (a stale TODO remains in `layerload.rs`) | none | – |
| Click-wait icon (`0x71000b2890`: `sin(t/60·π)·0.4+0.6`, 30×30, `keywait` atlas halves by wait kind, at cursor + (210, y − 30); NoText at (1870, 1030)) | `message_layer/mod.rs` `render` | matches | all constants equal | none | – |
| Message box placement (`0x71000b2890`: types < 4 and 5: `(1 − slide)·64 + (1080 − h) − 32`; Novel: `max(32, (1080 − size.y)·0.35)`) | `message_layer/mod.rs` `render` | matches | constants `0x710015a8e0`=64, `a5c8`=−32, `a40c`=0.35 | none | – |
| Click-to-advance during a voice block (`try_advance`) | `message_layer/mod.rs` `try_advance` | matches | Past the wait checks, a click stops (or, at a voice sync, continues) the voice when one plays, as `0x71000b1b10` (`+0x6c`) | none | - |
| Auto mode (`isAuto` `0x71000122f0`) | `adv/mod.rs` `is_auto` → `request_autoplay` each frame | matches | L / touch button toggles auto; the message layer advances after the voice or the auto delay | none | – |
| Skip (`isSkipping` `0x7100012430`) | `adv/mod.rs` `is_fast_forwarding` | matches | honours v11 "skip only read" (`save.line_read`); one step per 60 Hz frame | none | – |
| Song-title banner (`ADV+0x2e0`, set up at `0x710009e160`, drawn by `0x710009e590` when `settings[+0xbf]`) | `adv/hud.rs` `show_song_title` | matches | two TextObjects in system.fnt, 1920 wide, size 36, outline mode 1: the title (looked up by `0x71000eaa00` in the table at `*0x7100226790`) and a `♪` (`0x7100162433`) that bobs up by `max(0, sin(t/40·π))·8` px and pulses its colour by `sin·−0.4+1` / `sin·0.3+1` when negative; the tweener at `+0x118` is the banner's y (it slides down from above the screen), drawn in an ortho 1920×1080 projection, white text, black outline; gated on v15 | none | – |
| AUTOSAVE scene-title banner (`0x7100064240`, draw `0x7100064480` when `settings[+0xbe]`) | `adv/hud.rs` `show_scene_title`, called from `adv/command/autosave.rs` | matches (fade unverified) | TextObject, system.fnt, 1920 wide, size 48, outline mode 1, anchor (?, 1.0) at y 1076 (bottom edge); alpha 0 → 1 at 0.03 per frame (flag 0x40), hold 240 (flags 0x1080), → 0 at 0.03; the port reads 0.03 as a per-frame rate (about 33 frames), not confirmed in Eden; gated on v14 | low | – |
| `MessageBrowser` (backlog) + `Layouter` (`0x71001dc5b8`: overrides slots 1, 4, 5, 21, 29 of the message layouter), `LogBuilder`, `ChapterSelector` (system font) | `adv/menu/backlog.rs`, `adv/menu/chapter_list.rs` | diverges | backlog is in (width 1480, size 40, h-scale 0.9697, `0x71000a3510`); chapter list uses system.fnt. But the original lays out in medium **and bold** (`0x71000a3510` loads `0x71002291e0` and `0x71002291e8`), the port passes `medium_font` for both, so `@{` text is not bold. The port uses the plain layouter and writes the name as `【name】` into the text instead of the subclass's slot overrides; whether that equals slots 1/4/5/21/29 is unverified | low | S |
| `MessageVoicePlayer::Layouter` (`0x71001dd2e8`), `PlainTextParser` (`0x71001ddd20`) | none as classes; the backlog collects a line's `@v` specs with `MessageTextParser` | unverified | used by backlog voice replay / plain strings **inferred**; the port's replay plays every `@v` of the line, whether the original does the same is unverified | low | – |

## 2. Remaining divergences (ordered by what the player sees)

1. **Backlog bold**: `adv/menu/backlog.rs` passes `medium_font` to `TextBlock`, which uses one font for both styles;
   the original's backlog layouter takes medium + bold (`0x71000a3510`). Needs a two-font `TextBlock` path or the
   backlog laying out through `MessageTextLayouter` with `bold_font`.
2. **`TextBlock` left-trim**: harmless while every caller justifies.

## 3. Structural

- **TextObject parameters.** The original has one `TextObject` (`0x7100100ea0`) used by ~15 screens with a param
  block `(width, size, line spacing, outline mode, soft-breaks, always-rubi, padding above, padding below)` and one
  font. shin's `TextBlock::new` now takes one font, but still takes a full `LayoutParams` plus a separate outline, so
  callers build the params by hand (`hud::text_layout` is shared by some). The rules `0x7100101360` applies:
  alignment Justify, rubi 0 → 0.4·size, h-scale 1.0, kinsoku on, width − pad where pad = 6 for mode 1 / 3 for
  mode 2, outline 3 / 1.5 / none.
- **Backlog**: the original models it as a `MessageBrowser::Layouter` third subclass of the message layouter (slots
  4/5/29 like the message layer's). The port uses the plain layouter (see the table).

## 4. Open questions

- AUTOSAVE banner fade: `0x71000bb960(1.0, 0.03, …, 0x40)`; the port reads 0.03 as a per-frame step (about 33
  frames). Confirm in Eden or re-read the float argument registers in the disassembly of `0x7100064240`.
- Tween flag `0x1000` (the banner's hold step `0x1080`): per `0x71000bb470`, skipping stops at that step until it
  has played (`hud.rs` `Step::stop`).
- The original's click handler for MessageLayer (where it handles a click during `@/` and `@x`) was not decompiled;
  the fix in §2 item 2 is inferred from shin's own control flow.
- `0x7100107960` is the tips screen's cursor tip (title 48, text 32, width 1222; `adv/menu/tips.rs`) and
  `0x71000223d0` the shared Yes/No dialog (`adv/menu/dialog.rs`).
- Message-layer quad overdraw (shin: 2 px) and the v8 outline variant (`MessageLayer` checks `settings[+0xb8] == 1`
  and uses `_UNK_710015a5a0` for the outline distance) not compared; the port does not support v8 (TODO in
  `message_layer/layout.rs`).
