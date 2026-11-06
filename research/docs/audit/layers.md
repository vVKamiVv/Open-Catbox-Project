# Audit: layers, layer properties and effects, wipers, render programs

> **Snapshot.** This report describes shin as it stood at the November 2026 audit; its Status and shin columns
> are kept as written so the evidence stays readable. Since then: the bustups' lip-sync and blinking (D1), the transparent container clears, the double transform, the
> layer sort and wobble slots 44-55 are fixed; the whirl and glass wipers, blur, ripple, mosaic, dissolve and raster
> were compared with the original on screen and match. Current status of every finding:
> [alignment-plan.md](../alignment-plan.md), which also lists what was compared with the original and the port's
> measured accuracy.

Logs, decompiles and extracted shaders: `$S\audit\layers\` (`d1.c`–`d9.c`, `sh\program_*_fs.glsl`, `prop_counts.txt`,
`prop_values.txt`). All addresses are from the v1.0.3 `main` (Ghidra base `0x7100000000`).

## How Umineko uses layer properties (English `main.snr`, 14,300 LAYERCTRL)

| Property | Uses | Main values / targets | shin |
|---|---|---|---|
| 5 RenderPosition | 4,875 | `-layer id` (-22, -5, -17, ...) | sort key, tie-break wrong (D3) |
| 68/69 dissolve | 2,374 / 240 | bustups 1,326, plane `-4` 415 (407 on plane 1), pictures 249 | implemented |
| 0, 1, 3, 4 translate | 1,202 / 681 / 82 / 184 | | ok |
| 9 alpha | 1,118 | 0, 1000, 588, 500, ... | ok |
| 27 | 363 | 0 on bustups (154), rain (166) | rain only; **bustups ignore it** (D1) |
| 12/13 scale | 336 / 348 | | ok |
| 56–60 ghosting | 289 / 108 / 28 / 20 / 14 | 268 on the screen layer `-2` | ok, but see D2 |
| 23 blend type | 251 | always 1 (additive), 238 on movies | ok |
| 32–39 wobble X/Y | ~580 | mode 4 (sine), 1 (random); 76+76 on screen `-2` (screen shake) | mostly ok (D5) |
| 66 blur | 86 | | ok |
| 82–84 ripple | 84 / 41 / 41 | 78 on screen `-2` | ok |
| 22 show | 71 | | ok |
| 24 fragment shader | 69 | 1 (mono) x64 with params 28–30 = (234, 164, 429); 4 (negative) x4 | ok (negative param, D7) |
| 26 flip | 62 | | ok |
| 73–75 rain | 55 / 16 / 4 | | ok (doc §6) |
| 67 mosaic, 76–81 raster, 40–43 wobble alpha | 14, 8–4, 2 | | ok |
| 2, 25, 44–55, 61–65, 70–72, 85–89 | **0** | never set by the script | some are panics/divergences, harmless for Umineko |

Other counts: LAYERLOAD flags only 0, 2 (keep properties) and 3 (rain); LAYERUNLOAD delay always 0; MASKLOAD 169 (flags
16/17/19); WIPE mask type 29 (flags 0 or 4); TRANSSET/TRANSWAIT/LAYERSWAP/MASKUNLOAD 0.

## 1. Class/system table

| Original (address) | shin counterpart | Status | What differs / evidence | Player impact | Fix |
|---|---|---|---|---|---|
| `Layer::Properties` tweener block, `getValue` `0x7100074930` | `layer/properties.rs` | matches | 90 tweeners at `+0x28 + id*0x58`; blend type clamped `<3` else 0, fragment shader `<6` else 0 (`0x7100075530`), shader params = props 28–31 × 0.001 | none | – |
| Colour multiplier `0x7100074b30` | `get_color_multiplier` | matches | rgb × 0.0005 (`0x710015a938`), a = p9 × 0.001 × alpha-wobble (42/43), clamp [0,1] (`0x7100056710`), × (2,2,2,1) (disassembly) | none | – |
| Visibility `0x7100074cb0` | `is_visible` | matches | p22, int(p12), int(p13) ≠ 0 and alpha × wobble > 0 | none | – |
| Transform `0x7100074d80` | `get_transform` | matches except wobble slots | same order T(−scale origin)·S·T(so−ro)·R·T(t+ro); flip p26 bits 1/2; translation p0+p3−p20, p1+p4−p21, z p2 | none (see next row) | – |
| Wobbler slots `+0x1f18..+0x1f68`, update `0x71000c7270` | `properties.rs` wobblers | **diverges** | original: props 44–47 drive **rotation**, 48–51 **scale X**, 52–55 **scale Y** (transform reads amp/bias 46/47 for rotation, 50/51 for x, 54/55 for y). shin: 44→scale X, 48→scale Y, 52→rotation | none (44–55 never used) | S |
| Wobbler update `0x71000c7270` | `wobbler/mod.rs::update` | **diverges** | original resets the phase only when the **mode** changes (mode ≥ 9 → 0) and runs for any non-zero period (negative runs backwards); shin also resets on **period** change and only runs for period > 0 | low (a jump if a script tweens a shake period) | S |
| Wobbler value `0x71000c7370` | `Wobbler::value` | matches (modes 2–8); random mode inferred | random: mt19937 seeded `int(t)+seed` (0 → `0x12345678`) and `+1`, lerped by frac(t), 0 for the first period; seed is wobbler `+0`, source unknown; shin seed 0 (`TODO`) | low (13 random-mode uses) | S |
| Blend states (`setBlend` `0x71000cf700`, table filled by `0x71000ce600`) | `pipelines/conversions.rs` | matches except id 4 | ids: 0 no colour, 1 opaque, 2/3/4 straight normal/add/sub, 5/6/7 premultiplied normal/add/sub; alpha channel always (1−dst α, 1). Id 4 is NVN **reverse-subtract** (dst − src·α); shin `Layer3` uses `Subtract` (src·α − dst). `LayerPremultiplied3` is correct | none (blend type 2 never used) | S |
| Blend lookup tables `0x7100226840` = {2,3,4}, `0x7100226d58` → {5,6,7} | `from_regular_layer` / `from_premultiplied_layer` | matches | tile and movie direct draws use 2–4, picture blocks and effect composites 5–7 | none | – |
| Program tables by fragment shader: blocks `0x7100170dc0` {20,23,21,22,24,25}, composite-transparent `0x7100170dd8` {14,17,15,16,18,19}, composite-opaque/movie `0x7100170da8` {8,11,9,10,12,13} | `layer.wgsl` output kinds `LayerPremultiply` / `LayerDiscard` / `Layer` | matches | decompiled 8–25: 20–25 premultiply output, 14–19 discard below `c1.x` without premultiply, 8–13 plain | none | – |
| Fragment shaders (programs 8–13) | `utils.wgsl::evaluate_fragment_shader` | matches except negative | default `tex×c`; mono `mix(c, luma, w)×xyz` (0.299, 0.587, 0.114); fill `mix(c, xyz, w)`; fill2 = **same code as default**; gamma `exp2(log2(c)/xyz)`. **Negative (program 12) is `mix(c, 1−c, w) × xyz`**; shin drops `× xyz` | none today (4 uses, default xyz) | S |
| CPU fragment shader for tiles `0x71000735a0` | `LayerFragmentShader::evaluate` | **stub** | shin `todo!()` for fill, fill2, negative, gamma (panics), mono ignores `w` | none (no tile uses a shader) | S |
| `PictureBlockParams::setup` `0x7100072230` | `picture_layer.rs::setup` | matches | same pass split and shader simplification as shin's `is_equivalent_to_default` | none | – |
| `PictureLayer::renderDirect/renderIndirect` `0x71000c5fa0` / `0x71000c6040` | `picture_layer.rs`, `NewDrawableLayer::render_drawable_indirect` | matches, 1 shortcut | indirect always returns the transparent pass (`coversScreen` `0x7100072c80` not ported, already marked `shortcut:`); block culling `TODO` | none | S |
| Effect chain `NewDrawableLayer::preRender` `0x71000762e0` | `new_drawable_layer/mod.rs`, `effect_passes.rs` | matches (doc §11) | blur, mosaic, raster, ripple, dissolve, ghosting confirmed; ghosting transform `0x7100075220` = shin's | none | – |
| Prop 70 effect (program `0x2c`) | `pre_render` | **stub** | `todo!()` → panic if ever > 0 | none (never set) | M |
| Dissolve seed (layer `+0x58`) | `NewDrawableLayerState::dissolve_seed` | inferred | shin seeds from the clock; source in the original still open | low | S |
| Effect composite `0x7100078af0` | `try_finish_indirect_render` | matches | transparent pass programs 14–19 + blends 5–7, opaque pass 8–13 + blend 1; colour premultiplied (inferred from `0x71000566f0` multiply) | none | – |
| Clip mode props 61–65 (read in `0x71000788e0`) | `assert_eq!(clip mode, None)` in 4 places | stub | would panic | none (never set) | M |
| `LayerGroup` render `0x710007d5e0` | `layer_group.rs::render` | matches | opaque pass children back to front, then fill quad (program 0, stencil greater), transparent pass front to back | none | – |
| `LayerGroup` child sort, comparator `0x7100081110` | `layer_group.rs::pre_render` sort | **diverges** | higher RenderPosition first (|Δ| ≥ ε); **ties: lower layer id first** (key = Properties `+0x20`, set by LAYERLOAD from the record field `+0x58` = layer id). shin ties: **higher layerbank id first** | medium: layers left at the default RenderPosition stack in the wrong order (layerbanks are allocated, not ordered by id) | S |
| `LayerGroup` indirect `vfunc_16` `0x710007e2c0`, fill colour `0x7100226c88` | `LayerGroupNewDrawableDelegate::render_drawable_indirect`, `render` opaque clear | **diverges** | original clears the effect texture and fills uncovered pixels with **(0,0,0,0)**; shin uses opaque black (`0xff000000`) | medium: while plane 1 dissolves (407 uses), areas it doesn't cover turn black instead of showing plane 0 | S |
| `LayerGroup` delayed removal (`vfunc_3` `0x710007bc00`) | `remove_layer` | stub (`unimplemented!`) | | none (delay always 0) | M |
| `LayerGroup::TransitionLayer`, LAYERLOAD auto-wipe (flag 4) | none | missing | | none (flag never used) | L |
| LayerGroup mask (MASKLOAD) `0x710007df80`, `0x710007d8d0` | `prepare_mask_*`, `finish_render_with_mask` | matches (structure, flag bits) | flip 1/2, invert 4, scale **0x10** in both; mask programs `0x7100226da8` not compared | unknown (169 uses; the mask wiper has since been seen to match Eden in the scenario select's curtain wipe) | – |
| `PageLayer` `vfunc_8` `0x710009f360`, `vfunc_16` `0x710009fc50` | `page_layer.rs` | **diverges** | plane occlusion and stencil bump match; (a) shin passes the page's **own** transform into the effect chain, whose indirect render composes it again (original passes the parent); (b) clear colour (0,0,0,0) vs opaque black | low (page effects: 2 blur, 2 ghosting, 2 scale) | S |
| `ScreenLayer` `vfunc_8` `0x71000e67f0`, `vfunc_16` `0x71000e6b70` | `screen_layer.rs` | **diverges** | (a) same double transform: while ghosting/ripple/blur/raster is on the screen layer, its wobble shake, scale and translation are applied **twice**; (b) indirect returns opaque (1), shin transparent; (c) background colour (0,0,0,0) vs black | **high**: screen shake (76 wobble uses on `-2`) combined with screen ghosting (268) or ripple (78) shakes/zooms twice as far | S |
| `ScreenLayer::TransitionLayer` + PAGEBACK | `TransitionLayer`, `pageback` | matches; stub branch | `pageback(start_anew = true)` is `todo!()` (never called with true) | none | – |
| Back layer group (snapshot while animations are blocked) | `create_back_layer_group_if_needed` | **stub** (inferred semantics) | no-op; the original renders a snapshot instead of half-applied changes | low (possible one-frame half-built scenes) | M |
| `BustupLayer::update` `0x7100036a00` | `bustup_layer.rs` | **missing** | prop 27 = 0 freezes mouth (frame 0) and resets blink; otherwise mouth = voice level × mouth count / 3 (clamped) from a listener, and blinking: timer += dt, after a wait of `(1 − r²)·240 + 80` ticks (`0x710015a640`, `0x710015aa48`; r from minstd_rand 48271, × 2⁻³¹) the eye frames play over 20 ticks. shin: `mouth_state`/`eyes_state` stay 0, prop 27 ignored, lip-sync `TODO` in LAYERLOAD | **high**: no character ever blinks or moves lips | M |
| `MovieLayer` `0x71000c3560` / `0x71000c38c0` | `movie_layer.rs` | diverges (movie area) | alpha (`Transparent`) movies and repeat only `warn!`; direct path ignores colour/blend but the separate-pass test covers that | medium if Umineko has alpha movies (not counted) | M |
| `TileLayer` `0x7100106d00` / `0x7100106ed0` | `tile_layer.rs` | matches | program 1, straight blends 2–4, colour run through the CPU fragment shader | none | – |
| `RainLayer` | `rain_layer.rs` | matches (doc §6) | | none | – |
| `AnimationLayer`, `SakuraAnimationLayer`, `EffectLayer`, `FocusLineLayer` | none | missing | never loaded (layer types 4, 5, 7) | none | L |
| `TimedWiper` update `0x710011baa0` | `wiper/timed.rs` | matches | elapsed += dt while animations run, clamped | none | – |
| `DefaultWiper` `0x7100062660` / program `0x24` | `wiper/default.rs` | matches (doc §7) | | none | – |
| `MaskWiper` render `0x71000a1700` / program `0x25` | `wiper/mask.rs`, `wiper_mask.wgsl` | matches; flag bit differs | thresholds incl. invert identical; shader `mix(new, old, clamp(min + mask.r·(max−min)))` as shin. Scale flag is **bit 3 (0x8)** for the wiper, shin uses 0x10 | none (mask wipes use flags 0/4) | S |
| `WhirlWiper` (`0x28`), `GlassWiper` (`0x29`) | `whirl.rs`, `glass.rs` | matches (doc §7; both since compared with the original on screen) | | – | – |
| Scroll, Zoom, Turn, Wave, Scanline, Ripple wipers | `todo!()` in `AnyWiper::load` | missing | never used | none | L |

## 2. Top divergences (by what the player sees)

1. **Bustups never blink or lip-sync (prop 27 ignored).** Port `BustupLayer::update` (`0x7100036a00`) into
   `BustupLayerImpl::update` / `update_from_props`: if `int(prop 27) == 0` set mouth = 0 and blink timer = 0; else mouth =
   `min(level × mouth_count / 3, mouth_count − 1)` (level from the voice listener, `<0` → 0) and blink: `timer += dt`;
   if `timer ≥ wait + 20` draw `x = minstd(x)`, `r = (x − 1)·2⁻³¹`, `wait = (1 − r²)·240 + 80`, `timer = 0`, eye frame 0;
   while `wait ≤ timer < wait + 20` eye frame = `eye_table[int((timer − wait)/20 × len)]`. Hook the listener up where
   LAYERLOAD has `// TODO: connect the BustupLayer to the lipsync machinery`. Prop 27 is set 154 times on bustups; every
   talking sprite is affected.
