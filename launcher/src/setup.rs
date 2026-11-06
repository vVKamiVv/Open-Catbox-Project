//! First-run setup: turns the player's own Switch dump into the files the port reads, with the
//! native reader in [`crate::nx`] (no external tools).
//!
//! 1. Base NSP: the program NCA's title ID identifies the game; its title key comes from the NSP's
//!    common ticket, or from `title.keys` for personalised tickets. `data.rom` is streamed out of its RomFS.
//! 2. 1.0.3 update NSP (optional): `main` from the program NCA's ExeFS, and `patch.rom` (the 1.0.3
//!    Japanese script fixes) from its RomFS, a BKTR patch read on top of the base RomFS.
//! 3. Every file is hashed while it is written and checked against the known dumps (a mismatch is a
//!    warning), then moved into the data folder. Outputs go to `<data>/.setup` first, deleted on failure.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::nx::{self, FileRegion, Keys, Nca, Storage, FS_PFS0, FS_ROMFS};

pub const BASE_TITLE: &str = "01006a300ba2c000";
pub const UPDATE_TITLE: &str = "01006a300ba2c800";
pub const DATA_ROM_SHA256: &str =
    "6d90eb0bacacf769a7e4634407622b047acd711c47debb28136d7bab3fd0e591";
/// `main` of the 1.0.3 update (the version the port's research is based on).
const UPDATE_MAIN_SHA256: &str = "4670d3404cca2ed30d8bd1dfac243f8d50082c7ebea1e0ef4c6fe7ae01a646f3";
/// `patch.rom` of the 1.0.3 update's RomFS.
const UPDATE_PATCH_SHA256: &str =
    "4c82e74e13fe13e974545211b332c93f73d10a7f63ddcd461db73f47e341fcd8";

pub struct Input {
    pub base: PathBuf,
    pub update: Option<PathBuf>,
    pub prod_keys: PathBuf,
    pub title_keys: Option<PathBuf>,
}

/// `(step label, fraction 0..1)`
pub type Progress<'a> = &'a dyn Fn(&str, f64);

/// Runs the whole setup. Returns warnings (non-fatal problems) on success.
pub fn run(data: &Path, input: &Input, progress: Progress) -> Result<Vec<String>, String> {
    let tmp = data.join(".setup");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).map_err(|e| format!("Cannot create {}: {e}", tmp.display()))?;
    let result = run_in(data, &tmp, input, progress);
    let _ = fs::remove_dir_all(&tmp);
    // nothing should carry key material into a message, but keep long hex runs out of the UI regardless
    result.map_err(|e| redact_hex(&e))
}

