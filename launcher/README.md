# Open Catbox launcher

A small Windows app that sets the game up from the player's own Switch dump, downloads the translations,
and starts `shin.exe`. Built with [Tauri 2](https://tauri.app); the front end is plain HTML/CSS/JS in `ui/`
(no npm, no bundler).

```
launcher/
  src/main.rs      Tauri commands: settings, launch, moving the data folder
  src/setup.rs     first-run extraction with hactool
  src/langs.rs     translation downloads from the translators' GitHub releases
  ui/              index.html, style.css (design tokens at the top), app.js, sky.js (animated background)
  ui/fonts/        Cinzel and Cormorant Garamond (SIL OFL, licence files next to them)
  icon.svg         source of icons/ (`cargo tauri icon icon.svg`, then delete the mobile/Store sizes)
  capabilities/    what the window may call (dialogs, opening links)
```

## Running from source

```
cargo build --release -p shin                 # the game, found at target/release/shin.exe
cargo run -p open-catbox-launcher             # the launcher (debug)
```

The front end is embedded at compile time, so rebuild after editing `ui/`. With `cargo install tauri-cli
--version "^2" --locked` you can also use `cargo tauri dev` / `cargo tauri build` from this folder
(`build` makes an NSIS installer in `target/release/bundle/`).

Debug the web view with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333`, then open
`http://127.0.0.1:9333` in Chrome/Edge.

Players never see a console: release builds use the Windows GUI subsystem, and hactool and `shin.exe`
(a console app, kept that way for developers) are started with `CREATE_NO_WINDOW`, their output going to
log files instead.

## Where things are looked for

| What | Order |
| --- | --- |
| `hactool.exe` | Settings override, `<launcher dir>\tools\hactool.exe`, `..\tools\hactool\hactool.exe` of the source checkout |
| `shin.exe` | next to the launcher, `target\release\shin.exe` of the source checkout |
| settings | `%APPDATA%\OpenCatbox\launcher.json` |
| game data | `%APPDATA%\OpenCatbox\data` (can be moved in Settings) |

## Data layout

```
<data>/data.rom                                   base game RomFS file (SHA-256 checked)
<data>/languages/en/patch.rom, exefs/{main, <buildid>.ips}    Catbox English (&Olga)
<data>/languages/enx/patch.rom, exefs/{main, <buildid>.ips}   Catbox X (Foxdear)
<data>/languages/ja/exefs/main                       from the 1.0.3 update
<data>/logs/shin.log                              output of the last game run (RUST_LOG=warn, or debug
                                                  for shin's crates with Settings > Debug logging)
```

The game is started as
`shin.exe --assets-dir <data> --patch-dir <data>/languages/<lang> --exefs <data>/languages/<lang>/exefs`.

## How setup reads the dump

All intermediates go to `<data>/.setup` and are deleted afterwards (also on failure); peak disk use is about
twice the base NSP (~22 GB). `prod.keys` is copied there with the lines hactool 1.4 cannot parse removed.

1. `hactool -k keys -t pfs0 --outdir=base_nsp <base.nsp>` unpacks the NSP.
2. The title key is read from the `.tik` (common ticket: offset 0x180 for RSA-2048 signatures), or from
   `title.keys` for personalised tickets. The ticket's rights ID also tells a wrong file apart
   (e.g. the update chosen as the base game).
3. `hactool -k keys --titlekey=<key> --romfsdir=base_files <largest .nca>` gives `data.rom`.
4. With the update: unpack it like step 1, then
   `hactool -k keys --titlekey=<key> --exefsdir=update_exefs <largest .nca>` gives `main`.
   The update's RomFS (a BKTR patch whose only file is a new `patch.rom` with the 1.0.3 script) is skipped:
   hactool 1.4 decrypts it wrongly (`--baseromfs`/`--basefake --onlyupdated` give a `patch.rom` that is
   garbage after 0x210 bytes). So Japanese currently runs the 1.0.0 script from `data.rom` with the 1.0.3 `main`.
5. `data.rom` and `main` are checked against the known hashes (a mismatch is a warning, not an error).

Every hactool call also gets `--disablekeywarns --suppresskeys`, and error output shown to the user has long
hex strings redacted, so keys never end up on screen or in logs.

## Not done yet

- Linux and macOS: the Rust side is portable, but paths (`shin.exe`, `hactool.exe`), the no-console flag and
  packaging are Windows-only so far.
- Code signing of the launcher and installer.
- Packaging `shin.exe` and `tools\hactool.exe` next to the launcher in the installer.
- The 1.0.3 `patch.rom` for Japanese (needs a correct BKTR reader, e.g. our own in Rust or hactoolnet).
- The fullscreen setting is saved but not passed to the game yet (it has no flag; F11 toggles in game).
