# Umineko no Naku Koro ni Saku (Switch): engine internals

Notes on how the Nintendo Switch release of *Umineko no Naku Koro ni Saku ~Nekobako to Musou no Koukyoukyoku~*
(Entergram, title ID `01006A300BA2C000`) works internally, gathered while porting it to PC on top of
[DCNick3/shin](https://github.com/DCNick3/shin), an open-source reimplementation of the engine.

Everything here describes **behaviour and data formats**, verified against the original executable and assets.
No game code or assets are reproduced. Addresses refer to the v1.0.3 executable (`exefs/main`, NSO build ID
`7616F8963DACCD70E20FF3904E13367F96F2D9B3`) as loaded by Ghidra's Switch loader (base `0x7100000000`).

Status markers: **confirmed** (read from code and/or checked in-game), **inferred** (consistent with the code but not
fully traced), **open** (not known yet).

---

## 1. How to reproduce this research

1. Dump your own copy (base game + v1.0.3 update) and keys with your console.
2. Unpack the NSPs with `hactool` (`-t pfs0`), then extract the program NCA with the update applied
   (`--basenca=<base program nca> <update program nca> --exefsdir=... --romfsdir=...`).
   - hactool 1.4.0 rejects the 34-hex-digit `mariko_master_kek_source_*` lines of newer `prod.keys`; filter them out.
   - Pass `--disablekeywarns`, otherwise hactool prints console-unique keys.
   - Title keys come from the `.tik` files inside each NSP (encrypted title key at offset `0x180`, rights ID at `0x2A0`).
3. Load `exefs/main` into Ghidra 12.1.2 with [Adubbz/Ghidra-Switch-Loader](https://github.com/Adubbz/Ghidra-Switch-Loader)
   (loader "Nintendo Switch Binary", AArch64). Auto-analysis takes about 90 seconds.
   - On Windows, if Ghidra is launched from a packaged (MSIX) app, `%APPDATA%` is virtualized and Ghidra's OSGi script
     host fails ("The data file must be inside the data dir"); set `application.settingsdir`/`cachedir`/`tempdir` in
     `support/launch.properties` to a normal folder **without spaces**.
4. Helper scripts used for these notes live in `tools/ghidra_scripts/` (string cross-references, pointer search,
   vtable walking, decompile/disassemble at address, raw constant dumps).

The executable is **not stripped of RTTI**: C++ type names such as `N4shin9RainLayerE` are present, which makes it
possible to go from a class name to its typeinfo, its vtable and every virtual method.

---

## 2. Engine overview

See [class-inventory.md](class-inventory.md) for every class and [reverse-engineering-guide.md](reverse-engineering-guide.md)
for how to continue this research.

### Architecture (from the RTTI class list) **confirmed**

The executable contains 284 classes with RTTI, 282 of them in namespace `shin`:

- **Script commands:** every bytecode command is a class `shin::ADV::<NAME>` (61 of them, e.g. `ADV::LAYERLOAD`,
  `ADV::WIPE`, `ADV::SELECT`), plus `ADV::Command` (base), `ADV::Context`, `ADV::SceneLoader`, `ADV::SystemMenu`.
- **Layers:** `Layer` → `DrawableLayer`/`NewDrawableLayer` → `TileLayer`, `PictureLayer`, `BustupLayer`,
  `AnimationLayer`, `SakuraAnimationLayer`, `EffectLayer`, `MovieLayer`, `FocusLineLayer`, `RainLayer`, `QuizLayer`,
  plus containers (`LayerGroup`, `PageLayer`, `ScreenLayer`), each with a matching `*LayerLoader`.
- **Transitions:** `Wiper` with `DefaultWiper`, `MaskWiper`, `TimedWiper`, `GlassWiper`, `RippleWiper`,
  `ScanlineWiper`, `ScrollWiper`, `TurnWiper`, `WaveWiper`, `WhirlWiper`, `ZoomWiper` (each with a loader).
- **Text:** `MessageLayer` (with `Block`, `Section`, `Voice`, `Sync`, `Wait`), `MessageTextLayouter`,
  `MessageTextParser`, `MessageBrowser` (backlog, with `ChapterSelector`).
- **Menus and modes:** `Title`, `ScenarioSelect`, `SaveMenu`, `SaveGame`/`LoadGame`, `ConfigMenu`, `ManualViewer`,
  `TipsMode`, `CharsMode`, `CGMode`, `BGMMode`, `MovieMode`, `BustupMode`, `TitleDemo`, `FirstBoot`.
- **Minigames/overlays:** `QuizLayer`, `MurdererLayer` (the "who is the culprit" game), `CakeLayer`, `LastLayer`.
- **Infrastructure:** `TaskPool`/`AsyncQueue`, `CachedAssets`, `SourceMedia`, `TXA4`, `Font`, `RenderTexture`,
  `DepthStencil`, `MaskTexture`, `Audio`/`VoicePlayer`/`BGMPlayer`, `Movie`, `Trophy`.

### What the reimplementation still lacks (as of this writing)

The full, prioritised list from the November 2026 audit is in [alignment-plan.md](alignment-plan.md).

| Area | Original | shin | Missing or stubbed in shin |
|---|---|---|---|
| Script commands | 61 | 51 implemented | Missing: TRANSSET, TRANSWAIT, VOICESTOP, BGMWAIT, MSGSYNC, LAYERSWAP, MASKUNLOAD, RESUME, RESUMESET, NOP (unused). Stubs: CHARS, NOTIFYSET, TIPSGET, TROPHY, UNLOCK; AUTOSAVE's scene-title banner; SYSCALL 0's fade; EXIT stops the script instead of returning to the menus |
| Layer types | 10 | 7 | Animation (4), Effect (5), FocusLine (7), none used by Umineko. Quiz (9) has all four minigames (§13); the culprit game's texts come from the player's executable (§3) |
| Wipers | 11 | 5 (Whirl and Glass added) | Ripple, Scanline, Scroll, Turn, Wave, Zoom (none used by Umineko) |
| Systems | all menus and modes | adventure screen, saving and loading (§14) | title, save/load menus (a save is made with F5 and loaded with `--load`), config, backlog, galleries, tips, characters, manual, scenario select, chapter titles |

- The engine's own C++ namespace is **`shin`** (from RTTI), the same name the open-source reimplementation chose.
- Game logic runs from a compiled bytecode scenario, `main.snr`. Layers (pictures, sprites, movies, effects) are created
  by `LAYERLOAD` and animated through numbered layer properties set by `LAYERCTRL`.
- Every layer type has a matching `*LayerLoader` class (`RainLayerLoader`, `FocusLineLayerLoader`,
  `AnimationLayerLoader`, `QuizLayerLoader`, ...); their typeinfos sit together around `0x71001da3f0`.
- Layer properties are stored as an array of 0x58-byte tweeners starting at offset `0x28` of the property block;
  reading a property evaluates the tweener (`0x7100074930` -> `0x71000bb400`) and returns the raw script value
  (e.g. `1000` for a default of 1.0). **confirmed**

---

## 3. File layout of the dump

| Path | Contents |
|---|---|
| `romfs/data.rom` | All game data (11.1 GB, ROM2 archive). SHA-256 of v1.0.3: `6d90eb0bacacf769a7e4634407622b047acd711c47debb28136d7bab3fd0e591` |
| `romfs/patch.rom` | Update data (26 MB in v1.0.3), layered over `data.rom` |
| `romfs/copyright.bmp` | Legal screen image |
| `exefs/main` | Game executable (NSO, compressed) |
| `exefs/sdk`, `subsdk0`, `rtld`, `main.npdm` | Nintendo SDK, libraries, loader, process metadata |

The fan English translation ("Catbox") ships its own 374 MB `patch.rom` (replacing the update's) plus a patched `main`
and an IPS patch for the same build ID. Text, UI textures and translated images live in the `patch.rom`; the engine
reads files from `patch.rom` first and falls back to `data.rom`. **confirmed**

The translation's `main` is byte-identical to v1.0.3's; all its executable changes are in the IPS32 patch (60
records, offsets = NSO image offset + `0x100`). They are the texts the engine keeps in the executable (the culprit
game's testimonies `0x71001db528` and evidence `0x71001db270`, the system dialogs, "Yes"/"No"/"OK"/"End"/"%dm") and one
code byte: `0x71000b39da` turns `orr w0, wzr, #0x20000` into `#0x100000`, a constructor's buffer of 128 KB raised to
1 MB (room for the longer English text). Nothing adds word wrapping: the layouter breaks between any two characters
unless a kinsoku rule forbids it, so these texts, which have `@r` only between paragraphs, wrap mid-word in the original
too (to confirm on screen). The port reads them from the player's executable and IPS (`--exefs`, `SHIN_EXEFS`, or an
`exefs` folder next to the assets; `shin/src/asset/executable.rs`). **confirmed** (the wrapping: inferred)

---

## 4. ROM2 archive format **confirmed**

Little-endian.

| Offset | Size | Field |
|---|---|---|
| `0x00` | 4 | magic `ROM2` |
| `0x04` | 4 | version, `0x00010001` |
| `0x08` | 4 | index length in bytes |
| `0x0C` | 4 | file data offset multiplier (`0x200`) |
| `0x10` | 16 | unknown |
| `0x20` | | index starts here |

The index is a tree of directories. A directory is a `u32` entry count followed by 12-byte entries:

| Size | Field |
|---|---|
| 4 | name offset, relative to the start of this directory; **bit 31 set = subdirectory** |
| 4 | data offset: for a subdirectory, `× 16` from the start of the index; for a file, `× multiplier` from the start of the archive |
| 4 | file size in bytes |

Names are NUL-terminated ASCII. Directories contain `.` and `..` entries.

---

## 5. PIC4 pictures: differential blocks **confirmed**

shin already documented the PIC4 container and its dictionary-encoded blocks. Blocks without the dictionary flag use
**differential encoding**, which the original Japanese data apparently never uses but the English patch does
(e.g. `picture/text001.pic`):

- After LZ77 decompression, the block is `height` rows of `align16(width × 4)` bytes, RGBA.
- Row 0 stores plain pixel values.
- Every following row stores, per channel, the **difference from the pixel directly above**, wrapping modulo 256.

---

## 6. Rain, snow and petals: `shin::RainLayer` **confirmed**

Layer type 8 is a general particle layer. Its vtable is at `0x71001ddf90` (19 entries, the RainLayer-specific ones are
`0x71000c9xxx`–`0x71000caxxx`); a secondary vtable at `0x71001de038` is followed directly by its texture-name table.

| Function | Address | Role |
|---|---|---|
| constructor | `0x71000c99a0` | `(kind, distance_a, distance_b)` from `LAYERLOAD` params 0–2 |
| update | `0x71000ca640` | pre-warm, then one simulation step if property 27 is non-zero |
| step | `0x71000c9df0` | spawn, move, wrap, remove |
| spawn | `0x71000ca0b0` | creates particles |
| render | `0x71000ca730` | draws every particle as a textured quad |
| sort | `0x71000cad00` | `std::sort` of particles by depth before drawing |

### Load parameters

| Param | Meaning |
|---|---|
| 0 | particle kind: `0` rain, `1` petals (`hanabira`), `2` snow, `3` gold snow (clamped to 0–3) |
| 1, 2 | distance range; swapped if reversed, each clamped to 100–10000 |

Textures come from `/effects.txa` (`rain` 10×380, `hanabira` 60×60, `snow` 60×60, `goldsnow` 60×60; the archive also
holds `breakup` 28×28 and `focusline` 256×48).

### Per-kind constants

| Kind | Quad (before scaling) | Bounding radius | Base fall speed (per frame) |
|---|---|---|---|
| rain | 8 × 300 (±4, ±150) | 150.053 | 400 |
| petals | 60 × 60 | 42.43 | 8 |
| snow, gold snow | 30 × 30 | 21.21 | 5 |

### Properties used

| Property | Use |
|---|---|
| 27 | simulation runs only while non-zero (default 1) |
| 73 `RainIntensity` | spawn rate (default 1000) |
| 74 `RainDirection` | tilt; `1000` = 60° (π/3), clamped to ±60° |
| 75 `RainSpeed` | **particle size**, not speed (default 1000) |

### Simulation (per 60 Hz frame; the original passes `dt` in frames)

1. `accumulator += RainIntensity × 0.001 × (max_distance / 1000) × dt`; spawn `floor(accumulator)` particles, keep
   the fraction.
2. Spawning one particle (RNG is `minstd_rand`, multiplier 48271, modulus 2³¹−1, mapped to `[0,1)` by `(x−1)/2³¹`):
   - `u1` → `distance = min + u1 × (max − min)`; `u2` → `lateral = u2 × max`.
     If `lateral > distance` the draw is rejected and retried (so particles at distance `d` survive with probability
     `d / max`: far particles outnumber near ones).
   - `scale = 1000 / distance` (no cap).
   - `tilt = T + T × (2·u3 − 1) × 0.1`, where `T` is the layer tilt from property 74.
   - `size = RainSpeed / 1000 × scale`; the quad is scaled by `size` and rotated by `tilt`.
   - velocity = `scale × size × base_speed × (−sin tilt, cos tilt)`, proportional to `scale²`, so near drops are
     both bigger and much faster.
   - start: `x = lateral / distance × 1920`, `y = −extent` (extent = `size × bounding radius`), then shifted back
     along the fall direction by `extent × vx / vy`.
   - petals additionally spin by a random angle in ±π (quad only, not the velocity).
   - depth key for draw order = `u1`.
3. Every particle: `y += vy`; if `y < 1080 + extent`, `x += vx` and `x` wraps into `[0, 1920)`; otherwise it is
   removed.
4. **Pre-warm:** the first update runs whole-frame steps until at least one particle has left the screen, so the
   effect is already in full flow when the scene appears.

### Rendering

- Space is 1920×1080 with the origin top-left, translated by (−960, −540) into the layer's centered space.
- Particles are drawn far to near; a particle crossing the left/right edge is drawn a second time on the other side.
- There is **no per-particle alpha**: brightness is the texture times the layer's colour (scripts fade rain layers
  with `MulColorAlpha`, e.g. 700 and 500 in Episode 1).

### Verification

Compared frame by frame against the original (Eden emulator) at the start of Episode 1's "Letter and Umbrella"
chapter: the rainy mansion exterior shown by `WAIT 240` at script address `0x10f2f5`, right after rain layers
100–1000 and 1000–2000 are loaded. Streak count (~25–35 on screen), lengths, tilt and churn match. The
re-implementation's streaks look slightly wider and brighter, most visibly on near drops.

What was ruled out as the cause (all **confirmed** to match the original):

- **Blend:** the renderer picks the blend state from `0x7100226840` → `int[3] {2,3,4}` by layer blend type, which is
  exactly the reimplementation's `ColorBlendType::Layer1..3` (straight alpha, `SrcAlpha / OneMinusSrcAlpha` for
  type 1). The same tables are used by the generic layer pipeline (`0x7100025f8c`) and other renderers.
- **Shader:** the program comes from `0x7100226838` indexed by the layer's fragment shader (default → program 8, the
  layer shader); with the default fragment shader it computes texture × layer colour, the same as a sprite draw.
