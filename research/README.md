# Research: how the original engine works

Notes and tools from reverse engineering the Nintendo Switch release of *Umineko no Naku Koro ni Saku ~Nekobako to
Musou no Koukyoukyoku~* (title `01006A300BA2C000`, executable v1.0.3). Every engine value the port uses is taken
from the original executable; these files record how, so anyone can check or extend the work.

| Path | What it is |
|---|---|
| `docs/engine-internals.md` | Findings: formats, layers, transitions, script commands, menus, saving, voices, touch, with original addresses; §18-§21 record what the port adds |
| `docs/architecture.md` | How the port is put together: the crates, one frame from input to present, assets, saving, threads |
| `docs/coverage.md` | Every crate and subsystem, and where it is documented |
| `docs/launcher.md` | The launcher step by step: setup from the dump, languages, installing, updating, uninstalling |
| `docs/alignment-plan.md` | The audit's findings and what was done about them, the effects compared with the original, and the port's measured accuracy |
| `docs/audit/` | The November 2026 audit's four reports (commands, layers, text, systems), with addresses for every claim |
| `docs/reverse-engineering-guide.md` | Setup (Ghidra + SwitchLoader, headless scripts, shader decompilation) and workflow |
| `docs/class-inventory.md` | Every RTTI class of the executable (generated) |
| `tools/ghidra_scripts/` | Headless Ghidra scripts (decompile, xrefs, data dumps, symbol import, shader extraction) |
| `tools/re/` | Builds the symbol map and the class inventory |
| `tools/compare/` | `eden_capture.py`: captures lines in the original running on Eden, for side-by-side comparisons with the port's `--capture-messages` |
| `re/manual_symbols.csv` | **Hand-maintained** names for every identified function and constant |
| `re/rtti_inventory.tsv` | RTTI dump that the symbol map is built from |

Paths in the docs are relative to this folder.

## What is deliberately not here

- No game data, keys or executables: bring your own dump of a game you own.
- No decompiled code: the port re-implements behaviour, and the docs describe behaviour and addresses only.
- No Ghidra database or extracted shaders: both are derived from the executable; the guide shows how to make them.
