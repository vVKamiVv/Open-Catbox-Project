# Open Catbox launcher

A small Windows app that sets the game up from the player's own Switch dump, downloads the translations,
and starts `shin.exe`. It installs, updates and uninstalls itself: a release is one `OpenCatbox-Setup.exe`. Built with [Tauri 2](https://tauri.app); the front end is plain HTML/CSS/JS in `ui/`
(no npm, no bundler). Setup is fully native Rust: no external tools are needed or started.

How it works, step by step (for players and contributors): [`research/docs/launcher.md`](../research/docs/launcher.md).

## Structure

```
launcher/
  src/main.rs      Tauri commands: state, settings, setup, downloads, launching the game, moving the data folder,
                   installing, uninstalling, updating, version history
  src/install.rs   installing the launcher itself: copy, shortcuts, Installed apps entry, update, uninstall
  src/home.rs      where things live (the portable home), the one-time move in from %APPDATA%, checked copy/move,
                   using an existing game folder, deleting the player's data on uninstall
  src/setup.rs     first-run setup: finds the program NCAs, extracts and hash-checks data.rom, main, patch.rom
  src/nx/          Switch dump reader
    mod.rs           Storage trait, file regions, PFS0 (NSP / ExeFS) listing, streaming copy + SHA-256
    keys.rs          prod.keys, title.keys, tickets
    nca.rs           NCA header (AES-XTS), section key derivation, AES-CTR sections
    romfs.rs         IVFC level lookup, RomFS file table
    bktr.rs          update RomFS as a patch over the base: IndirectStorage + per-range CTR (AesCtrEx)
  src/langs.rs     translation downloads from the translators' GitHub releases
  src/music.rs     the music player: the Music Box's tracks from the game data, as Ogg Opus for the page; its settings
                   (music.json)
  src/history.rs   the Version history tab: CHANGELOG.md embedded at build time, plus newer GitHub releases
  ui/              index.html, style.css (design tokens at the top), app.js, music.js (the music player), sky.js
                   (animated background)
  ui/fonts/        Cinzel and Cormorant Garamond (SIL OFL, licence files next to them)
  icon.svg         source of icons/ (`cargo tauri icon icon.svg`, then delete the mobile/Store sizes)
  capabilities/    what the window may call (dialogs, opening links, updater, exit)
  scripts/         release.ps1 (builds and signs a release), make-latest-json.ps1 (the updater's latest.json)
```

## Running from source

```
cargo build --release -p shin                 # the game, found at target/release/shin.exe
cargo run -p open-catbox-launcher             # the launcher (debug)
```

A debug launcher only uses `target\release\shin.exe` (a debug `shin.exe` overflows its stack at startup). The
front end is embedded at compile time, so rebuild after editing `ui/`.

With the Tauri CLI (`cargo install tauri-cli --version "^2" --locked`), from this folder:

```
cargo tauri dev      # build and run with the CLI's file watching
```

Builds from source never show the install screen: only a launcher built with the `installer` feature (by
`scripts/release.ps1`, below) has `shin.exe` inside and can install itself.

Debug the web view with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333`, then open
`http://127.0.0.1:9333` in Chrome/Edge.

## Tests

```
cargo test -p open-catbox-launcher
```

Unit tests cover the formats with synthetic data. The `real_dump` test in `setup.rs` runs the whole setup on a real
dump (never committed): set `NX_BASE`, `NX_UPDATE`, `NX_KEYS`, `NX_TITLEKEYS` and `NX_OUT`, then
`cargo test --release -p open-catbox-launcher -- --ignored --nocapture`. `OPEN_CATBOX_APPDATA` stands in for
`%APPDATA%` in tests.

## Installing, updating, uninstalling

How it works for players: [`research/docs/launcher.md`](../research/docs/launcher.md#installing-updating-uninstalling).
In short, `OpenCatbox-Setup.exe` is the launcher itself with `shin.exe` embedded (`include_bytes!` of the file named by
`OPEN_CATBOX_SHIN`, behind the `installer` feature). Not installed, it shows the Install screen and copies itself to
`%LOCALAPPDATA%\Programs\Open Catbox\Open Catbox.exe` (folder of the player's choice) with `shin.exe` beside it,
shortcuts and an HKCU Installed apps entry; no administrator rights. `--uninstall` shows the Uninstall screen,
`--update <folder>` replaces only `Open Catbox.exe` and `shin.exe` without a window and starts the launcher. The
window is created in `main.rs` (`"create": false` in `tauri.conf.json`) so its WebView2 data can live in the home.

Updates use `tauri-plugin-updater` only to check `latest.json` and download the new Setup, whose minisign signature is
checked against the public key in `tauri.conf.json`. The launcher then starts that Setup with `--update <its folder>`
and exits.

## Releases

To make a release `X.Y.Z`:

1. Set the version in `Cargo.toml` (the only place: `tauri.conf.json` has none, so Tauri uses Cargo's) and add a
   `## vX.Y.Z — <date>` section to the root `CHANGELOG.md`.
2. From the repository root:

   ```powershell
   .\launcher\scripts\release.ps1 -OutDir release
   ```

   It builds `shin.exe`, then the launcher with it inside (dist profile, static C runtime, build paths remapped, into
   `target\installer`), stops if either binary holds this PC's user name or checkout path, signs the Setup with
   `cargo tauri signer sign` and the key in `%USERPROFILE%\.open-catbox\updater.key` (+ `.password`), and writes
   `OpenCatbox-Setup.exe`, `OpenCatbox-Setup.exe.sig`, `latest.json` and `SHA256SUMS.txt`. Needs the Tauri CLI
   (`cargo install tauri-cli --version "^2" --locked`).
3. Create the GitHub release `vX.Y.Z` and upload those four files.

The private key never goes into the repository; without it (and its password) no update can be published.

## Where things are

| What | Where |
| --- | --- |
| home | the launcher's folder (`%LOCALAPPDATA%\Programs\Open Catbox` by default) if writable, else `%APPDATA%\OpenCatbox` (also for source builds) |
| `shin.exe` | next to the launcher (release builds), then `target\release\shin.exe` of the source checkout (other builds) |
| Installed apps entry | `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Open Catbox` |
| settings | `<home>\launcher.json` |
| game data | `<home>\data`, or `data_dir` in the settings (absolute, or relative to the home) |
| saves | `<home>\saves\SAVEDATA` and `settings.json` (home in `%APPDATA%`: `%APPDATA%\shin`) |
| web view data | `<home>\webview` |
| game log | `<data>\logs\shin.log` |

## Not done yet

- Linux and macOS: the Rust side is portable, but the `shin.exe` path, the no-console flag and packaging are
  Windows-only so far.
- Code signing (Authenticode) of the Setup, so SmartScreen stops warning about it.