- **Geometry and texture:** the quad is `(±4, ±150)` with UVs covering the whole 10×380 `rain` texture, whose alpha
  peaks at ~146/255 in the centre and falls off over ~6 columns.
- **Colour space:** both blend in gamma space (the reimplementation uses `Rgba8Unorm` on a non-sRGB surface).

A measurement over 3–4 frames (mean minus median luminance of a 640×540 region) gave 0.55 for the reimplementation
and 0.17 for the original; the sample is too small to tell a real difference from random near-camera drops.
**Open:** repeat with many more frames; if the gap is real, check whether the texture bound in the render function
(`*0x7100226850`, a global rather than the per-kind texture) is really the `rain` texture.

### How Episode 1 uses it

The first rain scene (Kinzo's study) sets up rain in a shared subroutine around script address `0x0a1124`:
three rain layers with distance ranges 100–1000, 500–1500 and 1000–2000, intensities raised to 3000 and 2000,
one layer's `RainSpeed` set to 500, and `MulColorAlpha` 700/500. The rain layers sit behind the room picture and in
front of the night-sky picture, so indoors the rain is only visible through the windows.

---

## 7. Transitions (wipers)

### Which ones the game uses **confirmed**

Counted over the whole English `main.snr` (`sdu scenario disassemble`, 24,710 `WIPE` instructions, all with a literal
type). "Sites" are `WIPE` instructions; some sit in shared helper subroutines that scenes call many times:

| `WIPE` type | Wiper | Sites | Notes |
|---|---|---|---|
| 0 | `DefaultWiper` (crossfade) | 24,500 | |
| 1 | `MaskWiper` | 29 | |
| 8 | `WhirlWiper` | 14 | one is in helper `0x0a0924` (duration as argument, also animates layer properties 56–60), called **127** times with 120 or 180 frames |
| 9 | `GlassWiper` | 167 | 3 of them in shared helpers below `0x0a2000` |
| 2–7 | `ScrollWiper`, `ZoomWiper`, `TurnWiper`, `WaveWiper`, `ScanlineWiper`, `RippleWiper` | 0 | present in the engine, never used by Umineko |

### Common structure **confirmed**

Every wiper class has a 9-entry vtable; class-specific code is in slots 0/1 (destructors), 3 (per-frame update, only
where needed), 4 (clone: copies the parameters), 5 (compare with another wiper of the same type) and 8 (render). The
shared base keeps elapsed and total time at `+0x1c`/`+0x20`; progress is `elapsed / total`
(`0x710011ba80`). Render receives the new and the old screen as textures. Drawing goes through global render state:
`0x71000cf720` sets the shader program id, `0x71000cf700` the blend state, `0x71000cf7c0(slot, data, size)` a uniform
block, `0x71000cf830` the vertex data, `0x71000cf740` a texture binding.

### DefaultWiper **confirmed** (render `0x7100062660`)

Full-screen quad, orthographic projection, both screens bound, progress in uniform 1, program `0x24`, opaque blend.

### ScrollWiper **confirmed** (render `0x71000efcf0`, unused by Umineko)

CPU-only. Two direction parameters (`+0x24` for the new screen, `+0x28` for the old one), each 1–8 =
+x, +x+y, +y, −x+y, −x, −x−y, −y, +x−y (0 = no movement; y points down). The old screen is drawn opaque, offset by
`p × direction` screen sizes; the new screen is drawn on top with straight alpha (program 2, blend 2), offset by
`(1 − p) × direction`, so it slides in.

### WhirlWiper **confirmed** (constructor `0x710011b410`, render `0x710011b770`, shader program `0x28`)

Parameters (from `WhirlWiperLoader` `0x710011c6c0`): `+0x24` direction = the command's integer parameter, `+0x28`
scale = `1 / float parameter`, or `1.0` when its magnitude is below a small epsilon (`0x710015aa98`). Umineko's only
use passes zeros, giving direction −1 and scale 1.

Render: one full-screen quad, old screen in texture slot 0, new screen in slot 1, opaque blend, uniform
`(p, d, 1.0, scale)` with `d = +1` if direction is 1, else −1. The fragment shader, per pixel with UV in [0, 1]:

1. `q = (uv − 0.5) × (1.0, scale)`, `r = |q|`.
2. `s = smoothstep(clamp(p, 0, 1))` (`t²(3 − 2t)`).
3. `ripple = smoothstep(clamp(1 − 2|p − 0.5|, 0, 1)) × sin(10π·r) × 0.3`, strongest mid-transition.
4. Rotate `q` by `(s·d + ripple)` full turns, divide by `(1.0, scale)` and add 0.5 back.
5. Output `mix(old, new, s)` sampled at that rotated UV.

### GlassWiper **confirmed** (constructor `0x710006b930`, update `0x710006c980`, render `0x710006cae0`, shader program `0x29`)

Implemented in shin as `shin/src/wiper/glass.rs` + `wgsl/wiper_glass.wgsl` (not yet compared on screen). Summary first,
then the details as they were worked out:

- `GlassWiperLoader` (`0x710011c740`) passes only the duration; Umineko's 167 uses all have zero parameters.
- Construction: seed `minstd_rand` from a system random source (`0x71000fc2c0`, reduced mod 2³¹−1, 0 → 1); time scale
  `+0x28` = 60 / duration; gravity `+0x2c` = 0.35. Lattice of 13 × 8 points, row by row, two random numbers per
  point: x = column × 160 + rand × 128 − 64, y = row × 154.2857 + rand × 123.43 − 61.71; then **all four edges** are
  pinned (left column x = 0, right x = 1920, top row y = 0, bottom y = 1080; pinned points still consume their random
  numbers). Each of the 12 × 7 cells gives two shards in this order: (top-left, top-right, bottom-left) and
  (bottom-right, bottom-left, top-right). Shard order is row-major, 24 per row.
- Per shard (random draws in this order): centre = triangle centroid + (rand × 80 − 40, rand × 77.14 − 38.57), z = −4,
  in centred world coordinates `(x − 960, 540 − y)` with y up; angular velocity per axis (x, y, z) =
  `(rand × 0.10471667 − 0.052358337) × scale`; linear velocity = `(cx × 0.4 / 60, cy × 0.4 / 60, 15) × scale`;
  vertical speed `+0x270` = `2 × (1080 − cy) / 1080`; rotation 0.
- Mesh: **18** vertices of 32 bytes (position xyz relative to the centre, normal xyz, uv), stitched by a 24-entry
  `u16` index list (`0x7100170be0`) into 8 triangles: front face (z = 0, i.e. +4 from the centre), back face (z = −8),
  three side quads. The 6 face vertices are front A, B, C and back A, C, B; the 12 side vertices are copied from them
  through the corner table `0x7100170bcc` (4 per side, each side with its own normal). All normals point **into** the
  shard: front (0, 0, −1), back (0, 0, +1), side AB `normalize((B − A) × (A − A'))`, BC and CA likewise. UV = lattice
  point × (1/1920, 1/1080). The ±0.4-cell jitter can push a lattice point across its cell's diagonal; the triangle
  then folds over its neighbour, and since the vertex order is fixed, that shard is inside-out (culling then shows
  its back faces). shin keeps that behaviour.
- Update `0x710006c980`: rotation += angular velocity × dt, position += velocity × dt, then the vertical speed is
  integrated piecewise over the normalised time `+0x30` (which advances by scale × dt): within a whole step y gains
  `segment × vertical speed`; at each whole-step boundary the vertical speed drops by the gravity. Over a transition
  that is 60 steps, so the shards rise briefly and then fall.
- Render `0x710006cae0`: new screen as a full-screen quad (program 2, blend 1 = opaque). If `p < 1`: cull state 2
  (`0x71000cf6e0`), old screen in texture slot 0, blend 2 (straight alpha), program `0x29`; uniform 2 = `(alpha, flash,
  0, 0)` with `alpha = 1` until `p = 0.75` then `1 − ((p − 0.75)·4)²`, `flash = (1 − 2p)/2` while `p < 0.5`, else 0.
  View-projection = perspective (`0x71000a1c40`: vertical FOV 0.99026656 = `2·atan(0.54)`, aspect 16/9, near 10, far
  10000) after a camera translation of z = −1000, so z = 0 lands 1:1 on the 1920 × 1080 screen. Per shard the model
  matrix is built row-vector style as `Rx · Ry · Rz · T(position)` (rotate about x, then y, then z, then move), uploaded
  as uniform 1, and `model · viewprojection` as uniform 0; one 24-index draw per shard; cull state is reset to 0 at
  the end.
- Shader `0x29`: vertex shader transforms by uniform 0, passes the UV through, rotates the normal by uniform 1 and
  computes `light = −0.1767767 × (n.y + n.z) + flash` (0.25/√2: a quarter-strength light along (0, 1, 1), which with
  inward normals lights faces turned up and towards the camera); fragment shader outputs
  `rgb = texture(old, uv).rgb + light`, `a = alpha`. At rest the visible front face gets +0.177 on top of the flash,
  so the old screen flashes white (+0.677) the moment it breaks and stays slightly brightened.
- Cull state: `0x71000cf6e0` stores an index that the state flush `0x71000cf980` looks up in the table at
  `0x7100233af8` (filled by `0x71000ce600`: 0 → 0, 1 → 2, 2 → 1, i.e. NVN cull none / back / front) and applies
  through an NVN polygon state together with a front-face value of 0. shin's `CullFace` enum has the same order
  (`None`, `Back`, `Front`), so the glass shards use `CullFace::Front`. **Inferred:** combined with the clip-space y
  flip that both engines apply, this hides faces turned away from the camera; check against the original (shards
  should look brighter than the old screen after the flash, not darker).

Data layout and constant addresses (for anyone re-checking the numbers):

- Wiper object: seed `+0x24`, time scale `+0x28`, gravity `+0x2c`, normalised time `+0x30`, shard vector `+0x38`.
- Shards are 628 bytes (`0x274`): the 18 vertices (`0x240` bytes, uploaded per shard), position `+0x240..0x248`,
  rotation about x/y/z `+0x258..0x260`, linear velocity `+0x24c..0x254`, angular velocity `+0x264..0x26c`, vertical
  speed `+0x270`.
- Constants: 60 `0x710015a778`; lattice step 160 `0x710015a35c` and 154.2857 `0x710015a53c`; jitter scales 128
  `0x710015a798` and 123.43 `0x710015a79c`, offsets −64 `0x710015a37c` and −61.71 `0x710015a754`; centre offset 80
  `0x710015aa48`, −40 `0x710015a340`, 77.14 `0x710015a344`, −38.57 `0x710015a434`; 0.4 `0x710015a8ec`; spin
  0.10471667 `0x710015a4f4` and −0.052358337 `0x710015a5dc`; UV scales 1/1920 `0x710015a4bc` and 1/1080
  `0x710015a380`; projection 0.54 `0x710015a3d8`, FOV `0x710015aa68`, aspect `0x710015a71c`, far `0x710015a658`, camera
  z −1000 `0x710015a820`.
- Math helpers used here: `0x71000a2020` makes a vec3 (a bare `ret`: the arguments are already in `s0..s2`),
  `0x71000a1cf0` subtracts two vec3s, `0x71000a1d80` cross product, `0x71000a1d20` normalize, `0x71000a1b90` identity
  matrix, `0x71000a1bc0` orthographic matrix `(left, right, bottom, top, near, far)`, `0x71000a1c40` perspective
  matrix `(fov, aspect, near, far)`, `0x71000a1e10` / `0x71000a1e90` rotation about x / y (z is `0x71000a1f10`).
- Whirl's "unset" epsilon `0x710015aa98` is `FLT_EPSILON` (1.1920929e−7).

### Shaders **confirmed**

There are no shader files in the romfs: all 53 shader programs are embedded in the executable as NVN (Maxwell)
binaries.

- Static program table `0x71001de1c0`, 48 bytes per program; `+0` points to the vertex blob, `+8` to the fragment
  blob. Program id = index (e.g. 2 sprite, 8 layer, `0x24` default wiper, `0x28` whirl, `0x29` glass).
- Each blob: base offset at `+0x94`; control section at `+0xc0` (size `+0xcc`, kept on the CPU); GPU code at `+0xc4`
  (size `+0xc8`, copied to GPU memory). The setup loop is `0x71000cea50`; runtime program objects live at
  `0x710022d2f0` (stride `0x1f8`) and are bound by the state flush `0x71000cf980`.
- The code section starts with a 0x30-byte NVN header (magic `0x12345678`), then the Maxwell shader program header
  (SPH) and instructions; the compiler's literal pool (constant bank `c1`) follows the code (at `0x300` for these
  small shaders).