2. **Screen (and page) layer transform applied twice while an effect runs.** In `screen_layer.rs::pre_render` and
   `page_layer.rs::pre_render`, pass the parent `transform` to `new_drawable_state.pre_render(...)` instead of
   `&self_transform` (the original `vfunc_8`s pass the parent, `0x71000e67f0`, `0x710009f360`; shin's own `LayerGroup`
   already does this). Visible whenever screen shake (wobble on `-2`) or a screen zoom overlaps screen ghosting, ripple,
   blur or raster.
3. **Equal RenderPosition sorts in reverse and by the wrong key.** In `layer_group.rs::pre_render`, break ties with
   `props.get_layer_id()` ascending (lower id drawn first, i.e. below), as the comparator `0x7100081110` does; keep the
   position test `|Δ| ≥ ε → higher first`.
4. **Container clear colour is opaque black instead of transparent.** `LayerGroup`/`PageLayer` indirect clears and the
   `LayerGroup`/`PageLayer` opaque-pass fill, plus the `render_layer(..., FloatColor4::BLACK)` background in
   `ScreenLayer`/`TransitionLayer`, should use (0,0,0,0) (`0x7100226c88`). Matters for the 407 plane-1 dissolves (the
   uncovered part of plane 1 must reveal plane 0) and any group composited with blending. Also make `ScreenLayer`'s
   indirect return `PassKind::Opaque` like the original.
