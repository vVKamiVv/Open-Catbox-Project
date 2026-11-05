# Open Catbox Project

**Play *Umineko: The Symphony of Catbox and Dreams* natively on PC, in English.**

*Umineko no Naku Koro ni Saku ~Nekobako to Musou no Koukyoukyoku~* was only released on consoles. The Open Catbox
Project is a from-scratch PC engine for it: you supply your own Nintendo Switch copy of the game and the fan-made
[English patch](https://github.com/andOlga/umineko-catbox-english), and the port runs them directly on your computer,
with no emulator. Development and testing happen on Windows.

> [!NOTE]
> Work in progress: the story runs, but not every effect and screen exists yet. See [Progress](#progress).

## How it is made

- **Faithful by construction.** Every timing, constant, formula and shader is read out of the original game's
  executable through reverse engineering, never estimated by eye.
- **Documented in the open.** Nobody had written down how this engine works. Everything we learn goes into
  [`research/`](research/): behaviour, data formats and original addresses, plus the scripts used to find them, so
  anyone can verify the work or build on it.
- **Built on [shin](https://github.com/DCNick3/shin).** DCNick3's open-source re-implementation of Entergram's engine
  is our starting point; we develop it further here as an independent project. See [Credits](#credits).
- **No game content included.** No game data, keys or decompiled code are in this repository.

## Progress

| Area | State |
|---|---|
| Text, character sprites, backgrounds, music, sound effects | Working |
| English patch images (delta-coded pictures) | Working |
| Movies | Working |
| Rain, snow, petals and golden snow | Working |
| Screen transitions (crossfade, mask, whirl, glass shatter) | Working; whirl and glass not yet compared with the original on screen |
| Every script command the game uses | Working |
| Layer effects | Dissolve, blur, ripple, mosaic and raster working (not yet compared with the original on screen); zoom blur and ghosting next |
| Story choices | Working (compared with the original on screen); keyboard and click, mouse hover planned |
| The quiz | Working (not yet compared with the original); keyboard and click, mouse hover planned |
| The cake, culprit and last minigames (main8) | Working (not yet compared with the original); keyboard only; the culprit game's testimony texts are missing |
| Voices | Planned |
| Title screen, save/load, settings, backlog and other menus | Planned |

The full list of what is missing was made by statically analysing the entire game script; it lives in
[`research/docs/engine-internals.md`](research/docs/engine-internals.md) §10.

## Getting started

You need:

1. **The game**: the romfs of your own dump of the Switch release (title id `01006A300BA2C000`), made for example with
   [nxdumptool](https://github.com/DarkMatterCore/nxdumptool) and extracted with
   [hactoolnet](https://github.com/Thealexbarney/LibHac):
   ```bash
   hactoolnet -t pfs0 "<your dump>.nsp" --romfs=assets
   ```
   `assets/data.rom` should have sha256 `6d90eb0bacacf769a7e4634407622b047acd711c47debb28136d7bab3fd0e591`.
2. **The English patch**: copy the `patch.rom` from
   [umineko-catbox-english](https://github.com/andOlga/umineko-catbox-english) into the same `assets` folder. The
   port layers it over `data.rom`, just like the console does.
3. **ffmpeg** on your `PATH`, for the movies.

Then build and run (requires [Rust](https://www.rust-lang.org/)):

```bash
cargo build --release -p shin
```

Point the engine at your `assets` folder with the `SHIN_ASSETS` environment variable, or place the folder next to
the executable. Use the release build: the unoptimised debug build overflows the main thread's stack at startup.
Set `RUST_LOG` (for example `warn,shin=debug,shin_video=debug`) to see what the engine is doing.

**Controls**: click, Space or Enter to advance · hold Ctrl to fast-forward · F3 debug overlay · F10 1080p window ·
F11 fullscreen · Escape or Q to quit.

## Inside the repository

| Path | Contents |
|---|---|
| `shin/` | The game engine (the executable keeps shin's name for now) |
| `shin-core/` | Reading and writing the game's data formats |
| `shin-render*`, `shin-audio`, `shin-video`, `shin-tasks`, `shin-window`, `shin-input`, `shin-primitives`, `shin-derive` | Rendering, audio, video, threading, windowing, input and shared building blocks |
| `sdu/`, `shin-asm/` | Command-line tool to extract and convert game data (`.rom`, `.pic`, `.bup`, `.nxa`, `.snr`, `.fnt`, `.txa`), and an assembler/disassembler for the game's script bytecode |
| `research/` | Reverse-engineering notes, Ghidra scripts and the symbol map of the original executable |

## Credits

This project stands on the work of others, all named in [CREDITS.md](CREDITS.md):

- **[shin](https://github.com/DCNick3/shin)** by DCNick3 and its contributors: the engine this project grew from.
- **[umineko-catbox-english](https://github.com/andOlga/umineko-catbox-english)** by andOlga and contributors: the
  English translation this port is made to play.
- **07th Expansion** and **Entergram**, for the game itself.

## License

[Mozilla Public License 2.0](LICENSE), the same license as shin.

Not affiliated with Entergram, 07th Expansion, the shin project or the translation team. You need to own the game to
play it.