- Decompile with `ExtractShaders.java` + Ryujinx's `ShaderTools` (see the reverse-engineering guide).
- Default wiper `0x24` decompiles to a plain `mix(old, new, p)`, matching the reimplementation.

## 8. Movies **inferred**

Movies are H.264 MP4 files (`movie/*.mp4`), opaque or with a transparency mode; 1920×1088 High profile with B-frames
(the effect overlays `no80`–`no84` have 45–86 frames each, all of which decode fine).

Port note (**confirmed**, not an engine property): shin's ffmpeg-pipe decoder used to lose the end of the stream, and
sometimes all of it, because on Windows `async-process` writes ffmpeg's stdin through a buffered background thread and
the pipe was dropped without being flushed. Under load (several movies at once, as in the game) a clip then came out
truncated or empty and the movie layer panicked ("No frames available"). The pipe is now closed explicitly; the
ignored test `concurrent_decodes_yield_every_frame` in `shin-video` (`SHIN_TEST_MOVIE=<mp4>`) decodes a clip 40 times,
four at once, and checks every run gets every frame.

---

## 9. Open questions

- What property 27 is for besides gating particle simulation.
- The exact blend mode table used by the particle renderer (`0x7100226840` / `0x7100226838`).
- `SYSCALL` (`shin::ADV::SYSCALL`, execute `0x7100007c10`): ids other than 0 do nothing; id 0 (`FUN_7100012550`) tweens
  an ADV context value at `+0x500` to 0 over the shared constant 0.1 if the flag at `+0x4f8` is set, then clears that
  flag and `+0x47c`. What those context fields are is **open**; the port currently finishes the command immediately.
