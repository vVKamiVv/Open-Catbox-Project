# Audit: layers, layer properties and effects, wipers, render programs

Status checked against the code on 2026-11-07.

> Written at the November 2026 audit and since refreshed: the Status, shin and "What differs" columns describe the
> code as it is now; the original's addresses and the reverse-engineering facts are kept. The whirl and glass wipers,
> blur, ripple, mosaic, dissolve and raster were also compared with the original on screen and match; that comparison
> and the port's measured accuracy are in [alignment-plan.md](../alignment-plan.md).

Logs, decompiles and extracted shaders: `$S\audit\layers\` (`d1.c`–`d9.c`, `sh\program_*_fs.glsl`, `prop_counts.txt`,
`prop_values.txt`). All addresses are from the v1.0.3 `main` (Ghidra base `0x7100000000`).

## How Umineko uses layer properties (English `main.snr`, 14,300 LAYERCTRL)

| Property | Uses | Main values / targets | shin |
|---|---|---|---|
| 5 RenderPosition | 4,875 | `-layer id` (-22, -5, -17, ...) | ok (sort key and tie-break as the original) |
| 68/69 dissolve | 2,374 / 240 | bustups 1,326, plane `-4` 415 (407 on plane 1), pictures 249 | implemented |
| 0, 1, 3, 4 translate | 1,202 / 681 / 82 / 184 | | ok |
| 9 alpha | 1,118 | 0, 1000, 588, 500, ... | ok |
| 27 | 363 | 0 on bustups (154), rain (166) | ok (bustups and rain) |
| 12/13 scale | 336 / 348 | | ok |
| 56–60 ghosting | 289 / 108 / 28 / 20 / 14 | 268 on the screen layer `-2` | ok |
| 23 blend type | 251 | always 1 (additive), 238 on movies | ok |
| 32–39 wobble X/Y | ~580 | mode 4 (sine), 1 (random); 76+76 on screen `-2` (screen shake) | ok; random-mode seed inferred |
| 66 blur | 86 | | ok |
| 82–84 ripple | 84 / 41 / 41 | 78 on screen `-2` | ok |
| 22 show | 71 | | ok |
| 24 fragment shader | 69 | 1 (mono) x64 with params 28–30 = (234, 164, 429); 4 (negative) x4 | ok |
| 26 flip | 62 | | ok |
| 73–75 rain | 55 / 16 / 4 | | ok (doc §6) |
| 67 mosaic, 76–81 raster, 40–43 wobble alpha | 14, 8–4, 2 | | ok |
| 2, 25, 44–55, 61–65, 70–72, 85–89 | **0** | never set by the script | 44–55 wired as the original; clip 61–65 and prop 70 are logged once and not drawn if set, harmless for Umineko |

Other counts: LAYERLOAD flags only 0, 2 (keep properties) and 3 (rain); LAYERUNLOAD delay always 0; MASKLOAD 169 (flags
16/17/19); WIPE mask type 29 (flags 0 or 4); TRANSSET/TRANSWAIT/LAYERSWAP/MASKUNLOAD 0.

## 1. Class/system table

| Original (address) | shin counterpart | Status | What differs / evidence | Player impact | Fix |
|---|---|---|---|---|---|
| `Layer::Properties` tweener block, `getValue` `0x7100074930` | `layer/properties.rs` | matches | 90 tweeners at `+0x28 + id*0x58`; blend type clamped `<3` else 0, fragment shader `<6` else 0 (`0x7100075530`), shader params = props 28–31 × 0.001 | none | – |
| Colour multiplier `0x7100074b30` | `get_color_multiplier` | matches | rgb × 0.0005 (`0x710015a938`), a = p9 × 0.001 × alpha-wobble (42/43), clamp [0,1] (`0x7100056710`), × (2,2,2,1) (disassembly) | none | – |
| Visibility `0x7100074cb0` | `is_visible` | matches | p22, int(p12), int(p13) ≠ 0 and alpha × wobble > 0 | none | – |
| Transform `0x7100074d80` | `get_transform` | matches | same order T(−scale origin)·S·T(so−ro)·R·T(t+ro); flip p26 bits 1/2; translation p0+p3−p20, p1+p4−p21, z p2 | none | – |
| Wobbler slots `+0x1f18..+0x1f68`, update `0x71000c7270` | `properties.rs` wobblers, `shin-core` `LayerProperty` | matches | props 44–47 drive **rotation**, 48–51 **scale X**, 52–55 **scale Y** (amp/bias 46/47, 50/51, 54/55); a test pins the slots. Scale-bias defaults (1000) are assumed, the original's initialiser was not found | none (44–55 never used) | – |
| Wobbler update `0x71000c7270` | `wobbler/mod.rs::update` | matches | phase resets only when the **mode** changes (invalid mode → 0); any non-zero period runs, a negative one backwards | none | – |
| Wobbler value `0x71000c7370` | `Wobbler::value`, `wobbler/prng.rs` | matches (modes 2–8); random mode inferred | random: mt19937 seeded `int(t)+seed` (0 → `0x12345678`) and `+1`, lerped by frac(t), 0 for the first period; seed is wobbler `+0`, source unknown; shin seed 0 (`TODO`) | low (13 random-mode uses) | S |
| Blend states (`setBlend` `0x71000cf700`, table filled by `0x71000ce600`) | `pipelines/conversions.rs` | matches | ids: 0 no colour, 1 opaque, 2/3/4 straight normal/add/sub, 5/6/7 premultiplied normal/add/sub; alpha channel always (1−dst α, 1). Id 4 is reverse-subtract (dst − src·α); shin `Layer3` and `LayerPremultiplied3` both use `ReverseSubtract` | none | – |
| Blend lookup tables `0x7100226840` = {2,3,4}, `0x7100226d58` → {5,6,7} | `from_regular_layer` / `from_premultiplied_layer` | matches | tile and movie direct draws use 2–4, picture blocks and effect composites 5–7 | none | – |
| Program tables by fragment shader: blocks `0x7100170dc0` {20,23,21,22,24,25}, composite-transparent `0x7100170dd8` {14,17,15,16,18,19}, composite-opaque/movie `0x7100170da8` {8,11,9,10,12,13} | `layer.wgsl` output kinds `LayerPremultiply` / `LayerDiscard` / `Layer` | matches | decompiled 8–25: 20–25 premultiply output, 14–19 discard below `c1.x` without premultiply, 8–13 plain | none | – |
| Fragment shaders (programs 8–13) | `utils.wgsl::evaluate_fragment_shader` | matches | default `tex×c`; mono `mix(c, luma, w)×xyz` (0.299, 0.587, 0.114); fill `mix(c, xyz, w)`; fill2 = **same code as default**; gamma `exp2(log2(c)/xyz)`; negative (program 12) `mix(c, 1−c, w) × xyz` | none | – |
| CPU fragment shader for tiles `0x71000735a0` | `LayerFragmentShader::evaluate` (`shin-render/src/lib.rs`) | matches | all six shaders, same formulas as the GPU programs (mono uses `w`) | none | – |
| `PictureBlockParams::setup` `0x7100072230` | `picture_layer.rs::setup` | matches | same pass split and shader simplification as shin's `is_equivalent_to_default` | none | – |
| `PictureLayer::renderDirect/renderIndirect` `0x71000c5fa0` / `0x71000c6040` | `picture_layer.rs`, `NewDrawableLayer::render_drawable_indirect` | matches, 1 shortcut | indirect always returns the transparent pass (`coversScreen` `0x7100072c80` not ported, marked `shortcut:`); block culling `TODO` | none | S |
| Effect chain `NewDrawableLayer::preRender` `0x71000762e0` | `new_drawable_layer/mod.rs`, `effect_passes.rs` | matches (doc §11) | blur, mosaic, raster, ripple, dissolve, ghosting confirmed; ghosting transform `0x7100075220` = shin's | none | – |
| Prop 70 effect (program `0x2c`) | `pre_render` | **stub** | Logged once and skipped if ever > 0 (the effect is not decoded) | none (never set) | M |
| Dissolve seed (layer `+0x58`) | `NewDrawableLayerState::dissolve_seed` | inferred | shin seeds from the clock (marked `shortcut:`); source in the original still open | low | S |
| Effect composite `0x7100078af0` | `try_finish_indirect_render` | matches | transparent pass programs 14–19 + blends 5–7, opaque pass 8–13 + blend 1; colour premultiplied (inferred from `0x71000566f0` multiply) | none | – |
| Clip mode props 61–65 (read in `0x71000788e0`) | not drawn: a set clip mode is logged once (`layer::warn_unsupported_clip`) | stub | would panic | none (never set) | M |
| `LayerGroup` render `0x710007d5e0` | `layer_group.rs::render` | matches | opaque pass children back to front, then fill quad (program 0, stencil greater, (0,0,0,0)), transparent pass front to back | none | – |
| `LayerGroup` child sort, comparator `0x7100081110` | `layer_group.rs::pre_render` sort | matches | higher RenderPosition first (|Δ| ≥ ε); ties: lower layer id first (key = Properties `+0x20`, set by LAYERLOAD from the record field `+0x58` = layer id). equal ids (the port's id-less screen and message layers) fall back to the layer bank | none | – |
| `LayerGroup` indirect `vfunc_16` `0x710007e2c0`, fill colour `0x7100226c88` | `LayerGroupNewDrawableDelegate::render_drawable_indirect`, `render` opaque fill | matches | effect texture cleared and uncovered pixels filled with (0,0,0,0) | none | – |
| `LayerGroup` delayed removal (`vfunc_3` `0x710007bc00`) | `remove_layer` | diverges | A delay is logged and the layer removed at once | none (delay always 0) | M |
| `LayerGroup::TransitionLayer`, LAYERLOAD auto-wipe (flag 4) | none | missing | | none (flag never used) | L |
| LayerGroup mask (MASKLOAD) `0x710007df80`, `0x710007d8d0` | `prepare_mask_*`, `finish_render_with_mask` | matches (structure, flag bits) | flip 1/2, invert 4, scale **0x10** in both; mask programs `0x7100226da8` unverified (not compared) | unknown (169 uses; the mask wiper has since been seen to match Eden in the scenario select's curtain wipe) | – |
| `PageLayer` `vfunc_8` `0x710009f360`, `vfunc_16` `0x710009fc50` | `page_layer.rs` | matches | plane occlusion and stencil bump match; the effect chain gets the parent's transform; clears and fill use (0,0,0,0) | none | – |
| `ScreenLayer` `vfunc_8` `0x71000e67f0`, `vfunc_16` `0x71000e6b70` | `screen_layer.rs` | matches | the effect chain gets the parent's transform (no double shake/zoom); indirect returns the opaque pass; background (0,0,0,0) | none | – |
| `ScreenLayer::TransitionLayer` + PAGEBACK | `TransitionLayer`, `pageback` | matches; stub branch | `pageback(start_anew = true)` starts from an empty page (inferred; PAGEBACK always passes false) | none | – |
| Back layer group (snapshot while animations are blocked) | `create_back_layer_group_if_needed` (`adv/mod.rs`) | **stub** (inferred semantics) | no-op; the original renders a snapshot instead of half-applied changes | low (possible one-frame half-built scenes) | M |
| `BustupLayer::update` `0x7100036a00` | `bustup_layer.rs` | matches | prop 27 = 0 freezes the mouth (frame 0) and resets blinking; otherwise mouth = `min(level × mouth count / 3, count − 1)` from the voice's lip-sync level, and blinking: timer += dt, after a wait of `(1 − r²)·240 + 80` ticks (`0x710015a640`, `0x710015aa48`; r from minstd_rand 48271, × 2⁻³¹) the eye frames play over 20 ticks. Each bustup is seeded from a random source, as in the original | none | – |
| `MovieLayer` `0x71000c3560` / `0x71000c38c0` | `movie_layer.rs` | matches | alpha (`Transparent`) movies drawn with their alpha (`movie_alpha.wgsl`), repeat replays from the start, layer colour and blend applied; YUV converted in the same pass (the original uses a separate pass, same image) | none | – |
| `TileLayer` `0x7100106d00` / `0x7100106ed0` | `tile_layer.rs` | matches | program 1, straight blends 2–4, colour run through the CPU fragment shader | none | – |
| `RainLayer` | `rain_layer.rs` | matches (doc §6) | | none | – |
| `AnimationLayer`, `SakuraAnimationLayer`, `EffectLayer`, `FocusLineLayer` | none | missing | never loaded (layer types 4, 5, 7) | none | L |
| `TimedWiper` update `0x710011baa0` | `wiper/timed.rs` | matches | elapsed += dt while animations run, clamped | none | – |
| `DefaultWiper` `0x7100062660` / program `0x24` | `wiper/default.rs` | matches (doc §7) | | none | – |
| `MaskWiper` render `0x71000a1700` / program `0x25` | `wiper/mask.rs`, `wiper_mask.wgsl` | matches | thresholds incl. invert identical; shader `mix(new, old, clamp(min + mask.r·(max−min)))`. Scale flag is bit 3 (0x8) for the wiper, MASKLOAD keeps 0x10 | none | – |
| `WhirlWiper` (`0x28`), `GlassWiper` (`0x29`) | `whirl.rs`, `glass.rs` | matches (doc §7; both since compared with the original on screen) | | – | – |
| Scroll, Zoom, Turn, Wave, Scanline, Ripple wipers | `todo!()` in `AnyWiper::load` | missing | never used | none | L |

## 2. What still differs

Everything the player could see from this audit is fixed. What is left is unused by Umineko or inferred:
("Unused" was checked on 2026-11-07 by scanning the disassembled `main.snr` of the English, Catbox X and Japanese
releases: no LAYERCTRL sets props 61–65 or 70 or the delta flag, every LAYERUNLOAD delay is 0, PAGEBACK takes no
argument, WIPE uses types 0, 1, 8 and 9 only, and LAYERLOAD types 4, 5, 7 and flag 4 never appear.)

1. **Wobbler random seed.** Random mode (13 uses) runs with seed 0; the original reads wobbler `+0`, source unknown.
2. **Dissolve seed.** Seeded from the clock; the original reads layer `+0x58`, source unknown.
3. **Back layer group** (`create_back_layer_group_if_needed`): a no-op. Implement once its behaviour is confirmed
   (snapshot the root group when a layer command starts while animations are blocked; render the snapshot until
   animations run again).
4. **Unused features without the original's behaviour:** clip mode 61–65 and prop 70 are logged and not drawn,
   a delayed LAYERUNLOAD removes at once; the six unported wipers still panic if loaded.
5. **Missing, never loaded:** `LayerGroup::TransitionLayer` (LAYERLOAD flag 4), layer types 4, 5 and 7.

## 3. Structural

- **One container implementation instead of three.** `LayerGroup`, `PageLayer` and `ScreenLayer` still each
  re-implement the same "pre-render children, run the effect chain, composite or draw directly" logic. Their earlier
  drift (double transform, clear colours, returned pass) was fixed in each one by hand. The original shares
  `NewDrawableLayer::preRender` (`0x71000762e0`) and the composite `0x7100078af0`; each container only supplies slot 16
  (indirect: clear colour, child iteration, returned pass) and slot 9 (direct). A shared container type that always
  passes the parent transform and fills with (0,0,0,0) would stop them drifting again. Not done.
- **One property table.** Property roles are still spread over `LayerProperty` names, `initial_value()`, and reads in
  `properties.rs` and the layers (prop 27 in bustup and rain). The wobble slots now have a test; a single table (id →
  role, default, scale, wobbler slot) is not done.
- **Blend ids as in the original.** `ColorBlendType` still uses its own names rather than the eight NVN ids from
  `0x71000ce600` and the two lookup tables (`0x7100226840` straight, `0x7100226d58` premultiplied). The values now
  match; this is only naming.

## 4. Open questions

- Bustup lip-sync: answered in code. The level (0–2, −1 when silent) comes from a state machine over the voice's peaks
  (`voice_player.rs::lipsync_levels`); the eye frames run up to the last eye block and back (`0x7100035350`).
- Original default property values (RenderPosition, wobble biases 47/51/55): the `Layer::Properties` initialiser was not
  located (vtable `0x71001d9a80` is only referenced through `adrp/add`).
- Dissolve seed source (layer `+0x58`) and the wobbler's random seed (`+0` of each wobbler).
- Ghosting colour uniform: assumed (α, α, α, α) like shin; the decompiler lost the registers (`0x71000762e0` tail).
- Back layer group: exact trigger and lifetime in the original ADV render path.
- Which Umineko movies use the transparent mode (movie area).
- The mask programs for MASKLOAD (`0x7100226da8`) were not decompiled; the MASKLOAD path is unverified on screen.
