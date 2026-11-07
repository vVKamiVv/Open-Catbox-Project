# The launcher, step by step

The Open Catbox launcher (`launcher/`) turns your own Switch dump into the files the port reads, downloads the
English translations from their authors, and starts the game. This page explains what it does and why, for players
who want to know what happens to their files and for contributors who want to work on it. Build instructions are in
[`launcher/README.md`](../../launcher/README.md).

## What setup asks for

| File | Needed | Why |
| --- | --- | --- |
| Base game NSP (`01006A300BA2C000`) | yes | holds `data.rom`, all of the game's assets |
| 1.0.3 update NSP (`01006A300BA2C800`) | only for Japanese | holds the 1.0.3 executable (`main`) and the 1.0.3 Japanese script fixes (`patch.rom`) |
| `prod.keys` | yes | your console's keys (for example from Lockpick_RCM), to decrypt the NSPs |
| `title.keys` | only for personalised tickets | the title key, when the NSP's ticket is tied to one console |

The English translations bring their own `main` and `patch.rom`, so the update is only needed for Japanese. Setup
recognises a swapped file (the update chosen as the base game, or another title) and says so.

## What is extracted, and how it is checked

| File | From | Size | SHA-256 |
| --- | --- | --- | --- |
| `data.rom` | base NSP, RomFS | most of the NSP | `6d90eb0bacacf769a7e4634407622b047acd711c47debb28136d7bab3fd0e591` |
| `main` | update NSP, ExeFS | | `4670d3404cca2ed30d8bd1dfac243f8d50082c7ebea1e0ef4c6fe7ae01a646f3` |
| `patch.rom` | update NSP, RomFS (patched over the base) | 26,515,456 bytes | `4c82e74e13fe13e974545211b332c93f73d10a7f63ddcd461db73f47e341fcd8` |

Each file is hashed while it is written. A different hash is a warning, not an error: the game may still run, but
that dump is untested. Everything is written to `<data>\.setup` first and only moved into place when all of it
succeeded; the folder is deleted afterwards, on failure too. Setup checks first that the data folder has about the
size of the base NSP plus 1 GB free.

**Use an existing game folder** skips extraction: pick a folder with a `data.rom`, which is hash-checked with a
progress bar (an unknown hash only warns), then choose **Copy it here** (into the home's data folder) or **Use it where
it is** (saved as `data_dir`).

## How extraction works

Setup is native Rust (`launcher/src/setup.rs` and `launcher/src/nx/`); no external tool is downloaded or started.
Files are read on demand through one `Storage` trait, so the multi-gigabyte NSP is streamed, never loaded whole.
The formats follow switchbrew.org (NCA, PFS0, RomFS, Ticket).

1. **NSP (PFS0).** An NSP is a PFS0 archive: a file table followed by the files. Setup lists its NCAs and its
   ticket (`.tik`).
2. **NCA header.** Each NCA starts with a 0xC00-byte header encrypted with AES-128-XTS using `header_key`, with
   0x200-byte sectors whose tweak is the sector number as a big-endian 128-bit value (unlike standard XTS). The
   decrypted header gives the content type (the program NCA is the one setup wants), the title ID, the rights ID,
   the section table and the four section headers. An update's program NCA carries the base title ID, so the
   update is recognised by its meta NCA.
3. **Title key.** When the NCA has a rights ID, its key is the title key: from the NSP's common ticket, or from
   `title.keys` if the ticket is personalised. It is decrypted with `titlekek_XX`, where `XX` is the NCA's key
   generation. Without a rights ID, the key comes from the NCA's key area, decrypted with `key_area_key_*_XX`.
4. **Sections (AES-CTR).** Each section header is checked against its SHA-256 in the NCA header, then the section
   is decrypted with AES-128-CTR: the upper 8 bytes of the counter come from the section header, the lower 8 are the
   byte offset in the NCA divided by 16.
5. **ExeFS and RomFS.** The ExeFS is another PFS0, which holds `main`. The RomFS section starts with an IVFC hash
   tree whose last level is the RomFS itself; its file table gives the offset and size of `data.rom` or
   `patch.rom` in the root directory.
6. **The update's RomFS (BKTR).** The update does not contain a full RomFS: it is a patch over the base game's
   RomFS section, described by two bucket-tree tables in its section header.
   - *IndirectStorage* maps each range of the patched RomFS either to the base RomFS section or to the update's
     own data.
   - *AesCtrEx* splits the update's data into ranges, each encrypted with its own counter generation (it replaces
     the lower half of the counter's upper 8 bytes) or not encrypted at all.

   Reading through both gives the 1.0.3 RomFS, from which `patch.rom` is copied like any other file.

## Privacy

- Your dump and keys never leave your computer. Setup reads them in place and only writes the extracted game
  files into the data folder.
- Keys are never written to disk, logged or shown: error messages name a missing key, never its value, and any
  run of 32 or more hex digits is replaced with `<redacted>` before an error reaches the window.
- Nothing is uploaded. The launcher's only network traffic is reading the translations' GitHub releases and
  downloading them, when you ask, checking this project's latest release for a launcher update (on start and
  from Settings), and listing this project's releases when the Version history tab is first opened.