5. **Wobbler phase handling.** In `Wobbler::update`, reset the phase only when the mode changes and run for any
   non-zero period (`0x71000c7270`); give the random mode the original seed once its source is known.
6. **Wobble slots 44–55.** Rotation = 44–47, scale X = 48–51, scale Y = 52–55 (`0x7100074d80`). Rename/rewire in
   `properties.rs` and `shin-core` `LayerProperty`, and move the bias defaults accordingly (check the original defaults
   first, see open questions). Unused by Umineko, so no visible change today.
7. **Small shader/blend fixes:** negative shader `× param.xyz` in `utils.wgsl`; `ColorBlendType::Layer3` →
   `ReverseSubtract`; MaskWiper scale flag = 0x8 (the wiper only, MASKLOAD keeps 0x10); finish `LayerFragmentShader::evaluate`
   (mono with `w`, fill, fill2 = identity, negative, gamma) so tiles cannot panic.
8. **Back layer group** (`create_back_layer_group_if_needed`): implement once its behaviour is confirmed (snapshot the root
   group when a layer command starts while animations are blocked; render the snapshot until animations run again).

## 3. Structural

- **One container implementation instead of three.** `LayerGroup`, `PageLayer` and `ScreenLayer` each re-implement the
  same "pre-render children, run the effect chain, composite or draw directly" logic, and they have already drifted (the
  double transform in two of them, the clear colours, the returned pass). The original shares `NewDrawableLayer::preRender`
  (`0x71000762e0`) and the composite `0x7100078af0`; each container only supplies slot 16 (indirect: clear colour,
  child iteration, returned pass) and slot 9 (direct). Proposed shape: a `ContainerChildren` trait (`render_children(pass,
  transform, stencil, pass_kind)`, `clear_colour()`, `indirect_pass()`), and one generic `Container<C>` that owns
  `NewDrawableLayerState` + `LayerProperties`, always passes the **parent** transform into the effect chain, and does the
  opaque fill with (0,0,0,0).
