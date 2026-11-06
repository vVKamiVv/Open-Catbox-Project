# Open Catbox launcher

A small Windows app that sets the game up from the player's own Switch dump, downloads the translations,
and starts `shin.exe`. Built with [Tauri 2](https://tauri.app); the front end is plain HTML/CSS/JS in `ui/`
(no npm, no bundler). Setup is fully native Rust: no external tools are needed or started.

How it works, step by step (for players and contributors): [`research/docs/launcher.md`](../research/docs/launcher.md).

## Structure

```
launcher/
  src/main.rs      Tauri commands: state, settings, setup, downloads, launching the game, moving the data folder
  src/setup.rs     first-run setup: finds the program NCAs, extracts and hash-checks data.rom, main, patch.rom
  src/nx/          Switch dump reader
    mod.rs           Storage trait, file regions, PFS0 (NSP / ExeFS) listing, streaming copy + SHA-256
    keys.rs          prod.keys, title.keys, tickets
    nca.rs           NCA header (AES-XTS), section key derivation, AES-CTR sections
    romfs.rs         IVFC level lookup, RomFS file table
    bktr.rs          update RomFS as a patch over the base: IndirectStorage + per-range CTR (AesCtrEx)
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

A debug launcher only uses `target\release\shin.exe` (a debug `shin.exe` overflows its stack at startup). The
front end is embedded at compile time, so rebuild after editing `ui/`.

With the Tauri CLI (`cargo install tauri-cli --version "^2" --locked`), from this folder:

```
cargo tauri dev      # build and run with the CLI's file watching
cargo tauri build    # release build plus an NSIS installer in target/release/bundle/nsis/
```

Debug the web view with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333`, then open
`http://127.0.0.1:9333` in Chrome/Edge.

## Tests

```
cargo test -p open-catbox-launcher
```

Unit tests cover the formats with synthetic data. The `real_dump` test in `setup.rs` runs the whole setup on a real
dump (never committed): set `NX_BASE`, `NX_UPDATE`, `NX_KEYS`, `NX_TITLEKEYS` and `NX_OUT`, then
`cargo test --release -p open-catbox-launcher -- --ignored --nocapture`.

## Releases and updates

The launcher updates itself (`tauri-plugin-updater`): on start, and from **Settings → Check for launcher updates**,
it reads `https://github.com/vVKamiVv/Open-Catbox-Project/releases/latest/download/latest.json`, and if that
version is newer it offers to download the installer, check its signature against the public key in
`tauri.conf.json`, install it (NSIS, passive) and restart. **Version history** shows `CHANGELOG.md` (embedded at
build time) plus newer GitHub releases.

To make a release `X.Y.Z`:

1. Set the version in `Cargo.toml` and `tauri.conf.json`, and add a `## vX.Y.Z — <date>` section to the root
   `CHANGELOG.md`.
2. Build `shin.exe` (`cargo build --release -p shin`), then build the installer signed with the updater key, from
   this folder:

   ```powershell
   $env:TAURI_SIGNING_PRIVATE_KEY = Get-Content -Raw "$env:USERPROFILE\.open-catbox\updater.key"
   $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = Get-Content -Raw "$env:USERPROFILE\.open-catbox\updater.key.password"
   cargo tauri build
   ```

3. `.\scripts\make-latest-json.ps1 -Version X.Y.Z -NotesFile ..\CHANGELOG.md -OutDir <folder>` copies the
   installer and its `.sig` as `OpenCatbox_X.Y.Z_x64-setup.exe(.sig)` and writes `latest.json`.
4. Create the GitHub release `vX.Y.Z` and upload those three files.

The private key never goes into the repository; without it (and its password) no update can be published.

## Where things are

| What | Where |
| --- | --- |
| `shin.exe` | next to the launcher (release builds), then `target\release\shin.exe` of the source checkout |
| settings | `%APPDATA%\OpenCatbox\launcher.json` |
| game data | `%APPDATA%\OpenCatbox\data` (can be moved in Settings) |
| game log | `<data>\logs\shin.log` |

## Not done yet

- Linux and macOS: the Rust side is portable, but the `shin.exe` path, the no-console flag and packaging are
  Windows-only so far.
- Code signing of the launcher and installer.
- The fullscreen setting is saved but not passed to the game yet (it has no flag; F11 toggles in game).