fn run_in(
    data: &Path,
    tmp: &Path,
    input: &Input,
    progress: Progress,
) -> Result<Vec<String>, String> {
    let mut warnings = Vec::new();
    for (what, path) in [
        ("base game NSP", Some(&input.base)),
        ("prod.keys", Some(&input.prod_keys)),
        ("update NSP", input.update.as_ref()),
        ("title.keys", input.title_keys.as_ref()),
    ] {
        if let Some(path) = path {
            if !path.is_file() {
                return Err(format!("The {what} was not found: {}", path.display()));
            }
        }
    }

    // data.rom is nearly the whole base NSP
    let base_size = input.base.metadata().map(|m| m.len()).unwrap_or(0);
    let needed = base_size + (1 << 30);
    if let Ok(free) = fs4::available_space(data) {
        if free < needed {
            return Err(format!(
                "Not enough free space in {}: setup needs about {} GB, {} GB are free.",
                data.display(),
                gb(needed),
                gb(free)
            ));
        }
    }

    let keys = Keys::parse(
        &fs::read_to_string(&input.prod_keys).map_err(|e| format!("Cannot read prod.keys: {e}"))?,
    );
    if keys.get::<32>("header_key").is_err() {
        return Err("This prod.keys has no header_key. Please choose the prod.keys dumped from your Switch (e.g. with Lockpick_RCM).".into());
    }
    let title_keys = match &input.title_keys {
        Some(p) => nx::parse_title_keys(
            &fs::read_to_string(p).map_err(|e| format!("Cannot read title.keys: {e}"))?,
        ),
        None => HashMap::new(),
    };

    // --- base game ---
    progress("Reading the base game", 0.0);
    let (base, base_key) = open_program(&input.base, BASE_TITLE, &keys, &title_keys)?;
    let romfs = base
        .find_section(FS_ROMFS)
        .ok_or("The base game's program has no RomFS.")?;
    let base_romfs = || base.section(romfs, base_key).map_err(|e| e.to_string());
    let mut section = base_romfs()?;
    let (offset, size) = nx::find_root_file(&mut section, base.fs_header(romfs), "data.rom")
        .map_err(|e| e.to_string())?
        .ok_or(
            "The base game's RomFS has no data.rom. Is this really Umineko no Naku Koro ni Saku?",
        )?;
    let data_rom = tmp.join("data.rom");
    let hash = extract(
        &mut section,
        offset,
        size,
        &data_rom,
        "Extracting data.rom",
        progress,
    )?;
    if hash != DATA_ROM_SHA256 {
        warnings.push(format!("data.rom does not match the known 1.0.0 dump (SHA-256 {hash}). The game may still run, but this dump is untested."));
    }

    // --- 1.0.3 update (Japanese) ---
    let mut ja = None;
    if let Some(update) = &input.update {
        progress("Reading the update", 0.0);
        let (nca, key) = open_program(update, UPDATE_TITLE, &keys, &title_keys)?;

        let exefs = nca
            .find_section(FS_PFS0)
            .ok_or("The update has no ExeFS.")?;
        let mut section = nca.section(exefs, key).map_err(|e| e.to_string())?;
        let pfs0 = nx::le_u64(nca.fs_header(exefs), 0x40);
        let files = nx::pfs0_files(&mut section, pfs0).map_err(|e| e.to_string())?;
        let main_file = files
            .iter()
            .find(|f| f.name == "main")
            .ok_or("The update's ExeFS has no main.")?;
        let main = tmp.join("main");
        let hash = extract(
            &mut section,
            main_file.offset,
            main_file.size,
            &main,
            "Extracting the update's main",
            progress,
        )?;
        if hash != UPDATE_MAIN_SHA256 {
            warnings.push(
                "The update's main is not the known 1.0.3 build. Japanese may not work.".into(),
            );
        }

        let romfs = nca
            .find_section(FS_ROMFS)
            .ok_or("The update has no RomFS.")?;
        let mut patched =
            nx::open_patch(&nca, romfs, key, Box::new(base_romfs()?)).map_err(|e| e.to_string())?;
        let (offset, size) = nx::find_root_file(&mut patched, nca.fs_header(romfs), "patch.rom")
            .map_err(|e| e.to_string())?
            .ok_or("The update's RomFS has no patch.rom.")?;
        let patch = tmp.join("patch.rom");
        let hash = extract(
            &mut patched,
            offset,
            size,
            &patch,
            "Extracting the 1.0.3 script",
            progress,
        )?;
        if hash != UPDATE_PATCH_SHA256 {
            warnings.push(
                "The update's patch.rom is not the known 1.0.3 one. Japanese may not work.".into(),
            );
        }
        ja = Some((main, patch));
    }

    progress("Installing", 1.0);
    move_file(&data_rom, &data.join("data.rom"))?;
    if let Some((main, patch)) = ja {
        let ja_dir = data.join("languages").join("ja");
        let _ = fs::remove_dir_all(&ja_dir);
        fs::create_dir_all(ja_dir.join("exefs")).map_err(|e| e.to_string())?;
        move_file(&main, &ja_dir.join("exefs").join("main"))?;
        move_file(&patch, &ja_dir.join("patch.rom"))?;
        let _ = fs::write(ja_dir.join("version.txt"), "1.0.3");
    }
    Ok(warnings)
}

