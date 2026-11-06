//! First-run setup: turns the player's own Switch dump into the files the port reads, using hactool.
//!
//! Steps (all intermediates live in `<data>/.setup` and are deleted at the end, also on failure):
//! 1. `hactool -t pfs0 --outdir` unpacks the base NSP (NCAs + the ticket).
//! 2. The title key comes from the ticket (`.tik`), or from `title.keys` for personalised tickets.
//! 3. `hactool --titlekey --romfsdir <program nca>` extracts the base RomFS files, of which we keep `data.rom`.
//! 4. For the 1.0.3 update: unpack it the same way, then `--exefsdir` on its program NCA gives `main`.
//!    Its RomFS (a BKTR patch adding `patch.rom`) is skipped: hactool 1.4 decrypts it wrongly (the file
//!    is garbage after 0x210 bytes), so Japanese runs the 1.0.0 script with the 1.0.3 executable.
//! 5. `data.rom` is checked against the known SHA-256, then everything is moved into the data folder.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use sha2::{Digest, Sha256};

pub const BASE_TITLE: &str = "01006a300ba2c000";
pub const UPDATE_TITLE: &str = "01006a300ba2c800";
pub const DATA_ROM_SHA256: &str =
    "6d90eb0bacacf769a7e4634407622b047acd711c47debb28136d7bab3fd0e591";
/// `main` of the 1.0.3 update (the version the port's research is based on).
const UPDATE_MAIN_SHA256: &str = "4670d3404cca2ed30d8bd1dfac243f8d50082c7ebea1e0ef4c6fe7ae01a646f3";

pub struct Input {
    pub base: PathBuf,
    pub update: Option<PathBuf>,
    pub prod_keys: PathBuf,
    pub title_keys: Option<PathBuf>,
}

/// `(step label, fraction 0..1)`
pub type Progress<'a> = &'a dyn Fn(&str, f64);

/// Runs the whole setup. Returns warnings (non-fatal problems) on success.
pub fn run(
    hactool: &Path,
    data: &Path,
    input: &Input,
    progress: Progress,
) -> Result<Vec<String>, String> {
    let tmp = data.join(".setup");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).map_err(|e| format!("Cannot create {}: {e}", tmp.display()))?;
    let result = run_in(hactool, data, &tmp, input, progress);
    // the temp folder holds the decrypted keys file and ~11 GB of intermediates
    let _ = fs::remove_dir_all(&tmp);
    result
}

fn run_in(
    hactool: &Path,
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

    // the unpacked base NCA and the extracted data.rom exist side by side for a moment
    let base_size = input.base.metadata().map(|m| m.len()).unwrap_or(0);
    let needed = base_size * 2 + (1 << 30);
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

    let keys = tmp.join("keys.txt");
    write_filtered_keys(&input.prod_keys, &keys)?;
    let title_keys = match &input.title_keys {
        Some(p) => parse_title_keys(
            &fs::read_to_string(p).map_err(|e| format!("Cannot read title.keys: {e}"))?,
        ),
        None => HashMap::new(),
    };
    let ctx = Hactool {
        exe: hactool,
        keys: &keys,
        log: tmp.join("hactool.log"),
        progress,
    };

    // --- base game ---
    let base_nsp = tmp.join("base_nsp");
    ctx.run(
        "Unpacking the base game NSP",
        &["-t", "pfs0", &arg("--outdir=", &base_nsp), &s(&input.base)],
        &base_nsp,
        base_size,
    )?;
    let (nca, key) = find_program(&base_nsp, BASE_TITLE, &title_keys)?;
    let base_files = tmp.join("base_files");
    let nca_size = nca.metadata().map(|m| m.len()).unwrap_or(base_size);
    ctx.run(
        "Extracting data.rom",
        &with_key(&key, vec![arg("--romfsdir=", &base_files), s(&nca)]),
        &base_files,
        nca_size,
    )?;
    let data_rom = base_files.join("data.rom");
    if !data_rom.is_file() {
        return Err(ctx.failure("Extracting data.rom") + "\n\nIf hactool reported no error: the base game's RomFS has no data.rom. Is this really Umineko no Naku Koro ni Saku?");
    }
    let _ = fs::remove_dir_all(&base_nsp);

    // --- 1.0.3 update (Japanese) ---
    let mut ja_main = None;
    if let Some(update) = &input.update {
        let upd_nsp = tmp.join("update_nsp");
        let upd_size = update.metadata().map(|m| m.len()).unwrap_or(0);
        ctx.run(
            "Unpacking the update NSP",
            &["-t", "pfs0", &arg("--outdir=", &upd_nsp), &s(update)],
            &upd_nsp,
            upd_size,
        )?;
        let (nca, key) = find_program(&upd_nsp, UPDATE_TITLE, &title_keys)?;
        let exefs = tmp.join("update_exefs");
        ctx.run(
            "Extracting the update",
            &with_key(&key, vec![arg("--exefsdir=", &exefs), s(&nca)]),
            &exefs,
            11_000_000,
        )?;
        expect_file(&exefs.join("main"), &ctx)?;
        ja_main = Some(exefs.join("main"));
    }

    let hash = sha256_file(&data_rom, "Verifying data.rom", progress)
        .map_err(|e| format!("Cannot read data.rom: {e}"))?;
    if hash != DATA_ROM_SHA256 {
        warnings.push(format!("data.rom does not match the known 1.0.0 dump (SHA-256 {hash}). The game may still run, but this dump is untested."));
    }

    progress("Installing", 1.0);
    fs::create_dir_all(data).map_err(|e| e.to_string())?;
    move_file(&data_rom, &data.join("data.rom"))?;
    if let Some(main) = ja_main {
        let ja_dir = data.join("languages").join("ja");
        let _ = fs::remove_dir_all(&ja_dir);
        fs::create_dir_all(ja_dir.join("exefs")).map_err(|e| e.to_string())?;
        if sha256_file(&main, "Verifying main", progress)
            .ok()
            .as_deref()
            != Some(UPDATE_MAIN_SHA256)
        {
            warnings.push(
                "The update's main is not the known 1.0.3 build. Japanese may not work.".into(),
            );
        }
        move_file(&main, &ja_dir.join("exefs").join("main"))?;
        let _ = fs::write(ja_dir.join("version.txt"), "1.0.3");
    }
    Ok(warnings)
}