- How chapter titles (e.g. "Letter and Umbrella", bottom-left at the start of each chapter) are drawn; the
  reimplementation doesn't show them yet.
- The remaining layer types (`FocusLineLayer`, `AnimationLayer`, quiz layers) and the `EVBEGIN`/`EVEND`,
  `NOTIFYSET`, `CHARS`, `TIPSGET` commands.

---

## 10. Script commands: what Umineko uses and how they run

### Execution model **confirmed**

Each script command is a small object (`shin::ADV::<NAME>`) with a 5-slot vtable. Slot 4 is the execute function; the
ADV loop calls it every frame until it returns true (done), so waiting commands simply keep returning false. Fields:
`+0x8` the ADV context, `+0x10` the scene/system context, arguments from `+0x18` in declaration order. Slot 3 returns
1 for every command looked at except `EXIT` (0); its meaning is **open**.

The VM hands each command to the ADV through a listener interface; the ADV constructs the command object in place at
ADV `+0x25a0`, stores it as the current command at `+0x2590` and destroys the previous one through `+0x2598`. For
MSGSET the listener entry is `0x71000007b0` (primary vtable slot 9), but the game calls it through the thunk
`0x7100000830` (this − 8, ADV's secondary base); both call the constructor `0x7100005140(cmd, vm, adv, args)`. Its
arguments: `+0` auto_wait (byte), `+4` message id (u32), `+0x10` text pointer. **Confirmed in Eden:** the message id
the game passes is the script's id **minus one** (the script's 109, "Nanjo: ......You've...", arrives as 108, and so
on for every line checked). The constructor checks that id against the read-message set (`0x71000d19f0`, at
`PTR_DAT_71002264f0 + 0x20`) to decide whether skip may pass the line, so read flags in save data are indexed by the
script's id − 1; mind this when the port reads or writes them.

### Static inventory of the English `main.snr` **confirmed**

Counted over the whole script (`sdu scenario disassemble`, 49 command types used), so missing features can be listed
up front instead of found one crash at a time:

| Commands | Uses |
|---|---|
| MSGSET 72,998 · WAIT 47,792 · MSGCLOSE 39,295 · WIPE 24,710 · SEPLAY 21,036 · LAYERCTRL 14,300 · MSGWAIT 11,245 · PAGEBACK 9,774 · LAYERUNLOAD 8,361 · MSGSIGNAL 6,374 · LAYERLOAD 5,036 · SEVOL 3,967 | the bulk |
| LAYERWAIT 2,142 · BGMPLAY 1,745 · PLANESELECT 1,311 · LAYERINIT 893 · BGMVOL 839 · MOVIEWAIT 825 · MSGINIT 781 · BGMSTOP 756 · NOTIFYSET 524 · CHARS 511 · PLANECLEAR 483 · SESTOP 283 · SAVEINFO 236 · SESTOPALL 202 · AUTOSAVE 188 · MASKLOAD 169 | common |
| SEONCE 148 · BGMSYNC 113 · VOICEWAIT 102 · VOICEPLAY 84 · EXIT 53 · UNLOCK 38 · EVBEGIN/EVEND 32 · TROPHY 28 · SELECT 25 · TIPSGET 21 · QUIZ 20 · LAYERSELECT 15 · SGET 4 · SSET 3 · WIPEWAIT 2 · SEWAIT 2 · SYSSE 1 · SYSCALL 1 · DEBUGOUT 1 · SHOWCHARS 1 | rare |

Never used: TRANSSET, TRANSWAIT, VOICESTOP, BGMWAIT, MSGSYNC, SEPAN, LAYERSWAP, MASKUNLOAD, RESUME, RESUMESET, NOP.

Other things checked the same way:

- Layer types loaded (`LAYERLOAD`): picture (2) 3,480, movie (6) 835, bustup (3) 578, tile (1) 93, rain (8) 31,
  quiz (9) 19. Animation, effect and focus-line layers are never loaded.
- Layer effects set to a non-zero value (`LAYERCTRL`): dissolve (68) 1,201, blur (66) 56, ripple (82) 43, mosaic
  (67) 7, raster (76) 4; property 70 never. Fragment-shader modes (property 24): mono 64, negative 4.
- `LAYERCTRL` flags: easings 0–4 only, plus `ff_to_current` (328) and `scale_time` (2); `delta` is never combined
  with a fast-forward flag and no unused bits are set.
- `LAYERUNLOAD` delays are always 0; `PAGEBACK` takes no arguments; the only unary VM operation used is negate.
- `SELECT`: 15 in the developer scenario-select menu (`NCSELECT`, around `0x09cc84`), 5 in a hint menu (`0xa9dd7e`–`0xa9e089`),
  5 real story choices titled "Choice" (`0xbf2abd`–`0xc1b32a`, e.g. "Kill" / "Don't").

### Sound status bits **confirmed**

The status that the *WAIT commands test against their mask (`0x71000f2fd0` for sound effects, `0x7100033e00` for
music) is: 2 = a sound is on that channel, +1 for one kind of track (a field at `+0xc` is 0; probably "not looping"),
+4 / +8 / +16 = a volume / pan / speed tween is still running. shin's `AudioWaitStatus` already uses this layout. The
voice status (`0x710011a980`) is 3 if any voice is in state 0, otherwise 2 if any is in state 1, otherwise 0.

### Commands added to the port in this round **confirmed**

- `SEONCE` (execute `0x7100007320`): `(se, wait mask, volume, pan, 1000)`. In skip mode (`0x7100012430`) it ends at
  once without sound. Otherwise its first run plays the sound once, without loop or fade-in, on the first automatic
  channel (−1, −2, …) that is not playing, then it waits until that channel's status clears the mask (Umineko: 0 or
  1). The fifth argument is not read.
- `BGMSYNC` (`0x7100006e20`): done when no music is playing, or when the track position (microseconds,
  `0x7100033e70`) reaches `sync_time` frames of 1/60 s.
- `VOICEWAIT` (`0x7100007760`): done when the voice status clears the mask (always 2).
- `WIPEWAIT` (`0x7100006b30`): done when the page layer has no running transition (`0x71000e66e0`).
- `SYSSE` (`0x71000077d0`): id 0 plays the built-in sound named "horror" (string at `0x710015d64e`) from `sysse.bin`
  at the given volume (`0x71000272b0`); any other id does nothing; never waits.
- `EXIT` (`0x7100004b10`): first frame: the SYSCALL 0 fade, clear the layer banks, close the message box, stop the
  voice, fade and stop all sound effects, and start a 30-frame uninterruptible wait (`0x71000138d0`). Then, once the
  wait, the fade and the task pool are done, it leaves for the menus (`0x710000fab0`, `0x71000147e0`). The music is
  not stopped. The port stops the sound effects and then the script, as it has no menus yet. (An earlier version of
  these notes read the 30-frame wait as a music fade-out.)
- The **ADV wait timer** (context `+0x490` interruptible flag, `+0x494` frames left as a float): `0x71000138d0`
  starts it with `(interruptible, frames)`, `0x71000138f0` cancels it (skip mode), `0x7100013900` reports it active
  while frames > 0. `WAIT` (`0x7100005090`) is just "start it, cancel it while skipping, done when inactive"; `EXIT`
  uses it for its pause. Where the interruptible flag is read, i.e. what ends such a wait early, is **open**; the port
  ends interruptible waits on the advance input.
- `MSGCLOSE` (`0x7100005880`): closes the message box with its animation unless skipping (`0x71000b13a0`); with
  `wait_for_close` it stays until the close animation is over (`0x71000b19c0`).
- `LAYERLOAD` flags used by Umineko: 2 ("keep previous properties", 180 uses) and 2 + 1 ("don't block animations",
  4); shin already handled both but asserted that no flag was ever set.
- Related: `SESTOPALL` (`0x7100007020`) uses the same skip test and drops its fade-out to 0 in skip mode; its
  stop-all (`0x71000f2d20`) also stops the automatic channels.

### Still missing, in rough order of impact

