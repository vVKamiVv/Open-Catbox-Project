<div align="center">

# 🦋 Open Catbox Project 🦋

**Play *Umineko: The Symphony of Catbox and Dreams* natively on PC, in English.**

*A gameboard rebuilt piece by piece, so the witch's tea party can go on outside the console.*

</div>

---

*Umineko no Naku Koro ni Saku ~Nekobako to Musou no Koukyoukyoku~* was only ever released on consoles. The Open
Catbox Project is a PC engine for it: you bring your own Nintendo Switch copy of the game and a fan-made English
translation, and the port plays them directly on your computer, with no emulator. Development and testing happen on
Windows.

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

<div align="center">

### 99.0 % like the original

</div>

The mean, over the 6 side-by-side comparisons with the original (running on an emulator) that have a per-pixel
figure, of the share of pixels within 32/255 of the original's: blur 99.9 %, whirl wipe 99.6 %, ripple 99.3 %,
the Tips episode list 99.1 %, Characters 98.3 %, Tips 98.0 %. What is left is the edges of text and fine artwork,
filtered slightly differently by two different GPUs. The formula, each comparison and the screens and effects compared without
a per-pixel figure (glass, mosaic, dissolve, raster, transparent movies, every menu) are in the
[accuracy notes](research/docs/alignment-plan.md#accuracy).

Separately, **every command, layer type, transition and layer effect the game's script uses is implemented**. The inventory comes from statically analysing the whole script
([`engine-internals.md`](research/docs/engine-internals.md) §2 and §10).

## 🍰 Preparing the tea party

### With the launcher

The **Open Catbox launcher** (`launcher/`) does the whole setup for you:

1. Point it at your own dump of the Switch release (the base NSP, title id `01006A300BA2C000`, and optionally the
   1.0.3 update NSP for Japanese) and your console's `prod.keys`. It unpacks the game data on your computer and
   checks it; nothing is uploaded anywhere.
2. Pick a language: **English** (Catbox English by &Olga), **English, Catbox X** (Foxdear's revision, currently
   up to Episode 7) or **Japanese**. The translations are downloaded from their authors when you choose them and
   stay installed, so you can switch at any time.
3. Press **Play**.

How it works, step by step, is in [`research/docs/launcher.md`](research/docs/launcher.md).

> [!IMPORTANT]
> **Wanted: a designer.** The launcher's look (the night sea, the moon, the butterflies, the layout) was made
> entirely with AI, as a working placeholder. If you can design something worthy of the golden land, please do:
> all of it lives in `launcher/ui/` (HTML, one CSS file with the colours and fonts as variables at the top, and a
> canvas script for the sky). It must stay our own art: no images or logos from the game or the translations.

### By hand

You need:

1. **The game**: the romfs file `data.rom` of your own dump of the Switch release (title id `01006A300BA2C000`), made
   for example with [nxdumptool](https://github.com/DarkMatterCore/nxdumptool). Extract it from your base NSP into an
   `assets` folder with a Switch extraction tool of your choice (the launcher does this natively, with no extra tools).
   `assets/data.rom` should have sha256 `6d90eb0bacacf769a7e4634407622b047acd711c47debb28136d7bab3fd0e591`.
2. **An English translation**: copy the `patch.rom` of either into the same `assets` folder; the port layers it
   over `data.rom`, just like the console does. If the translation comes with an `exefs` folder (an `.ips` patch),
   keep it next to the assets too, or pass `--exefs`: the culprit game's testimonies are read from it.
   - [umineko-catbox-english](https://github.com/andOlga/umineko-catbox-english) by andOlga and contributors (its
     authors allow mirroring its downloads with a link back to that page, its canonical home).
   - [Catbox X](https://github.com/Foxdear/umineko-catbox-X-english), Foxdear's revision of it, from its author's
     releases.
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
| Left click (on release), Space, Enter | A | Advance, choose |
| Backspace, Escape, right click | B | Back; hide the text box; skip an event |
| Hold the left button still in an event | Hold a touch | Skip the event |
| Y | Y | System menu |
| X | X | Backlog |
| `-` / `=` | − / + | Previous / next chapter |
| A / S | L / R | Auto mode / skip mode |
| Hold left Ctrl | | Fast-forward |
| Up arrow | Up | Roll back one line; move in menus |
| Drag and flick | Touch | Scroll lists, turn pages, pan pictures |
| Page Up / Page Down | Right stick | Scroll long texts (Tips, Characters, the culprit game) |
| F5 | | Save to the last used bookmark |
| F11 | | Fullscreen |

A tap leaves a ripple and a drag a trail of stars, as on the Switch's touch screen (setting "visual touch
feedback"). For working on the port, `--dev` adds debug overlays (F3: FPS, script state, every layer and its
animations) and time control (F6 pause, F7 one frame, F8/F9 slower/faster, F10 normal speed).

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
- **[umineko-catbox-english](https://github.com/andOlga/umineko-catbox-english)** by andOlga and contributors, and
  **[Catbox X](https://github.com/Foxdear/umineko-catbox-X-english)**, Foxdear's revision of it: the English
  translations this port is made to play. Neither is part of this project: andOlga's may be mirrored with a link
  back to its canonical page, as its README allows; Catbox X comes from its author's releases.
- **07th Expansion** and **Entergram**, for the game itself.

## ⚖️ License

[Mozilla Public License 2.0](LICENSE), the same license as shin.

Not affiliated with Entergram, 07th Expansion, the shin project or the translation teams.