fn with_key(key: &Option<String>, mut args: Vec<String>) -> Vec<String> {
    if let Some(key) = key {
        args.insert(0, format!("--titlekey={key}"));
    }
    args
}

struct Hactool<'a> {
    exe: &'a Path,
    keys: &'a Path,
    log: PathBuf,
    progress: Progress<'a>,
}

impl Hactool<'_> {
    /// Runs hactool, reporting progress as the size of `watch` against `expected` bytes.
    fn run(
        &self,
        label: &str,
        args: &[impl AsRef<std::ffi::OsStr>],
        watch: &Path,
        expected: u64,
    ) -> Result<(), String> {
        (self.progress)(label, 0.0);
        let log = File::create(&self.log).map_err(|e| e.to_string())?;
        let mut cmd = Command::new(self.exe);
        cmd.arg("-k")
            .arg(self.keys)
            .args(["--disablekeywarns", "--suppresskeys"])
            .args(args);
        cmd.stdin(Stdio::null())
            .stdout(log.try_clone().map_err(|e| e.to_string())?)
            .stderr(log);
        no_window(&mut cmd);
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Cannot start hactool ({}): {e}", self.exe.display()))?;
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                break status;
            }
            (self.progress)(
                label,
                (path_size(watch) as f64 / expected.max(1) as f64).min(0.99),
            );
            std::thread::sleep(Duration::from_millis(300));
        };
        if !status.success() {
            return Err(self.failure(label));
        }
        (self.progress)(label, 1.0);
        Ok(())
    }

    fn failure(&self, label: &str) -> String {
        let log = fs::read_to_string(&self.log).unwrap_or_default();
        let tail: Vec<&str> = log
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        let tail = redact_hex(&tail[tail.len().saturating_sub(4)..].join("\n"));
        let lower = log.to_ascii_lowercase();
        let hint = if lower.contains("key") || lower.contains("invalid nca header") {
            "\n\nThis usually means prod.keys is missing keys for this game. Dump prod.keys again from a console whose firmware is at least as new as the game."
        } else {
            ""
        };
        format!("{label} failed.\nhactool said:\n{tail}{hint}")
    }
}

fn expect_file(path: &Path, ctx: &Hactool) -> Result<(), String> {
    if path.is_file() && path_size(path) > 0 {
        Ok(())
    } else {
        Err(ctx.failure(&format!(
            "Creating {}",
            path.file_name().unwrap_or_default().to_string_lossy()
        )))
    }
}

/// Picks the program NCA (the largest one) and its title key.
fn find_program(
    dir: &Path,
    title: &str,
    title_keys: &HashMap<String, String>,
) -> Result<(PathBuf, Option<String>), String> {
    let files: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    let name = |p: &PathBuf| {
        p.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase()
    };
    let nca = files
        .iter()
        .filter(|p| name(p).ends_with(".nca") && !name(p).ends_with(".cnmt.nca"))
        .max_by_key(|p| path_size(p))
        .ok_or("This file does not look like a Switch NSP (it contains no NCA).")?;

    let Some(tik) = files.iter().find(|p| name(p).ends_with(".tik")) else {
        // no ticket: standard crypto, or the key has to come from title.keys
        let key = title_keys
            .iter()
            .find(|(r, _)| r.starts_with(title))
            .map(|(_, k)| k.clone());
        return Ok((nca.clone(), key));
    };
    let ticket = fs::read(tik).map_err(|e| e.to_string())?;
    let (rights_id, key_type, key) =
        parse_ticket(&ticket).ok_or("The NSP's ticket could not be read.")?;
    if !rights_id.starts_with(title) {
        let found = &rights_id[..16];
        return Err(match (title, found) {
            (BASE_TITLE, UPDATE_TITLE) => "This is the 1.0.3 update NSP. Please choose the base game NSP here.".into(),
            (UPDATE_TITLE, BASE_TITLE) => "This is the base game NSP. Please choose the 1.0.3 update NSP here.".into(),
            _ => format!("This NSP is a different title ({found}), not Umineko no Naku Koro ni Saku ({title})."),
        });
    }
    if key_type == 0 {
        return Ok((nca.clone(), Some(key)));
    }
    // personalised ticket: the title key is encrypted for one console, so title.keys must provide it
    match title_keys.get(&rights_id) {
        Some(key) => Ok((nca.clone(), Some(key.clone()))),
        None => Err("This NSP has a personalised ticket. Please also choose the title.keys dumped from your console.".into()),
    }
}