1. Voice playback (shin's voice player is a stub, so `VOICEWAIT` currently never waits).
2. Menus, so that `EXIT` can return somewhere.
3. The culprit game's testimony and evidence texts (§13), which are strings in the executable.

---

## 11. Layer effects

### The effect chain **confirmed** (`0x71000762e0`, shared by every drawable layer)

Each frame, before drawing, a drawable layer (picture, bustup, movie, …) reads eight properties: blur radius (66),
property 70, mosaic size (67), raster amplitudes (76, 79), ripple amplitude (82), dissolve intensity (68) and ghosting
alpha (56). They are tweeners at `properties + 0x28 + id × 0x58`; the float ones are scaled by 0.001. If all are zero
and the layer doesn't otherwise need a separate pass, it is drawn directly. Otherwise:

1. It renders itself into an offscreen texture (virtual slot 16; for `shin::PictureLayer` `0x71000c6040`): bind the
   texture (`0x71000cf250`), clear colour to transparent black and stencil to 0 (`0x71000d0380(1.0, 5, 0, 0)`), and
   draw with **neutral** parameters (`0x7100072360`: both opaque and transparent picture blocks, colour (1, 1, 1, 1),
   blend type 1, default fragment shader) and stencil reference 1 (`0x71000731a0`). The layer's own colour, alpha,
   blend type and fragment shader are applied once, when the finished texture is composited. It returns the pass the
   composite belongs in: opaque only if the picture is fully opaque and its transformed rectangle covers all four
   screen corners (`0x7100072c80`), transparent otherwise.
2. Effects run on that texture in this order, each into a second texture that is then swapped with the first:
   blur (program `0x2b`, drawn twice), property 70 (`0x2c`), mosaic (`0x2a`), raster (`0x2d`), ripple (`0x2e`),
   dissolve (programs 2 and `0x2f`), ghosting (program 8 with blend 5, the previous frame).
3. The result is composited with the layer's colour, blend type and shader.

Render-state helpers used here: `0x71000cf250` set render target, `0x71000d0380` clear `(depth, flags, colour, stencil)`
with flags bit 0 colour, bit 1 depth, bit 2 stencil; `0x71000cf650` stencil state, laid out as
`{function, fail op, depth-fail op, pass op, write mask, read mask, reference}` with GL-ordered enums
(function 7 always, 5 not-equal; op 0 keep, 2 replace), the same order as shin's `StencilFunction` and
`StencilOperation`; `0x71000cf880` index data; `0x71000cffc0` indexed draw; `0x71000d0520` create a render texture.

In the port: `NewDrawableLayer::render_drawable_indirect` (default method) and
`NewDrawableLayerState::{pre_render, try_finish_indirect_render}`.

### Dissolve **confirmed** (property 68 = intensity × 1000, property 69 = mode)

The layer breaks apart into 20-pixel pieces, starting from the corner opposite the one the mode selects
(bit 0: right instead of left, bit 1: top instead of bottom). With `p = 1 − 3 × min(intensity, 1)` (1 intact,
−2 gone):

- **Intact part** (only while −1 ≤ p < 1): a right triangle in the selected corner with legs
  `L = (p + 1) / 2 × 2971.72` (`0x710015a724`), texture coordinates = position / (1920, 1080), drawn with the plain
  sprite program (2), opaque, writing stencil 1 (always / replace). Its three corners come from the index table
  `0x7100170f70` (`{0,3,2}`, `{2,1,3}`, `{0,1,2}`, `{0,1,3}` for modes 0–3 over the quad corners top-left, top-right,
  bottom-left, bottom-right).
- **Pieces**: a 96 × 54 grid of 20-pixel cells. For each cell (row by row), with `c'` = column (or 96 − column when
  bit 0 is clear) and `r'` = row (or 54 − row when bit 1 is set), distance `d = (c' + r') / 148` (`0x710015a9d4`) and
  cell progress `t = p + 2d`. Two `minstd_rand` numbers are drawn for every cell (seeded from the layer's field `+0x58`;
  its source is **open**): an angle and a radius. Cells with `t ≤ 0` or `t > 1.1` (`0x710015a43c`) are skipped; for
  `t ≥ 1` the piece sits still; below 1 it moves by `radius × (1 − cos((1 − t) × π/2))` in direction
  `(base + rand₁ × 0.48 − 0.24)` turns, `radius = rand₂ × 800 + 200` (`0x710015a6b4`, `0x710015a8b0`), with base 0.875,
  0.625, 0.125, 0.375 turns for modes 0–3 (away from the intact corner; y points down). Each piece is a square of side
  `32 t` (`0x710015a674`) around the cell centre, showing the picture patch at its starting position, masked by the
  "breakup" texture of `effects.txa` (looked up by name, string `0x710015e9f7`). They are drawn with program `0x2f`,
  blend 5 (premultiplied alpha) and stencil not-equal 1, so they never cover the intact triangle.
- Shader `0x2f`: vertices of 24 bytes (position xy, picture uv, mask uv); fragment = `texture(picture, uv) ×
  texture(breakup, mask_uv).a`.
- The diagonal distance slightly exceeds 1 in the corner cells, so at full intensity a few sub-pixel specks remain,
  in the original as in the port.
- The result always composites in the transparent pass.

In the port: `layer/new_drawable_layer/effect_passes.rs` (`apply_dissolve`) and `wgsl/dissolve.wgsl`.

### Blur **confirmed** (property 66 = radius × 1000)

Separable, at half resolution, four draws that each render into the second texture and swap it with the first:

1. Shrink (`0x7100078360`): the sprite program (2), opaque, draws the whole canvas into a half-size viewport
   (`0x71000cf3c0(target, no depth, width / 2, height / 2)`), i.e. the top-left quarter of the texture.
2. Horizontal pass, program `0x2b`, opaque, half-size viewport, reading uv 0–0.5: eight taps at `uv ± offsetᵢ` with
   offsets `(0.5, 1.5, 2.5, 3.5) × radius / 1920` and the result `Σ ½ wᵢ (tap₋ + tap₊)`, weights
   `(0.4041, 0.3147, 0.1909, 0.0902)` (bit patterns `0x3eceed6b`, `0x3ea127be`, `0x3e437dbe`, `0x3db8afdf`, summing to
   1; there is no centre tap).
3. Vertical pass, the same with offsets `/ 1080` along y.
4. Grow back (`0x7100078580`): the sprite program, opaque, full viewport, reading uv 0–0.5.

The target is not cleared, so taps that reach past the quarter read stale texels at the picture's right and bottom
edges, in the original as in the port. In the port a projection that maps the canvas onto the top-left quarter stands
in for the half-size viewport (`effect_passes::apply_blur`, `wgsl/blur.wgsl`).

### Ripple **confirmed** (properties 82 amplitude, 83 wavelength, 84 period)

One opaque full-canvas pass of program `0x2e`. Uniforms: centre (0.5, 0.5), scale (1, 1) (both constant),
`k = 1 / (int(property 83) × 0.001)` (0 when that is 0), amplitude `property 82 / 1920` and the running phase. Per
pixel: `d = (uv − centre) × scale`, `r = |d|`, `r' = r + amplitude × sin(2π k (2r − phase))`, sample at
`d / r × r' / scale + centre`: rings that travel outward as the phase grows.

The phase lives in the layer at `+0x54` and is advanced every frame by `0x7100078780`: `phase += dt / property 84`
(dt in 1/60 s ticks, skipped when property 84 is 0). The same function advances two raster phases, `+0x4c` by
`dt / property 78` and `+0x50` by `dt / property 81`. In the port: `NewDrawableLayerState::update`,
`effect_passes::apply_ripple`, `wgsl/ripple.wgsl`.

### Mosaic **confirmed** (property 67 = block size − 1)

Runs when `int(property 67) > 0`: one opaque full-canvas pass of program `0x2a`. With `n = int(property 67)` the
uniform block is `((n+1)/1920, (n+1)/1080, 1920/(n+1), 1080/(n+1))` and a centre `(0.5, 0.5)`. Per pixel:
`block = floor((uv − centre) × (1920, 1080)/(n+1)) + 0.5`, sample at `block × (n+1)/(1920, 1080) + centre`, so
blocks of `n + 1` pixels aligned on the picture centre, each showing its centre pixel. In the port:
`effect_passes::apply_mosaic`, `wgsl/mosaic.wgsl`.

### Raster **confirmed** (properties 76–78 horizontal, 79–81 vertical: amplitude, wavelength, period)

Runs when either amplitude is at least `DAT_710015aa98`: one opaque full-canvas pass of program `0x2d`, uniforms
`(p76/1920, p79/1080, 1080/p77, 1920/p80)` (a zero wavelength gives 0) and the phases `+0x4c`, `+0x50` (advanced as
described under Ripple). Per pixel:
`x' = uv.x + p76/1920 × sin(2π (uv.y × 1080/p77 + phase₄c))` and
`y' = uv.y + p79/1080 × sin(2π (uv.x × 1920/p80 + phase₅₀))`: the first wave shifts rows sideways and runs down the
picture, the second shifts columns and runs across. This confirms the port's horizontal/vertical property names.
In the port: `effect_passes::apply_raster`, `wgsl/raster.wgsl`.

## 12. Story choices **confirmed** (compared with the original on screen)

### The command

`shin::ADV::SELECT` is built by the ADV's listener slot 14 (`0x7100000d90`) and executed by `0x7100005ba0`. On its
first run it collects the option strings, builds a mask of options picked before (bit `i` set when bit
`choice_set_base + i` of the global seen-choices set at `PTR_DAT_71002264f0 + 0x38` is set) and creates a
`SelectLayer` (`0x71000f0070` → `0x71000f00f0`) with `choice_visibility_mask` and that mask. Every frame after, once
the layer has reported a pick, it sets that seen bit, stores `pick + 1` at `choice_index` of the choice table
(`+0x50`), writes the pick to `dest` and finishes. The 5 story choices (§10) use `choice_visibility_mask` 4095 or the
register `$v38`.

### The layer (`shin::SelectLayer`, vtable `0x7100222e58`)

- **Options:** one per set bit of the visibility mask, at most 12; bit 12 shuffles them (minstd_rand from
  `shin::randomSeed`). Each is a text object (below) laid out 1000 px wide at size 48 with a 3 px outline, and
  tinted (0.75, 0.75, 1, 1) when picked before, white otherwise.
- **Layout:** bar height = text block height + 16 (the block includes the outline on both sides); bars stacked
  32 px apart; the stack starts at `(1080 − total) × 0.35`. Text: anchored at its top centre at x = 960, 8 px below
  the bar's top.
- **Bar** (`0x71000f1280`): the `select` texture of `msgtex.txa` (1200 × 80) from x 360 to 1560, in three slices: an
  8 px cap from v 0–0.1 above the bar, the body (v 0.1–0.9) and an 8 px cap (v 0.9–1) below, drawn with blend 2 in
  white at the option's alpha. The highlight and the pick flash draw the same shape with `select_cur`, at
  alpha × hover × 0.5 and at the flash value.
- **Animation** (update `0x71000f0eb0`, states through a jump table at `0x710017504a`): the whole menu fades in by
  0.1 per frame, then (state 0) each option, 4 frames after the previous one, slides from x −300 to 0 (sine-out)
  and fades in over 20 frames, with the system sound `select`; state 1 waits for that; state 2 takes input. After
  a pick (state 3, once the flash is over) the other options slide to +300 (sine-in) and fade over 20 frames,
  4 frames apart, and the picked one fades over 30 frames after them; state 4 waits, then the pick is reported.
- **Input** (`0x71000f1740`, only in state 2 with the menu fully shown): up/down move the cursor with
  wrap-around and the sound `move`; the old highlight fades out at 0.1 per frame (10 frames), the new one in at 0.25 (4 frames). Confirm plays
  `seldecide` and flashes the option to 0.7 and back at 0.2 per frame (3.5 frames each way); with no cursor it plays `error`. Touch/mouse:
  a bar is hit between x 460 and 1460 (`0x71000f1940`); pressing on it moves the cursor and raises the flash,
  releasing over it picks, releasing elsewhere drops the flash (`0x71000f19e0`, `0x71000f1aa0`). The menu starts without a cursor unless a flag (`+0x79` bit 7) is set;
  played with a controller in Eden it starts without one.
- Tween easing codes as elsewhere (0 linear, 1 sine-in, 2 sine-out); `0x80` holds the current value for the
  duration (a delay); `0x40` makes the duration argument a rate per frame: the step lasts |target − start| / rate frames
  (`0x71000bb620`; corrected November 2026, it was read as seconds before). The low 6 bits of the flags pick the
  easing; `0x80` makes the target relative (target 0 is a hold), `0x100` restarts from the current value, `0x1000`
  marks a point where skipping stops fast-forwarding.

### The text object (constructor `0x7100100ea0`, layout `0x71001011d0` → `0x7100101360`, draw `0x7100101a90`)

Position, anchor (fraction of its size; the menu uses (0.5, 0)), scale, two text colours and an outline colour.
Layout parameters `(width, size, ?, outline mode)`: outline mode 1 draws a 3 px outline and pads the block by
3 px on every side (mode 2: 1.5 px, as in the message box); horizontal font scale 1.0. Unlike the message box,
the first line is not a speaker's name. In the port: `layer/text_block.rs` (`TextBlock`), `layer/select_layer.rs`,
`adv/command/select.rs`.

## 13. The quiz and the other minigames **confirmed** (behaviour from the code; not yet compared on screen)

### Layer type 9 is a family of minigames

The quiz layer loader (`shin::QuizLayerLoader`, build `0x7100084900`) switches on the first load parameter:

| Kind | Class | Constructor | Uses in main8 |
|---|---|---|---|
| 0 | `QuizLayer` | `0x71000c7640` → `0x71000c76a0` | 16 questions (`0xa49444`–`0xa6f189`), mask from `$v29` |
| 1 | `CakeLayer` | `0x7100043ef0` → `0x7100043f40` | once, `0xa44d3b` (the answer is ignored) |
| 2 | `MurdererLayer` | `0x710009a890` → `0x710009a8f0` | at `0xa96b4d`, parameter `$v36`; the answer indexes a jump table of scenes and the script loads it again, counting attempts in `$v37` and leaving after 100 (`0xaaa67e`) |
| 3 | `LastLayer` | `0x710006d280` → `0x710006d2d0` | once, `0xb59125`; the script branches on answer 0 |

Each comes with an `*OverlayLayer` that takes the input while a `QUIZ` command waits on it. The script always
loads them at layer 17, runs `QUIZ $v1, 17`, then `PAGEBACK` and `LAYERUNLOAD 17`.

`LAYERLOAD` (`0x7100008770`) treats a quiz-type layer loaded over another quiz-type layer as "already the same"
whatever the parameters, and clears the layerbank's completed flag (`+0x3f84`); the script unloads the layer
between questions anyway.

### The command (`shin::ADV::QUIZ`, constructor `0x710000dcc0`, vtable `0x71001d3700`)

- The argument is a layer id on the current plane (resolved to a layerbank by `0x71000201f0`).
- Execute (`0x710000e020`): on the first run it finds the layer, `dynamic_cast`s it to the quiz layer base and
  installs the layer's overlay (vfunc `0x78`) as the input handler, takes a save snapshot (`0x710000f720`) and
  clears an ADV flag (`+0x480`). Every run after, it asks the layer for its result (vfunc `0x80`: a done flag and
  a value). Once done it writes the value to `dest`, records it in the save's command history (`+0x2da0`, 6-bit
  entries), stores it with a completed flag in the layerbank (`+0x3f84`/`+0x3f88`) and sets the ADV flag again.
