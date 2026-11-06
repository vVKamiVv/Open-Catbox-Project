<div align="center">

# 🦋 Open Catbox Project 🦋

**Play *Umineko: The Symphony of Catbox and Dreams* natively on PC, in English.**

*A gameboard rebuilt piece by piece, so the witch's tea party can go on outside the console.*

</div>

---

*Umineko no Naku Koro ni Saku ~Nekobako to Musou no Koukyoukyoku~* was only ever released on consoles. The Open
Catbox Project is a PC engine for it: you bring your own Nintendo Switch copy of the game and the fan-made
[English patch](https://github.com/andOlga/umineko-catbox-english), and the port plays them directly on your
computer, with no emulator. Development and testing happen on Windows.

> [!NOTE]
> Work in progress: the story, its effects and the menus run, but not everything has been compared with the
> original yet. See [The Golden Land so far](#-the-golden-land-so-far).

## 🟥 Spoken in red

These are not theories. They hold, without exception:

- $\color{red}{\textsf{No game data, keys or decompiled code are in this repository.}}$
- $\color{red}{\textsf{You need to own the game to play it.}}$

## 📜 Rules of the gameboard

1. **Faithful by construction.** Timings, constants, formulas and shaders are read out of the original game's
   executable through reverse engineering rather than estimated by eye; where the code leaves a gap, the comments
   say so. Screens are then compared side by side with the original running on an emulator.
2. **Documented in the open.** Nobody had written down how this engine works. Everything learned goes into
   [`research/`](research/): behaviour, data formats and original addresses, plus the scripts used to find them, so
   anyone can check the work or build on it.
3. **Built on [shin](https://github.com/DCNick3/shin).** DCNick3's open-source re-implementation of Entergram's
   engine is the starting point; it is developed further here as an independent project. See [Credits](#-credits).

## 🦋 The Golden Land so far

| Piece | State |
|---|---|
| Text, sprites, backgrounds, music, sound effects | Working |
| Voices, with panning and lip-sync | Working |
| English patch images | Working |
| Movies, including transparent effect movies | Working (Windows decodes them itself; elsewhere through ffmpeg) |
| Rain, snow, petals and golden snow | Working |
| Screen transitions (crossfade, mask, whirl, glass shatter) | Working; whirl and glass not yet compared with the original |
| Layer effects (dissolve, blur, ripple, mosaic, raster) | Working; not yet all compared with the original |
| Every script command the game uses | Working |
| Story choices, the quiz and the main8 minigames | Working (the culprit game's testimonies come from the translation's `exefs` patch: keep that folder next to the assets or pass `--exefs`) |
| Title, scenario select, save/load, settings, backlog, system menu | Working, compared with the original |
| Tips, Characters, manual, Picture/Music/Movie/Character Box | Working, compared with the original |
| Mouse: clicking, dragging and flicking lists, as the touch screen does | Working on every menu |
| Controllers | Working |

The full inventory of what the game's script needs was made by statically analysing the whole script; it lives in
[`research/docs/engine-internals.md`](research/docs/engine-internals.md) §10, and the plan for the rest in
[`research/docs/alignment-plan.md`](research/docs/alignment-plan.md).

## 🍰 Preparing the tea party

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
3. On Linux and macOS, **ffmpeg** on your `PATH`, for the movies. Windows decodes them itself (Media Foundation)
   and only falls back to ffmpeg on editions without it (the "N" editions lacking the Media Feature Pack).

Then build and run (requires [Rust](https://www.rust-lang.org/)):

```bash
cargo build --release -p shin
```

Point the engine at your `assets` folder with the `SHIN_ASSETS` environment variable, or place the folder next to
the executable. Use the release build: the unoptimised debug build overflows the main thread's stack at startup.
Set `RUST_LOG` (for example `warn,shin=debug,shin_video=debug`) to see what the engine is doing. Your Switch save
(`SAVEDATA`) can be used too: see `--savedata` in `shin --help`.

## 🎮 Moving the pieces

| Keyboard / mouse | Switch controller | Does |
|---|---|---|
| Click, Space, Enter | A | Advance, choose |
| Backspace, Escape, right click | B | Back; hide the text box; skip an event |
| Y | Y | System menu |
| X | X | Backlog |
| `-` / `=` | − / + | Previous / next chapter |
| A / S | L / R | Auto mode / skip mode |
| Hold left Ctrl | | Fast-forward |
| Up arrow, mouse wheel | Up | Roll back / scroll |
| Drag and flick | Touch | Scroll lists, turn pages, pan pictures |
| F5 | | Save to the last used bookmark |
| F11 | | Fullscreen |

For working on the port, `--dev` adds debug overlays (F3: FPS, script state, every layer and its animations) and
time control (F6 pause, F7 one frame, F8/F9 slower/faster, F10 normal speed).

## 🗝️ Inside the mansion

| Path | Contents |
|---|---|
| `shin/` | The game engine (the executable keeps shin's name for now) |
| `shin-core/` | Reading and writing the game's data formats |
| `shin-render*`, `shin-audio`, `shin-video`, `shin-tasks`, `shin-window`, `shin-input`, `shin-primitives`, `shin-derive` | Rendering, audio, video, threading, windowing, input and shared building blocks |
| `sdu/`, `shin-asm/` | Command-line tool to extract and convert game data (`.rom`, `.pic`, `.bup`, `.nxa`, `.snr`, `.fnt`, `.txa`), and an assembler/disassembler for the game's script bytecode |
| `research/` | Reverse-engineering notes, Ghidra scripts and the symbol map of the original executable |

## 🌹 Credits

This project stands on the work of others, all named in [CREDITS.md](CREDITS.md):

- **[shin](https://github.com/DCNick3/shin)** by DCNick3 and its contributors: the engine this project grew from.
- **[umineko-catbox-english](https://github.com/andOlga/umineko-catbox-english)** by andOlga and contributors: the
  English translation this port is made to play.
- **07th Expansion** and **Entergram**, for the game itself.

## ⚖️ License

[Mozilla Public License 2.0](LICENSE), the same license as shin.

Not affiliated with Entergram, 07th Expansion, the shin project or the translation team.