- **One property table.** Property roles are spread over `LayerProperty` names, `initial_value()`, and ad-hoc reads
  (wobbler wiring in `properties.rs`, prop 27 read only by rain). Build one table from the original's offsets
  (id → role, default, scale, which wobbler slot) with a test asserting the wobbler mapping (rotation 44, scale X 48,
  scale Y 52) and the transform order; that would have caught D6, and gives a place to record that prop 27 means
  "animate" for bustups and rain.
- **Blend ids as in the original.** Make `ColorBlendType` mirror the eight NVN blend states (0 none, 1 opaque, 2–4
  straight, 5–7 premultiplied) with one const table of factors/equations transcribed from `0x71000ce600`, and the two
  lookup tables (`0x7100226840` straight, `0x7100226d58` premultiplied) as named functions. This removes the hand-written
  match that got `Layer3` wrong.

## 4. Open questions

- Bustup lip-sync: what the listener's slot `+0x10` returns (voice level 0–3?) and how the eye-frame table
  (BustupLayer `+0xd0..+0xd8`) is built from the bustup file.
- Original default property values (RenderPosition, wobble biases 47/51/55): the `Layer::Properties` initialiser was not
  located (vtable `0x71001d9a80` is only referenced through `adrp/add`).
- Dissolve seed source (layer `+0x58`) and the wobbler's random seed (`+0` of each wobbler).
- Ghosting colour uniform: assumed (α, α, α, α) like shin; the decompiler lost the registers (`0x71000762e0` tail).
- Back layer group: exact trigger and lifetime in the original ADV render path.
- Which Umineko movies use the transparent mode (movie area).
- Not verified on screen: everything above is from code; the mask programs for MASKLOAD (`0x7100226da8`) were not
  decompiled.