- Replay: when a save made before the quiz is loaded, the command reads the recorded answer from the history
  instead of asking (`0x710000df50`, until the command counter `+0x2db0` catches up with the save's).

### The quiz (`shin::QuizLayer`, kind 0)

- **Textures:** `/quiz.txa` with `button` (256 × 384, answers A–F as 64 px rows) and `consult` (768 × 176, the
  "Ask Auntie Eva!" button in two 368 px frames side by side). The English patch replaces the archive.
- **Buttons:** a static table (`0x710022d108`, filled by `0x71000c9710`) of 7 entries `(texture, u, v, w, h)`:
  rows 0–5 of `button`, then `consult`. Load parameter 2 is a mask; bit `i` shows entry `i`, and the quiz answers
  with that bit index (0–5 = A–F, 6 = Ask).
- **Layout** (constructor): a running top starting at y 288 (`0x710015aa58`); each visible button is centred at
  x 240 (`0x710015a640`) with its top 4 px below the running top, which then moves down by its height + 8.
  Coordinates are 1920 × 1080 top-left, drawn through the layer's transform after a (−960, −540) translation.
- **Draw** (`0x71000c8130`): blend 2, one quad per button with program 5 (below), vertex colour white, first tint
  white at alpha hover × 0.5, second tint white at alpha flash. While the blink phase is ≥ 0.5, the consult
  button shows its second frame (u + w).
- **Update** (`0x71000c8070`): advances each button's two tweeners; the blink phase advances by
  frames / 60 (`0x710015a778`), so the frames swap every half second. Once picked and the flash tween has ended,
  the state becomes done.
- **Input** (`QuizOverlayLayer`, keys `0x71000c8910`, only while waiting): the cursor starts on the first button
  with its hover at 1. Up/down move it with wrap-around and the system sound `move`; the old hover fades to 0 in
  0.05 per frame (`0x710015a704`, 20 frames), the new one to 1 at 0.25 (4 frames). Confirm plays `decide`, flashes the button to 0.7 and back
  at 0.2 per frame (`0x710015a584`, `0x710015a954`; 3.5 frames each way) and marks it picked. Touch/mouse (`0x71000c8ad0`–`0x71000c8e00`):
  a press on a button moves the cursor there and flashes it up, releasing on it picks it, leaving cancels the
  flash.
- **Result** (`0x71000c85d0`): the picked entry's bit index, done when the state is done.

### Program 5

Fragment: `mix(mix(texture.rgb, tint1.rgb, tint1.a), tint2.rgb, tint2.a) × vertex.rgb`, alpha
`texture.a × vertex.a`. shin had declared it as `Button` without a shader; the port's is
`shin-render-shaders/wgsl/button.wgsl`.

In the port: `layer/user/quiz_layer.rs`, `adv/command/quiz.rs`, with what the minigames share in
`layer/user/minigame.rs`. Mouse input is not wired yet (as for the choices).

### What the minigames share

- Each has an `*OverlayLayer` input handler that reads the controller state (`0x71000c4900`) once per frame: held
  directions in byte 0 (bits 0–3 up, down, left, right), confirm in byte 7 bit 4, repeating direction presses in
  byte 11 (bits 0–3), the left stick at `+0x10`/`+0x14`.
- They draw with blend 2 and `setLayerDepthStencil(ref, 1, 0)`: the second argument lets equal stencil values pass,
  so their overlapping sprites all draw (with the strict test only the first sprite of each spot survives).
- The pick flash is shared: to 0.7 and back at 0.2 per frame, through program 5's tint.

### The cake game (`shin::CakeLayer`, kind 1) **confirmed** (seen in the port, not yet compared with the original)

- **Textures:** `/cake.txa` (not replaced by the English patch, so its banner stays Japanese): `base` (the whole
  cake, drawn at (360, 0)–(1560, 1080)), slices `00`–`14`, `msg` (the "choose a cake" banner, (616, 920)–(1304, 1016))
  and `finger` (96 × 96, drawn at the pointer − 16).
- **Slices:** a static table (`0x710022cbd8`, filled by `0x7100046c40`) of 15 entries `(name, x, y, w, h)`; each
  slice's sprite is drawn at (360 + x, y), w × h, only while its hover alpha is above 0, with alpha = hover and the
  pick flash as tint. Slice 11's table width (432) is smaller than its texture (480): the sprite is squeezed.
- **Pointer:** starts at (1440, 900); 8 px per frame per held direction plus the stick × 12; clamped to x 28–1863,
  y 38–1015. The finger and banner share one alpha tweener (starts at 1).
- **Hit test** (`0x7100044fe0`): a 4-bit map of 240 × 240 cells of 4 px (`0x71001675e9`) over x 480–1440, y 0–960;
  each cell holds slice + 1 (0 = none). The map is hand-made (wedges on the top, the sides below), not the sprites'
  alpha: the port rebuilds it from the sprites' opaque pixels with later slices on top, which matches 98.5 % of the
  cells (the rest are edges).
- **Input** (`0x7100045da0`): moving changes the hovered slice (old hover → 0 at 0.05 per frame, new → 1 at 0.25).
  Confirm on a slice plays `decide`, flashes it and fades the finger and banner at 0.1 per frame; confirm on nothing plays
  `error`. The answer is the slice's index (`0x7100045250`), once the flash is over.

### The last choice (`shin::LastLayer`, kind 3) **confirmed** (seen in the port, not yet compared with the original)

- **Textures:** `/last.txa` (English patch): `bg`, `front`, `select`, `back`, `shadow`, `plate`, `flash_door`, `obi`.
  Everything is drawn in centred coordinates, shifted horizontally by a pan tweener.
- **Front view:** `front` (full screen) with the two `select` halves at (−688, 276)–(−64, 452) and (64, 276)–(688, 452)
  and `obi` beyond the screen edges, all at the front view's alpha; each `select` half has its own flash.
- **Door views:** `bg` covers −1920…1920 (uvs 0–2, repeating, in the opaque pass). With the pan above 0 (left door):
  `back` right half at (−504, 276)–(−24, 452) with the back flash, `shadow` and the left half of `plate` at
  (−1248, 340)–(−624, 516) with the plate flash, and `flash_door` at (−1424, −492)–(−506, 352) twice: at alpha
  (sin(2π·phase) × 0.2 + 0.2) × door alpha (phase advances 0.01 per frame, `0x710015a928`), then at the plate flash.
  The right door mirrors all of it (x negated, uvs swapped).
- **Input** (`0x710006ff60`, only while the pan and the front fade are idle): on the front view, left/right plays
  `move`, flashes that plate and starts the pan to ±960 (20 frames, sine-out), the front view's fade to 0 and the
  door view's to 1 (20 frames, linear). On a door view, confirm plays `decide`, fades the door view in 5 frames and
  flashes the plate; the opposite direction flashes `back` and returns to the front view. The pan and fades wait
  while a select or back flash runs.
- **Answer** (`0x710006f870`): 0 for the left door, 1 for the right, once the plate flash is over.

In the port: `layer/user/cake_layer.rs`, `layer/user/last_layer.rs`.

### The culprit game (`shin::MurdererLayer`, kind 2) **confirmed** (seen in the port, not yet compared with the original)

A small application of six screens (`shin::MurdererLayer::{TopMenu, ReplayMenu, Rule, HintMenu, Check, Select}`),
all drawn from `/murderer.txa` in 1920 × 1080 top-left space. The load parameter (`$v36`) is the number of wrong
answers so far; the script loads the game again after each scene it answers with.

- **Layer** (constructor `0x710009a8f0`, update `0x710009b090`): the current screen draws into a render texture in
  two passes (background, then foreground; `0x710009b240`), cleared to opaque black, with the layer's transform
  applied inside. A screen hands over through `beginTransition` (`0x710008cac0`): it renders itself into a second
  texture as the snapshot, installs a wiper and names the next screen, which replaces it at the start of the next
  update. The wipers are the engine's own: `DefaultWiper` (a linear `mix(old, new, p)` crossfade, program `0x24`)
  for 30 frames between screens, 10 frames for the face changes, and the 60-frame `GlassWiper` (§7) into the
  culprit choice. Keys reach no screen while a wiper runs.
- **Answer** (`0x710009b6e0`): a replayed episode 0–5, a hint 6–26, or the culprit choice: 27 when right,
  28 + min(attempts, 20) when wrong. The layer's restore (`0x710009b700`, for save data) maps the same ranges back to
  the ReplayMenu, HintMenu and Select screens.
- **TopMenu** (`0x710008be00`, argument = cursor): Beatrice in four pieces on the left (her face changes before the
  culprit choice) and six plates at (1000, 441 + 96i), 704 × 94, coloured from (0.2157, 0.1059, 0.1059) to
  (1, 0.1608, 0.1608) by the hover, labels from `topmenu` rows; the pick flash whitens the plate untextured (program
  1). Up/down wrap. Items: re-read the story, the rules, purple statements by person, by chapter, hints, and the
  culprit choice (which plays `execute` and shatters into Select).
- **ReplayMenu** (`0x710008dad0`): six `thumb_*_s` in a 3 × 2 grid from (124, 193), 564 × 326 apart, wrapping in both
  directions; a pick holds 30 frames, then zooms the thumbnail to full screen over 60 frames (sine-out) while
  crossfading to the large `thumb_*` (program 6), and answers its index.
- **Rule** (`0x710008f8c0`): `rule_text` in a nine-slice `right` frame; only cancel (back) responds.
- **HintMenu** (`0x71000906b0`): hints 0…attempts in rows 90 px apart, clipped to (280, 180)–(1640, 900); the newest
  shows "??????????" (`hint_text` row 0), the cursor starts on it; up/down without wrap, scrolling the row into view
  over 4 frames; a pick answers index + 6 after 60 frames. The scroll bar is the engine's (`0x71000ef580`, the
  `scroll` texture of `systex.txa`).