/// Returns `(rights id, title key type, title key)` as lowercase hex.
fn parse_ticket(b: &[u8]) -> Option<(String, u8, String)> {
    let sig = u32::from_le_bytes(b.get(0..4)?.try_into().ok()?);
    // signature type -> start of the ticket body (signature + padding)
    let body = match sig {
        0x10000 | 0x10003 => 0x240,
        0x10001 | 0x10004 => 0x140,
        0x10002 | 0x10005 => 0x80,
        _ => return None,
    };
    let key = hex(b.get(body + 0x40..body + 0x50)?);
    let key_type = *b.get(body + 0x141)?;
    let rights = hex(b.get(body + 0x160..body + 0x170)?);
    Some((rights, key_type, key))
}

/// hactool 1.4 rejects keys whose hex length is not a whole number of 16-byte blocks
/// (e.g. the 17-byte `*master_kek_source_*` lines newer dumpers write).
fn write_filtered_keys(src: &Path, dst: &Path) -> Result<(), String> {
    let text = fs::read_to_string(src).map_err(|e| format!("Cannot read prod.keys: {e}"))?;
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| {
            l.split_once('=')
                .is_some_and(|(_, v)| v.trim().len() % 32 == 0 && !v.trim().is_empty())
        })
        .collect();
    if !lines
        .iter()
        .any(|l| l.trim_start().starts_with("header_key"))
    {
        return Err("This prod.keys has no header_key. Please choose the prod.keys dumped from your Switch (e.g. with Lockpick_RCM).".into());
    }
    fs::write(dst, lines.join("\n")).map_err(|e| e.to_string())
}

fn parse_title_keys(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|l| l.split_once('='))
        .map(|(r, k)| (r.trim().to_ascii_lowercase(), k.trim().to_ascii_lowercase()))
        .filter(|(r, k)| r.len() == 32 && k.len() == 32)
        .collect()
}

pub fn sha256_file(path: &Path, label: &str, progress: Progress) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len().max(1);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 4 << 20];
    let mut done = 0u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        done += n as u64;
        if done % (256 << 20) < n as u64 {
            progress(label, done as f64 / len as f64);
        }
    }
    Ok(hex(&hasher.finalize()))
}

/// Rename, or copy + delete across volumes.
pub fn move_file(from: &Path, to: &Path) -> Result<(), String> {
    let _ = fs::remove_file(to);
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fs::copy(from, to).map_err(|e| format!("Cannot write {}: {e}", to.display()))?;
    let _ = fs::remove_file(from);
    Ok(())
}

pub fn path_size(path: &Path) -> u64 {
    match fs::metadata(path) {
        Ok(m) if m.is_dir() => fs::read_dir(path)
            .map(|d| d.filter_map(|e| e.ok()).map(|e| path_size(&e.path())).sum())
            .unwrap_or(0),
        Ok(m) => m.len(),
        Err(_) => 0,
    }
}

/// Keeps hactool's console output from leaking keys into error messages.
fn redact_hex(text: &str) -> String {
    let mut out = String::new();
    let mut run = String::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_hexdigit() {
            run.push(c);
            continue;
        }
        out.push_str(if run.len() >= 16 { "<redacted>" } else { &run });
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

fn hex(b: &[u8]) -> String {
    b.iter().fold(String::new(), |mut s, x| {
        use std::fmt::Write;
        let _ = write!(s, "{x:02x}");
        s
    })
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn arg(flag: &str, p: &Path) -> String {
    format!("{flag}{}", p.display())
}

fn gb(bytes: u64) -> u64 {
    bytes.div_ceil(1 << 30)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticket_fields() {
        let mut t = vec![0u8; 0x2c0];
        t[0..4].copy_from_slice(&0x10004u32.to_le_bytes());
        t[0x180] = 0xab;
        t[0x281] = 1;
        t[0x2a0..0x2a8].copy_from_slice(&[0x01, 0x00, 0x6a, 0x30, 0x0b, 0xa2, 0xc0, 0x00]);
        let (rights, key_type, key) = parse_ticket(&t).unwrap();
        assert!(rights.starts_with(BASE_TITLE));
        assert_eq!(key_type, 1);
        assert!(key.starts_with("ab00"));
        assert!(parse_ticket(&[0; 8]).is_none());
    }

    #[test]
    fn redacts_long_hex_only() {
        assert_eq!(
            redact_hex("key 0123456789abcdef0123 ok 0x10"),
            "key <redacted> ok 0x10"
        );
    }
}
