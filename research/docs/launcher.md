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
  downloading them, when you ask.

## Data layout

The data folder is `%APPDATA%\OpenCatbox\data` by default and can be moved in Settings (to an empty folder).

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
shin.exe --assets-dir <data> --patch-dir <data>\languages\<lang> --exefs <data>\languages\<lang>\exefs
```

`shin.exe` is looked for next to the launcher, then in `target\release\` of a source checkout.

## Settings and logs

- Settings are stored in `%APPDATA%\OpenCatbox\launcher.json`: the data folder, the last language, fullscreen and
  debug logging. Fullscreen is only remembered for now; press F11 in the game.
- The game's output goes to `<data>\logs\shin.log`, overwritten on each start. By default it is run with
  `RUST_LOG=warn`; **Settings → Debug logging** raises shin's own crates to `debug`, which is what a bug report
  needs.

## No console windows

The launcher is built for the Windows GUI subsystem, so it opens no console. `shin.exe` stays a console program for
developers, but the launcher starts it with `CREATE_NO_WINDOW` and sends its output to the log instead.

## Design

The launcher's look (the night sea, the moon, the butterflies, the layout) was made with AI as a working
placeholder. Designers are very welcome to replace it: everything lives in `launcher/ui/` (`index.html`,
`style.css` with the colours and fonts as CSS variables at the top, `app.js`, and `sky.js`, the canvas script for
the sky). It must stay original art: no images or logos from the game or the translations.