- **Check** (`0x7100093420`, argument = mode): a viewer, never answers. Mode 0 is the family tree (the table at
  `0x710017224b`: line tiles and people with culprit and victim bits); victims are tinted 0xffff0f0f and skipped by
  the cursor; picking a person shows `/chars/m_<name>.txa` at 25 % and their testimony. Mode 1 lists 23 pieces of
  evidence under six chapter headers that stick to the top. The cursor moves in 2D by the lowest |D| / cos⁴ to the
  candidates (no wrap). The testimony and evidence texts are strings in the executable (`0x71001db528`, 17 pointers by
  person; `0x71001db270`, 29 entries of index, flags, title, body), which the English patch's IPS translates (§3).
  The text object (`0x7100094480`, laid out by `TextObject::layout` `0x7100101360` with the message layouter) is
  1030 px wide with a 3 px outline at (723, 180), size 32, no line padding, and room for rubi above every line (rubi
  size 0 means 0.4 × the size), so lines are 48 px apart; it breaks between any two letters, mid-word in English
  (compared in Eden); clipped to (146, 180)–(1774, 900); the right stick scrolls it 32 px per frame and, in mode 0, the hint "Scroll" shows when it
  is taller than 720 px. The port reads the strings from the player's own `main` and `.ips`.
- **Select** (`0x7100097930`): the tree with every person (28 × 30 cells), Battler on the right in three poses.
  Confirm toggles a person as a culprit (the `murderer` mark), and "Confirm" (`tokutei`) submits with `attack`: a
  white flash fading over 30 frames, 60 frames, and when wrong `ahaha`, a 10-frame crossfade to Battler's troubled
  face and 60 more frames. The answer is right only when exactly Rudolf, Kyrie and Battler are marked.

In the port: `layer/user/murderer/` (one file per screen, `tree.rs` for the table and the 2D cursor). Debug
fast-forwarding goes straight to Select and marks the right answer.

### Text colours

`@cRGB` in a message sets the colour from three decimal digits in the order red, green, blue, each mapped to
0, 0x1c, 0x39, 0x55, 0x71, 0x8e, 0xaa, 0xc6, 0xe3, 0xff: `@c649`, the purple of the testimonies and of purple truths,
is (170, 113, 255) in the original (measured in Eden). shin read the digits in reverse (blue, green, red), which drew
it pink; fixed in `shin-primitives` `UnormColor::from_decimal_rgb`. **confirmed**

### Mouse and touch

The minigames and the choice menu take the touch screen through their overlay layers' handlers: slot 18 hit-tests a
point (1920 x 1080, top left) and remembers what is under it, 19 is the pointer leaving, 20 a press on the hit, 21 the
release over it (activate) and 22 a release elsewhere (undo the press). The dispatcher that calls them was not
decompiled; the port calls them like a button (hit-tests while the button is up, a press holds its target until the
release) **inferred**. In the port: `PointerTracker` in `layer/user/minigame.rs`; the window maps the mouse into the
canvas (`shin-window`).