/// Finds the program NCA of an NSP, checks it is `title`, and returns it with its section key.
fn open_program(
    path: &Path,
    title: &str,
    keys: &Keys,
    title_keys: &HashMap<[u8; 16], [u8; 16]>,
) -> Result<(Nca, [u8; 16]), String> {
    let not_nsp = "This file does not look like a Switch NSP.";
    let mut nsp =
        FileRegion::whole(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
    let files = nx::pfs0_files(&mut nsp, 0).map_err(|_| not_nsp)?;

    // an update's program NCA carries the base program ID; its meta NCA has the update's title ID
    let (mut program, mut meta_title) = (None, None);
    for f in files.iter().filter(|f| f.name.ends_with(".nca")) {
        let nca = Nca::open(nsp.sub(f.offset, f.size).map_err(|_| not_nsp)?, keys)
            .map_err(|e| e.to_string())?;
        match nca.content_type() {
            0 => program = Some(nca),
            1 => meta_title = Some(nca.title_id()),
            _ => {}
        }
    }
    let nca = program.ok_or("This NSP contains no game program.")?;
    let found = format!("{:016x}", meta_title.unwrap_or(nca.title_id()));
    if found != title {
        return Err(match (title, found.as_str()) {
            (BASE_TITLE, UPDATE_TITLE) => "This is the 1.0.3 update NSP. Please choose the base game NSP here.".into(),
            (UPDATE_TITLE, BASE_TITLE) => "This is the base game NSP. Please choose the 1.0.3 update NSP here.".into(),
            _ => format!("This NSP is a different title ({found}), not Umineko no Naku Koro ni Saku ({title})."),
        });
    }

    let title_key = match nca.rights_id() {
        None => None,
        Some(rights) => {
            let ticket = match files.iter().find(|f| f.name.ends_with(".tik")) {
                Some(f) => {
                    let mut b = vec![0u8; f.size.min(0x1000) as usize];
                    nsp.read_at(f.offset, &mut b).map_err(|e| e.to_string())?;
                    Some(nx::parse_ticket(&b).ok_or("The NSP's ticket could not be read.")?)
                }
                None => None,
            };
            Some(match ticket {
                Some(t) if !t.personalised && t.rights_id == rights => t.title_key,
                // personalised: the title key is encrypted for one console, so title.keys must provide it
                _ => *title_keys.get(&rights).ok_or(match ticket {
                    Some(t) if t.personalised => "This NSP has a personalised ticket. Please also choose the title.keys dumped from your console.",
                    _ => "This NSP has no usable ticket. Please also choose the title.keys dumped from your console.",
                })?,
            })
        }
    };
    let key = nca
        .content_key(keys, title_key)
        .map_err(|e| e.to_string())?;
    Ok((nca, key))
}

/// Streams a file out of a dump, reporting bytes done / total; returns its SHA-256.
fn extract(
    s: &mut dyn Storage,
    offset: u64,
    size: u64,
    dest: &Path,
    label: &str,
    progress: Progress,
) -> Result<String, String> {
    progress(label, 0.0);
    nx::copy_out(s, offset, size, dest, &mut |done| {
        progress(
            &format!("{label} ({} of {})", human(done), human(size)),
            done as f64 / size.max(1) as f64,
        )
    })
    .map_err(|e| format!("{label} failed: {e}"))
}

fn human(bytes: u64) -> String {
    if bytes >= 1 << 30 {
        format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64)
    } else {
        format!("{:.0} MB", bytes as f64 / (1u64 << 20) as f64)
    }
}

/// Rename, or copy + delete across volumes.
fn move_file(from: &Path, to: &Path) -> Result<(), String> {
    let _ = fs::remove_file(to);
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fs::copy(from, to).map_err(|e| format!("Cannot write {}: {e}", to.display()))?;
    let _ = fs::remove_file(from);
    Ok(())
}

/// Replaces runs of 32+ hex digits (the length of a 16-byte key) with `<redacted>`.
fn redact_hex(text: &str) -> String {
    let mut out = String::new();
    let mut run = String::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_hexdigit() {
            run.push(c);
            continue;
        }
        out.push_str(if run.len() >= 32 { "<redacted>" } else { &run });
        run.clear();
        out.push(c);
    }
    out.pop();
    out
}

pub fn no_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

fn gb(bytes: u64) -> u64 {
    bytes.div_ceil(1 << 30)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_key_length_hex_only() {
        assert_eq!(
            redact_hex("key 00112233445566778899aabbccddeeff ok 01006a300ba2c000"),
            "key <redacted> ok 01006a300ba2c000"
        );
    }

    /// End-to-end run on a real dump (never committed): set NX_BASE, NX_UPDATE, NX_KEYS, NX_TITLEKEYS
    /// and NX_OUT, then `cargo test --release -p open-catbox-launcher -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_dump() {
        let var = |k| std::env::var_os(k).map(PathBuf::from);
        let input = Input {
            base: var("NX_BASE").unwrap(),
            update: var("NX_UPDATE"),
            prod_keys: var("NX_KEYS").unwrap(),
            title_keys: var("NX_TITLEKEYS"),
        };
        let out = var("NX_OUT").unwrap();
        let warnings = run(&out, &input, &|label, f| {
            eprintln!("{label} {:.0}%", f * 100.0)
        })
        .unwrap();
        eprintln!("warnings: {warnings:?}");
    }
}
