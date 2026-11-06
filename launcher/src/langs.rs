//! Translation patches, downloaded from their authors' GitHub releases (never bundled with the launcher).

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

use serde::Serialize;

/// `(id, GitHub repo)`; the release asset is the Atmosphère layout `patch_atmos.zip`.
pub const PATCHES: &[(&str, &str)] = &[
    ("en", "andOlga/umineko-catbox-english"),
    ("enx", "Foxdear/umineko-catbox-X-english"),
];
const ASSET: &str = "patch_atmos.zip";
const USER_AGENT: &str = "OpenCatbox-Launcher";

#[derive(Serialize, Clone)]
pub struct Release {
    pub id: String,
    pub tag: String,
    pub url: String,
    pub size: u64,
}

pub fn repo(id: &str) -> Result<&'static str, String> {
    PATCHES
        .iter()
        .find(|(i, _)| *i == id)
        .map(|(_, r)| *r)
        .ok_or_else(|| format!("Unknown translation '{id}'"))
}

pub fn latest(id: &str) -> Result<Release, String> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", repo(id)?);
    let json: serde_json::Value = ureq::get(&url)
        .set("User-Agent", USER_AGENT)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("Cannot reach GitHub: {e}"))?
        .into_json()
        .map_err(|e| e.to_string())?;
    let asset = json["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|a| a["name"] == ASSET))
        .ok_or_else(|| {
            format!(
                "The latest release of {} has no {ASSET}.",
                repo(id).unwrap_or(id)
            )
        })?;
    Ok(Release {
        id: id.into(),
        tag: json["tag_name"].as_str().unwrap_or("?").into(),
        url: asset["browser_download_url"]
            .as_str()
            .unwrap_or_default()
            .into(),
        size: asset["size"].as_u64().unwrap_or(0),
    })
}

/// Installed version of a language (its `version.txt`), if its files are complete.
pub fn installed(data: &Path, id: &str) -> Option<String> {
    let dir = data.join("languages").join(id);
    let complete =
        dir.join("exefs").join("main").is_file() && (id == "ja" || dir.join("patch.rom").is_file());
    complete.then(|| {
        fs::read_to_string(dir.join("version.txt"))
            .unwrap_or_else(|_| "installed".into())
            .trim()
            .to_string()
    })
}

/// Downloads the latest release and installs it as `<data>/languages/<id>`. Returns the installed tag.
pub fn install(data: &Path, id: &str, progress: &dyn Fn(f64)) -> Result<String, String> {
    let release = latest(id)?;
    let langs = data.join("languages");
    fs::create_dir_all(&langs).map_err(|e| e.to_string())?;
    let zip = langs.join(format!("{id}.zip.part"));
    let staging = langs.join(format!("{id}.new"));
    let result = download(&release, &zip, progress).and_then(|_| {
        let _ = fs::remove_dir_all(&staging);
        unpack(&zip, &staging)?;
        fs::write(staging.join("version.txt"), &release.tag).map_err(|e| e.to_string())?;
        let target = langs.join(id);
        let _ = fs::remove_dir_all(&target);
        fs::rename(&staging, &target).map_err(|e| e.to_string())
    });
    let _ = fs::remove_file(&zip);
    let _ = fs::remove_dir_all(&staging);
    result.map(|_| release.tag)
}

fn download(release: &Release, to: &Path, progress: &dyn Fn(f64)) -> Result<(), String> {
    let resp = ureq::get(&release.url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;
    let total = resp
        .header("Content-Length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(release.size)
        .max(1);
    let mut reader = resp.into_reader();
    let mut file = File::create(to).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0u64;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("Download interrupted: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        progress(done as f64 / total as f64);
    }
    if release.size != 0 && done != release.size {
        return Err(format!(
            "Download incomplete ({done} of {} bytes).",
            release.size
        ));
    }
    Ok(())
}

/// Copies just the three things the port needs out of the Atmosphère archive. Entry names are untrusted:
/// files are only ever written under fixed names.
fn unpack(zip: &Path, dir: &Path) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(File::open(zip).map_err(|e| e.to_string())?)
        .map_err(|e| format!("The download is not a valid zip: {e}"))?;
    fs::create_dir_all(dir.join("exefs")).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().replace('\\', "/");
        let lower = name.to_ascii_lowercase();
        let file_name = name.rsplit('/').next().unwrap_or_default();
        let stem = file_name
            .strip_suffix(".ips")
            .or_else(|| file_name.strip_suffix(".IPS"))
            .unwrap_or_default();
        let dest = if lower.ends_with("/romfs/patch.rom") {
            dir.join("patch.rom")
        } else if lower.ends_with("/exefs/main") {
            dir.join("exefs").join("main")
        } else if lower.contains("exefs_patches/")
            && !stem.is_empty()
            && stem.chars().all(|c| c.is_ascii_hexdigit())
        {
            dir.join("exefs").join(format!("{stem}.ips"))
        } else {
            continue;
        };
        let mut out = File::create(&dest).map_err(|e| e.to_string())?;
        io::copy(&mut entry, &mut out).map_err(|e| format!("Cannot unpack {name}: {e}"))?;
    }
    if !dir.join("patch.rom").is_file() || !dir.join("exefs").join("main").is_file() {
        return Err("The release archive does not have the expected layout (romfs/patch.rom and exefs/main).".into());
    }
    Ok(())
}