---

## 14. Saving and loading **confirmed** (code; Switch saves load in the port)

A save does not hold the scene. It holds how to get there: the scenario (`scenario_id`, the VM's initial `$v0`), the
VM's first random seed, a **save-point counter** value (`save_position`) and the **route**: every SGET, SELECT and
QUIZ result in execution order. Loading replays the script to that counter without drawing anything, then builds the
scene from the scene model the commands keep. Addresses below are in `shin::ADV::Context` (`+0x2530..`).

**The counter** (`+0x2db0`) counts save-point commands as they start (their constructor, not their execute):
MSGSET unless an EVBEGIN..EVEND event plays; AUTOSAVE; EVBEGIN; EVEND; RESUMESET (unused); SELECT and QUIZ when no
line is still shown (`+0x90f0`, set by MSGSET, cleared by a MSGCLOSE after a waited line: MSGSET's wait flag or
`MSGWAIT -1`). An answered SELECT or QUIZ counts once more. Before counting, the pending SSETs reach the persistent
data (`0x710000f800`); SSET only queues (`+0x9158`), and SGET reads the queue first.

**The snapshot** (`0x710000f720`) makes the current counter the save point and copies the slot info into the
in-memory auto slot: when a MSGSET's text is handed to the message box, when a SELECT or QUIZ opens and when it is
answered, and at AUTOSAVE. AUTOSAVE writes no file; the auto slot reaches the disk with the next write.

**The route** is a bit stream, most significant bit first, grown in 1024-byte steps on read and write: SGET writes
`1, v` for 0 or 1, else `0` and 16 bits; SELECT 4 bits; QUIZ 6 bits. Random results are not in it: the VM's
generator steps once per instruction from the saved seed (`state = state * 0x343fd + 0x269ec3`), and `rnd a, b`
gives `min + ((n * (state >> 8)) >> 24)` with `n = |a - b| + 1` (`0x71000ed3c0`), so a replay draws the same numbers.

**Replay** (`0x710001b340`) runs the VM calling each command's slot 2 instead of its execute: almost every command is
skipped (its constructor already updated the scene model); SGET, SELECT and QUIZ take the route's value; SSETs before
the save point are skipped (they happened); the save-point commands stop once the counter reaches the save point, and
that command then runs live (normally the MSGSET whose line was on screen). A save made inside an event, or a replay
that reaches EXIT, falls back to the last save point before it (`+0x2dc8`, `0x710000f210`).

**The file** is one file, `savedata:/SAVEDATA` (format in `shin-core/src/format/save`): version byte 1, play
seconds, the persistent data, the seen-message / seen-choice / last-choice / CG / BGM / movie sets, the settings, the
auto slot and 100 manual slots. It is written only at EXIT, when returning to the title, when closing the config with
changed settings, and on a manual save or delete (`0x71000d52f0`). A version-0 file is a fresh save. Defaults:
`0x71000d2110`.

In the port: `adv/vm_state/save.rs` (counter, route, pending SSETs, snapshot), `adv/command/mod.rs`
(`replay_command`), `Adv::new` (the replay and its fallback), `adv/restore.rs` (the scene rebuild: layers, masks,
group properties, BGM, looping sounds) and `savedata.rs` (the file, by default `%APPDATA%\shin\SAVEDATA`).
The save and load screens (section 15) save and load; F5 still saves to the slot the menu would open at, and
`--load auto` or `--load <1-100>` loads at start. shortcuts: the rebuild starts on a black screen instead of the 0.3 s
cross-fade, and does not restore a minigame's answer or the message box style. The user's Switch save (22 h, Tsubasa Story03, save points 316 and 182) replays in 40–80 ms and resumes
on a line; that it is the same line as on the Switch is still to be checked.

## 15. Scenes, menus and the story's controls **confirmed** (compared with the original on screen)

**Scenes.** The game runs one scene at a time and switches with a crossfade from a snapshot of the old screen:
FirstBoot (black while the system assets and the save load; its logo and caution screens are dead code in v1.0.3) ->
TitleDemo (the opening movie, movie table entry 0, `sakucs_op`; confirm, + or a tap skips with a 90-frame fade of
the picture and the sound) -> Title (`CreateTitle(-1)`: 90-frame fade and the background's blur clearing; `(0)` when
coming back from the scenario select: 60 frames, everything shown at once) -> ScenarioSelect -> the bridge -> ADV.
The system menu's "Title" writes the save and goes to `CreateTitle(-1)`.

**Tasks.** A scene owns a task pool (`ADV+0x158`, add `0x7100100930`): every task updates each frame, the top one
has the input, and the ADV's own buttons only work while the pool is empty. A menu that opens another reports back
through a callback with 0 when its close starts and 1 when it has ended; the shared dialog does the same with
`(event, button)`, button -1 for cancel.

**The dialog** (`0x71000223d0`): message (system.fnt, size 54, width 1440, line spacing 3, no room for rubi) over a
0.5 black dim, buttons of size 44 in a row (or a column when a row would not fit in 0.75 of the width), at least 600
wide, grown from 1.4 to 1 (sine-out) and faded in over 12 frames; the cursor starts on No. Colours, 9-slice tables
and the button layout are in the code (`adv/menu/dialog.rs`).

**The system menu** (Y, `0x710001c100`): pauses the script, voices and sounds, hides the message window and banners,
and shows a blurred capture of the scene (two passes of the blur program at full size, taps 0.75/2.25/3.75/5.25 px)
drawn at **half brightness**, colour (0.5a, 0.5a, 0.5a, a), premultiplied while it fades in over 10 frames and
opaque once up (`0x71000126b4`; measured 0.49 in Eden). Characters shows only once CHARS has run (its state starts
below 0); Tips only with tips. Panels: scene title and play time, size 38 without rubi room. B (cancel) hides the
window until any button (`0x7100013720`), pausing nothing.

**Config** (`0x71000586b0`, `/config.txa`): 3 tabs, 14 items (table in `adv/menu/config_menu.rs`); settings apply
live, Y resets to the defaults after a dialog, closing writes the save when anything changed.

**Save and load** (`0x71000d5b40`, `/saveload.txa`): 100 bookmarks 186 px apart in a 730 px view; each visible
bookmark replays in the background to show its SAVEINFO levels 0 and 1 and a 288x162 thumbnail of its scene (the
layers loaded into a screen layer of their own, rendered and halved twice). Saving and removing ask first and write
the file; loading starts a new ADV.

**Title** (`0x710010b130`, `/title.txa`): background, BGM and volume follow persist[0] (table `0x7100187796`); the
items depend on persist data and the auto slot; Continue fades everything and the music over 90 frames and starts
the auto slot. **Scenario select** (`0x71000dc8f0`, `/snrsel.txa`): tabs show once one of their pieces is open; a
piece's state is its persist slot (scenario + 1; Episode 1 always at least 2); read pieces ask "Start the game?";
+ unlocks the main episodes. **The bridge** (`0x71000e3a20`): the last screen as two doors hinged on the outer
edges, perspective camera (fovy 2 atan 0.54, 1000 px away, moving 500 px in), opening over 180 frames onto white;
the new game starts at frame 360 with a fresh seed.

**The story's controls** (`0x7100011f08`, system enabled, auto and skip off): Y system menu, B hide window (inside
an event: skip the event), + "Skip this chapter?", - "Return to the last chapter?", Up roll back one line. The
jumps and event skip **look ahead**: replay to the current line, then keep running the commands a jump passes (slot
3) until one stops it: for +, the next AUTOSAVE (chapter), choice or minigame, or with setting v11 an unread line;
for event skip, the event's EVEND; EXIT means nothing to jump to ("error"). The screen is then rebuilt at that save
point as a load. "Return to the last chapter" goes to the current chapter's AUTOSAVE once more than one of its lines
has played, else the one before (`+0x2dbc[(+0x2dd0 > 1)]`); rollback goes to the previous line's save point
(`+0x2db4`) once two lines have played. With v11 on, the + dialog shows a Japanese text the English patch does not
translate (`0x710015eedb`).

In the port: `app.rs` (scenes), `title_demo.rs`, `title.rs`, `scenario_select.rs`, `adv/menu/` (dialog, system menu,
hidden window, config, save menu, thumbnails, jump dialog, backlog) and `ui/` (drawing and input helpers).

**The backlog** (X, `0x71000a20b0`, `/logview.txa`): the LogBuilder replays the current chapter from its start through
the current line and on through read text (stopping at an unread line with v11, a choice or minigame not played,
or EXIT), collecting lines (colour 999), choices `@r《prompt》@r option` (977), chapter titles from SAVEINFO 0 and 1
at each AUTOSAVE (799) and minigames (977). Lines are newrodin at size 40, 97 % width, 1480 wide, at least 76 tall
when voiced, plus 20, 14 px apart, the current one at 0. A replays a voiced line, L/R move between markers, Y asks
"Start reading from the selected line?" and rebuilds there.

shortcuts: no backlog chapter list, galleries, Characters, Tips or manual yet; the backlog's text starts at a guessed
x; the scenario select fades in instead of the curtain mask wipe;
the bridge has no bloom glow; the dialog fades its pieces instead of a render texture; the BGM player is per scene
instead of shared, so music does not carry across scenes. Keyboard: Y and X keys for the Y and X buttons, = and -
for + and -.
