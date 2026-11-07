<div align="center">

# 🦋 Open Catbox Project 🦋

**Play *Umineko: The Symphony of Catbox and Dreams* natively on PC, in English.**

*A gameboard rebuilt piece by piece, so the witch's tea party can go on outside the console.*

</div>

---

A native PC version of *Umineko no Naku Koro ni Saku ~Nekobako to Musou no Koukyoukyoku~*. Bring your own Switch
copy of the game; no emulator needed. Windows for now.

## 🟥 Spoken in red

- $\color{red}{\textsf{No game data, keys or decompiled code are in this repository.}}$
- $\color{red}{\textsf{You need to own the game to play it.}}$

## 📜 Rules of the gameboard

1. **Faithful.** Timings, effects and menus are taken from the original game's code and checked side by side with
   it.
2. **Open.** Everything we learn about the engine is written down in [`research/`](research/).
3. **Built on [shin](https://github.com/DCNick3/shin)** by DCNick3, developed further here as its own project.

## 🦋 The Golden Land so far

<div align="center">

### 99.7 % like the original

</div>

Measured against the original game, screen by screen ([how](research/docs/alignment-plan.md#accuracy)). Every
command, effect and transition the game uses is implemented.

### Extras over the Switch version

- **PC settings**: window mode and size, V-Sync, frame-rate cap, supersampling.
- **Rebindable controls** for keyboard, mouse and controller, with Xbox, PlayStation and Switch button prompts.
- **Save from the galleries**: pictures, music, movies and character sprites.
- **Any refresh rate**: 120, 144 and 240 Hz run at the right speed.
- **A launcher** that sets everything up, updates itself and plays the soundtrack from your game files.

Full list in the [changelog](CHANGELOG.md).

## 🍰 Preparing the tea party

### With the launcher

1. Download `OpenCatbox-Setup.exe` from the [releases](https://github.com/vVKamiVv/Open-Catbox-Project/releases)
   and run it.
2. Pick your game dump (the base NSP, plus the 1.0.3 update for Japanese) and your `prod.keys`.
3. Pick a language: **English**, **English (Catbox X)** (up to Episode 7) or **Japanese**.
4. Press **Play**.

How the launcher works: [`launcher.md`](research/docs/launcher.md).

> [!IMPORTANT]
> **Wanted: a designer.** The launcher's look was made with AI as a placeholder. If you'd like to design a better
> one, everything is in `launcher/ui/`. It must be original art, with nothing taken from the game or the
> translations.

### By hand

1. Extract `data.rom` from your base NSP (title id `01006A300BA2C000`) into an `assets` folder.
2. Add a translation's `patch.rom` to the same folder, and its `exefs` folder if it has one:
   [Catbox English](https://github.com/andOlga/umineko-catbox-english) or
   [Catbox X](https://github.com/Foxdear/umineko-catbox-X-english).
3. Build with [Rust](https://www.rust-lang.org/) and run:

```bash
cargo build --release -p shin
```

Put the `assets` folder next to `shin.exe`, or set `SHIN_ASSETS` to its path. On Linux and macOS, movies need
**ffmpeg**. More options: `shin --help`.

## 🎮 Moving the pieces

| Keyboard / mouse | Controller (Switch labels) | Does |
|---|---|---|
| Left click, Space, Enter | A | Advance, choose |
| Backspace, Escape, right click | B | Back, hide the text box |
| Hold the left button | Hold a touch | Skip an event |
| Y | Y | System menu |
| X | X | Backlog |
| `-` / `=` | − / + | Previous / next chapter |
| A / S | L / R | Auto / skip mode |
| Hold left Ctrl | | Fast-forward |
| Up arrow | Up | Roll back a line |
| Page Up / Page Down | Right stick | Scroll long texts |
| F5 or P | ZR | Quick save; in a gallery, save the item |
| F11 | | Fullscreen |

Rebind keys in **Config → PC → Controls**. To confirm with Xbox A / PlayStation ✕, set **Confirm button** to Bottom.

## 🗝️ Inside the mansion

| Path | Contents |
|---|---|
| `shin/` | The game engine |
| `shin-core/` | The game's data formats |
| `shin-render*`, `shin-audio`, `shin-video`, `shin-tasks`, `shin-window`, `shin-input`, `shin-primitives`, `shin-derive` | Rendering, audio, video, threads, window, input |
| `sdu/`, `shin-asm/` | Data extraction tool and script assembler |
| `launcher/` | The launcher |
| `research/` | Engine notes and reverse-engineering tools; start at [`architecture.md`](research/docs/architecture.md) |

## 🌹 Credits

Full list in [CREDITS.md](CREDITS.md).

- **[shin](https://github.com/DCNick3/shin)** by DCNick3 and contributors: the engine this grew from.
- **[umineko-catbox-english](https://github.com/andOlga/umineko-catbox-english)** by andOlga and contributors, and
  **[Catbox X](https://github.com/Foxdear/umineko-catbox-X-english)** by Foxdear: the English translations.
- **07th Expansion** and **Entergram**, for the game.

## ⚖️ License

[Mozilla Public License 2.0](LICENSE), like shin.

Not affiliated with Entergram, 07th Expansion, the shin project or the translation teams.
