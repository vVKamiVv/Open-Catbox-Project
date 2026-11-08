# Reverse-engineering guide

How to work with the decompiled Switch executable of *Umineko no Naku Koro ni Saku* to find out how the original engine
does something, and how to feed that back into the PC port. Companion documents:

- [engine-internals.md](engine-internals.md): what has been figured out so far (formats, systems, open questions).
- [class-inventory.md](class-inventory.md): every C++ class in the executable, with typeinfo and vtable addresses.

## Ground rules

1. **Re-implement, don't copy.** Read the decompiled code to understand behaviour, then write your own code in the
   port. Never paste decompiler output into the port or the docs.
2. **Use your own dump.** Nothing from the game (code, assets, keys) is distributed; every step below starts from a
   copy you dumped yourself.
3. **Record what you learn.** Every finding goes into `engine-internals.md` with the original addresses, and every
   identified function or constant goes into `re/manual_symbols.csv`, so the next person starts where you stopped.
4. **Verify against the original.** Compare the same moment in the original (e.g. in an emulator, see below) and in
   the port before calling something done.

## Setup

| Tool | Where | Notes |
|---|---|---|
| hactool 1.4.0 | `tools/hactool` | Extracting the dump; see `engine-internals.md` §1 for the key-file quirks |
| Ghidra 12.1.2 | `tools/ghidra_12.1.2_PUBLIC` | Needs a JDK 21+ (`JAVA_HOME`) |
| Ghidra-Switch-Loader 1.6.1 | `Ghidra/Extensions/SwitchLoader` | Must match the Ghidra version exactly |
| Project scripts | `tools/ghidra_scripts`, `tools/re` | Described below |

Ghidra configuration used here (`support/launch.properties`): `application.settingsdir`, `cachedir` and `tempdir`
point to a folder **without spaces** outside `%APPDATA%`. Needed when Ghidra is started from a packaged (MSIX) app,
whose `%APPDATA%` writes are virtualized, which breaks Ghidra's OSGi script host; and because the launcher splits
paths containing spaces.

### Creating the Ghidra project

```bash
analyzeHeadless.bat "<project dir>" umineko -import "<dump>/exefs/main" -overwrite
analyzeHeadless.bat "<project dir>" umineko -process main -scriptPath tools/ghidra_scripts -postScript ApplySymbols.java re/symbols.csv
```

The first command imports and auto-analyzes (about 90 seconds); the second applies all known names, so the decompiler
shows `shin::RainLayer::step` instead of `FUN_71000c9df0`. Pass `-noanalysis` when re-running scripts on an already
analyzed project, and `-readOnly` for queries that shouldn't save anything.

**Windows gotcha:** `analyzeHeadless.bat` runs through `cmd.exe`, which treats `|`, `(`, `)` and `,` in arguments
specially. The project scripts therefore take plain space-separated arguments, never regexes.

## Scripts

All scripts write their result to the file named by their last argument.

| Script | Arguments | What it does |
|---|---|---|
| `DumpXrefs.java` | `<keyword>... <out>` | Finds strings containing any keyword, lists where they're referenced, decompiles the referencing functions |
| `DecompileAt.java` | `<out> <addr>...` | Decompiles the functions at the addresses, creating a function first if analysis left a bare label |
| `CreateMissingFunctions.java` | (none) | Creates the functions auto-analysis missed: every pointer into code found in the data (vtables, callbacks), every call target and every gap in the code that disassembles (2,604 more, 6,380 in all) |
| `DecompileAll.java` | `<out folder>` | Decompiles every function with the parallel decompiler into `<out>/index.tsv` and one `.c` per 0x10000-byte block: the whole engine, greppable (about 6.6 MB). Run it after `CreateMissingFunctions.java`, on a copy of the project so it does not lock the working one. Never commit its output |
| `Disasm.java` | `<count> <addr>... <out>` | Raw disassembly; needed when the decompiler hides register-only helpers |
| `Refs.java` | `<addr>... <out>` | References to an address plus the 8-byte pointers around it (for walking typeinfo and vtables) |
| `FindPtr.java` | `<hex value>... <out>` | Every place memory stores a pointer value (e.g. all vtables pointing at a typeinfo) |
| `DumpData.java` | `<addr>... <out>` | 16 bytes at each address as hex, floats, ints and string (constants, tables) |
| `DumpBytes.java` | `<addr> <length> <out>` | Raw bytes of a range to a binary file (tables too big for `DumpData`); game data, keep it out of the repository. Run it as the only `-postScript` of a Ghidra run |
| `RttiInventory.java` | `<out>` | Every RTTI class with typeinfo, base, vtables and virtual methods (TSV) |
| `ApplySymbols.java` | `<symbols.csv>` | Names functions and data from the symbol file, in C++ namespaces |
| `tools/re/make_symbols.py` | `<re dir>` | Builds `re/symbols.csv` from the RTTI inventory plus `re/manual_symbols.csv` |
| `tools/re/make_class_inventory.py` | `<re dir> <docs dir>` | Renders the RTTI inventory as `docs/class-inventory.md` |

### Symbol files

- `re/rtti_inventory.tsv`: generated; one row per class.
- `re/manual_symbols.csv`: **hand-maintained**; `address,name,comment` for everything identified by reading code.
  Add a row whenever you identify a function, constant or table.
