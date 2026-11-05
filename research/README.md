# Research: how the original engine works

Notes and tools from reverse engineering the Nintendo Switch release of *Umineko no Naku Koro ni Saku ~Nekobako to
Musou no Koukyoukyoku~* (title `01006A300BA2C000`, executable v1.0.3). Every engine value the port uses is taken
from the original executable; these files record how, so anyone can check or extend the work.

| Path | What it is |
|---|---|
| `docs/engine-internals.md` | Findings: formats, layers, transitions, script commands, with original addresses |
| `docs/reverse-engineering-guide.md` | Setup (Ghidra + SwitchLoader, headless scripts, shader decompilation) and workflow |
| `docs/class-inventory.md` | Every RTTI class of the executable (generated) |
| `tools/ghidra_scripts/` | Headless Ghidra scripts (decompile, xrefs, data dumps, symbol import, shader extraction) |
| `tools/re/` | Builds the symbol map and the class inventory |
| `re/manual_symbols.csv` | **Hand-maintained** names for every identified function and constant |
| `re/rtti_inventory.tsv` | RTTI dump that the symbol map is built from |

Paths in the docs are relative to this folder.

## What is deliberately not here

- No game data, keys or executables: bring your own dump of a game you own.
- No decompiled code: the port re-implements behaviour, and the docs describe behaviour and addresses only.
- No Ghidra database or extracted shaders: both are derived from the executable; the guide shows how to make them.