## Data layout

Everything lives in the launcher's own folder, the **home**: the install folder when it is writable, otherwise (and
for builds from source) `%APPDATA%\OpenCatbox`.

```
<home>/
  Open Catbox.exe, shin.exe
  launcher.json              the launcher's settings
  data/                      the game data (below); data_dir in launcher.json can point elsewhere
  saves/SAVEDATA             the game's saves, and its settings.json
  webview/                   the window's WebView2 data
```

With the home in `%APPDATA%`, the saves stay in `%APPDATA%\shin`, as before. The data folder can be moved in
Settings (to an empty folder).

```
data/
  data.rom                   the base game
  languages/
    en/                      Catbox English
      patch.rom
      exefs/main, <build id>.ips
      version.txt            installed release tag
    enx/                     Catbox X, same layout
    ja/                      from the 1.0.3 update
      patch.rom              the 1.0.3 Japanese script fixes
      exefs/main             (no .ips)
      version.txt            "1.0.3"
  logs/shin.log              output of the last game run
```

## Languages

The English translations are never bundled; they are downloaded from their authors when you choose them.

- The launcher asks the GitHub releases API for the latest release of
  [andOlga/umineko-catbox-english](https://github.com/andOlga/umineko-catbox-english) or
  [Foxdear/umineko-catbox-X-english](https://github.com/Foxdear/umineko-catbox-X-english) and downloads its
  `patch_atmos.zip` (the Atmosphère layout). The download size is checked against the release.
- Only three things are kept: `romfs/patch.rom`, `exefs/main` and the `exefs_patches/*/<build id>.ips` files.
  Archive entry names are not trusted: files are only ever written under these fixed names. The language is
  unpacked into a staging folder and swapped in when complete, with the release tag in `version.txt`.
- **Check for updates** compares each installed tag with the latest release and offers **Update**.
- andOlga's README allows mirroring its downloads as long as they link back to the canonical page,
  <https://github.com/andOlga/umineko-catbox-english>; the launcher downloads straight from there, and anything
  this project offers of it links back to that page.
- **Catbox X** is Foxdear's revision of Catbox English, currently up to Episode 7, and comes only from its author's
  releases.
- **Japanese** is not downloaded: it comes from the 1.0.3 update in your own dump (see setup above).

## Starting the game

The launcher runs, from `shin.exe`'s own folder:

```
shin.exe --assets-dir <data> --patch-dir <data>\languages\<lang> --exefs <data>\languages\<lang>\exefs --savedata <home>\saves\SAVEDATA [--fullscreen]
```

`--fullscreen` is added when the launcher's Fullscreen setting is on: the game then starts in borderless fullscreen
for that run (F11 still toggles it, and the game's own PC tab is left as it was). How the game uses these folders is
in [engine-internals.md](engine-internals.md) §20. `shin.exe` is looked for next to the launcher (where installing
puts it), then, in builds from source, in `target\release\` of the checkout.

## Settings and logs

- Settings are stored in `<home>\launcher.json`: the last language, fullscreen, debug logging, an optional `data_dir`
  (absolute, or relative to the home) and `migrated_from` after the one-time move below.
- **Settings → Import save…** copies a `SAVEDATA` you pick (for example one backed up from the Switch) over the
  game's save, keeping the current one as `SAVEDATA.bak` next to it. Only its size is checked (empty or over 1 MB is
  refused); the game itself rejects a file that does not decode.
- The game's output goes to `<data>\logs\shin.log`, overwritten on each start. By default it is run with
  `RUST_LOG=warn`; **Settings → Debug logging** raises shin's own crates to `debug`, which is what a bug report
  needs.

## Version history

The **Version history** tab lists every release with its notes, the installed one marked. They come from this
repository's `CHANGELOG.md`, built into the launcher (its `## vX.Y.Z — date` sections), so the list shows offline.
The first time the tab opens, the launcher also reads this project's GitHub releases (published ones only) and adds the
versions the built-in changelog does not have yet, newest first; if GitHub cannot be reached, the tab says so and
keeps the built-in list (`launcher/src/history.rs`).

## No console windows

The launcher is built for the Windows GUI subsystem, so it opens no console. `shin.exe` stays a console program for
developers, but the launcher starts it with `CREATE_NO_WINDOW` and sends its output to the log instead.

## Moving in from %APPDATA%

Before v0.2.1 everything lived in `%APPDATA%`. The first time an installed launcher starts with a home that has no
`launcher.json`, it brings things in once:

- the old `%APPDATA%\OpenCatbox\launcher.json` is adopted;
- game data in the old default folder is moved into `<home>\data` (a rename on the same drive; otherwise copied,
  read back and checked with SHA-256, then deleted), with a progress screen. If space is short or the copy fails,
  the data stays where it is and `data_dir` points at it. A custom data folder is kept as it is;
- `SAVEDATA` and `settings.json` from `%APPDATA%\shin` are copied to `<home>\saves` if it has no saves yet; the
  originals are kept;
- a short notice says what happened, once.

## Installing, updating, uninstalling

A release is one file, `OpenCatbox-Setup.exe`: the launcher itself, with `shin.exe` inside. It installs for the
current Windows user only, so it never asks for administrator rights, and it never opens a console window
(the launcher is a GUI program; every helper it starts runs with `CREATE_NO_WINDOW`).

**Install.** Run the Setup and the Install screen asks for a folder (default
`%LOCALAPPDATA%\Programs\Open Catbox`; a folder you browse to gets an `Open Catbox` folder of its own inside) and for a
Start menu and a desktop shortcut. **Install** then:

1. writes `shin.exe` and a copy of the Setup named `Open Catbox.exe` into the folder, each to a `.part` file first,
   swapped in whole;
2. creates the chosen shortcuts (`Open Catbox.lnk`, through Windows' own shortcut object, so a desktop in OneDrive
   works);
3. adds Open Catbox to Installed apps: `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Open Catbox` with
   `DisplayName`, `DisplayVersion`, `Publisher` (Open Catbox Project), `DisplayIcon`, `InstallLocation`,
   `UninstallString` (`"<folder>\Open Catbox.exe" --uninstall`), `EstimatedSize`, `NoModify` and `NoRepair`;
4. starts the installed copy and closes. On its first start the installed copy brings in settings, data and saves from
   `%APPDATA%` (above), and never shows the Install screen. Running the Setup again offers the registered folder,
   which reinstalls over it.

**Update.** On start, and from **Settings → Check for updates**, the launcher reads
`https://github.com/vVKamiVv/Open-Catbox-Project/releases/latest/download/latest.json`. If it names a newer version, a
banner offers it. **Update now** downloads that version's `OpenCatbox-Setup.exe` and checks its signature (minisign,
against the public key built into the launcher; a file that does not match is refused), saves it to a temporary folder,
starts it as `OpenCatbox-Setup.exe --update <install folder>` and closes. The new Setup opens no window: it waits for the
old launcher to exit (up to a minute), replaces only `shin.exe` and `Open Catbox.exe` (data, saves and settings are
left alone), points the shortcuts that exist at it,
updates the version in Installed apps, starts the new launcher and has its temporary copy deleted. If it cannot (for
example while the game is running), nothing is half replaced: the launcher starts again and says why in the banner.

**Uninstall.** From Installed apps, or **Settings → Uninstall…**. The Uninstall screen asks whether to **Also delete the
game data, languages and saves** (off by default). If ticked, only known files are deleted (`data.rom`, the languages,
the logs, `SAVEDATA*`, `settings.json`, `launcher.json`) and folders only when left empty, so other files in a data
folder of your own survive. Uninstalling then removes the shortcuts and the Installed apps entry, closes, and a hidden PowerShell then deletes
`Open Catbox.exe` and `shin.exe` (a running program cannot delete itself), the web view's data
(`<home>\webview`, and the older `%LOCALAPPDATA%\io.github.vvkamivv.opencatbox`) and the program folder if nothing else
is in it. Saves are deleted only when the box is ticked.

## Design

The launcher's look (the night sea, the moon, the butterflies, the layout) was made with AI as a working
placeholder. Designers are very welcome to replace it: everything lives in `launcher/ui/` (`index.html`,
`style.css` with the colours and fonts as CSS variables at the top, `app.js`, and `sky.js`, the canvas script for
the sky). It must stay original art: no images or logos from the game or the translations.