- `re/symbols.csv`: generated from the two above; the input of `ApplySymbols.java`. Virtual methods that occur in
  exactly one class's primary vtable get a `Class::vfunc_N` name; methods shared between classes stay unnamed until
  someone identifies them by hand.

## Method: from a feature to its code

This is how the rain system was traced, and it generalises to any feature:

1. **Find a handle.** A file name, a texture name, a class name or a script command. Run `DumpXrefs.java` with a
   keyword. Asset paths often have no resolved reference (AArch64 builds addresses in two instructions), but class
   names from RTTI always lead somewhere.
2. **Go from class to methods.** In `class-inventory.md`, or with `Refs.java`/`FindPtr.java`: the typeinfo is
   referenced by the class's vtable(s); the vtable lists every virtual method. The constructor is the function
   that stores the vtable address (`Refs.java` on the vtable shows it).
3. **Read the interesting methods**: grep the whole-program export (`DecompileAll.java`, see Scripts) or decompile
   them one by one (`DecompileAt.java`), and dump the constants they load (`DumpData.java`).
   Typical layout: `update`, `render`, `step`/`spawn` helpers and a constructor whose parameters match the script
   command that creates the object.
4. **Read around decompiler blind spots.** Small helpers that only use registers decompile to an empty body; read
   them with `Disasm.java`, and read the caller's disassembly to see which registers hold what.
5. **Name what you found** in `re/manual_symbols.csv`, regenerate `symbols.csv`, re-apply.
6. **Write it up** in `engine-internals.md`: behaviour, constants, formulas, addresses.
7. **Re-implement in the port** from the write-up, with tests for the parts that are pure logic.
8. **Verify** the same moment in the original and the port (see below), measuring rather than eyeballing when the
   effect is random (mean minus median luminance over many frames).

## Shaders

The 53 GPU programs are embedded in the executable (see `engine-internals.md` §7 "Shaders"). To read one:

1. Extract: `-postScript ExtractShaders.java 71001de1c0 <program id>... <out dir>` writes
   `program_<id>_{vs,fs}.code` (GPU code) and `.ctrl` (control section) plus a header log.
2. Strip the 0x30-byte NVN header from the `.code` file (the Maxwell SPH starts at `0x30`).
3. Decompile: `tools/shadertools/Ryujinx.ShaderTools.exe <file> <out.glsl>` (built from the Ryubing fork of Ryujinx
   with the portable .NET SDK in `tools/dotnet`; set `DOTNET_ROOT` to that folder).
4. Constants that appear as `c1.data[N]` live in the literal pool right after the last instruction of the code
   section (look for the first float-looking words after the code).

Uniform blocks: the engine's `setUniform(slot, …)` slot N shows up as `c(N + 3)` in the decompiled GLSL; textures
bound to slot 0 and 1 appear as `tcb_8` and `tcb_A`.

### Adding a shader to the port

`shin-render-shaders/build.rs` turns every `wgsl/*.wgsl` into a program (`wiper_whirl.wgsl` → `WiperWhirl`,
`WiperWhirlBindings`; textures/samplers are paired by their `*_texture`/`*_sampler` names). It watches each shader
and the `wgsl/` folder itself, so adding or removing a `.wgsl` file regenerates the bindings on the next build.

## Finding the same moment in the port and the original

- **Port (shin):** run with `--fast-forward-to <script address>`; it fast-forwards and stops at that address, then
  runs normally. Script addresses come from a trace log (`RUST_LOG=shin_core::vm=trace` prints every instruction
  with its `pc`). Use an address that really starts an instruction, e.g. the `pc` of a `WAIT` that holds the shot
  you want.
- **Original (emulator):** title → *Start* → episode (never *Continue*, which jumps to the latest save), then the
  in-game chapter jump (*Plus*: next chapter, *Minus*: previous/current chapter start; confirm with *Yes*) and
  *R* to toggle text skip. Chapter starts are reliable sync points between the two.

### Capturing the same line of text in both

Lines of text are exact sync points: give both sides the script's message ids (as `sdu scenario disassemble` prints
them, `MSGSET { msg_id: 301, ... }`) and each saves a PNG per line once it has typed out and settled for 4 s.

- **Port:** `shin --capture-messages 297,301,420 --capture-dir captures/port` fast-forwards from the start, stops
  at each line, saves `<id>.png` at the canvas size (1920x1080 on a 100% scaled display) and quits after the last.
- **Original (Eden):** enable Settings > Debug > *Enable GDB Stub* (port 6543), then run
  `python -I research/tools/compare/eden_capture.py --messages 297,301,420 --out captures/original`, start
  Episode 1 (title → *Start*; the title cursor starts on *Continue*) and turn on skip (*R*). The tool breaks on the
  MSGSET constructor, turns skip off a few lines before each target, steps line by line, and saves Eden's own
  screenshot (5760x3240 at 3x scale). Restart Eden before each run. How it works around Eden's debugger is
  described in the script.

The game passes the script's message id minus one internally (engine-internals §10); both tools take and print the
script's ids. Expect differences from animation timing (blinks, the pulsing wait arrow) as well as real ones.

For a later episode add `--episode N`, so the port reaches the line through that episode's own set-up. To score a
pair for the port's accuracy figure, scale Eden's screenshot to 1920x1080 and take the share of pixels at most 32/255
away from the port's; [alignment-plan.md](alignment-plan.md#accuracy) lists every scored comparison and the formula.
