//! Version history: the repository's CHANGELOG.md, embedded at build time, plus newer GitHub releases when online.

use std::time::Duration;

use serde::Serialize;

const CHANGELOG: &str = include_str!("../../CHANGELOG.md");
const RELEASES: &str = "https://api.github.com/repos/vVKamiVv/Open-Catbox-Project/releases";

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Entry {
    pub version: String,
    pub date: String,
    pub notes: Vec<String>,
}

#[derive(Serialize)]
pub struct History {
    pub installed: &'static str,
    pub entries: Vec<Entry>,
    /// Why the GitHub releases could not be read, when online was asked for.
    pub online_error: Option<String>,
}

/// Bullet items of a markdown body; indented lines continue the previous item, other text lines are items too.
pub fn notes(body: &str) -> Vec<String> {
    let mut items: Vec<String> = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            items.push(item.trim().into());
        } else if line.starts_with([' ', '\t']) && !items.is_empty() {
            let last = items.last_mut().unwrap();
            last.push(' ');
            last.push_str(trimmed);
        } else {
            items.push(trimmed.into());
        }
    }
    items
}

/// `## v<version> — <date>` sections, in file order; other `##` sections end a release.
pub fn parse_changelog(text: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut current: Option<(Entry, String)> = None;
    for line in text.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            if let Some((mut entry, body)) = current.take() {
                entry.notes = notes(&body);
                entries.push(entry);
            }
            if let Some(rest) = heading.trim().strip_prefix('v') {
                let (version, date) = rest.split_once('—').unwrap_or((rest, ""));
                let entry = Entry {
                    version: version.trim().into(),
                    date: date.trim().into(),
                    notes: Vec::new(),
                };
                current = Some((entry, String::new()));
            }
        } else if let Some((_, body)) = current.as_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    if let Some((mut entry, body)) = current {
        entry.notes = notes(&body);
        entries.push(entry);
    }
    entries
}

/// Published (not draft, not prerelease) releases from the GitHub API JSON.
fn parse_releases(json: &serde_json::Value) -> Vec<Entry> {
    json.as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["draft"] != true && r["prerelease"] != true)
        .filter_map(|r| {
            let tag = r["tag_name"].as_str()?;
            Some(Entry {
                version: tag.trim_start_matches('v').into(),
                date: r["published_at"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(10)
                    .collect(),
                notes: notes(r["body"].as_str().unwrap_or("")),
            })
        })
        .collect()
}

fn version_key(version: &str) -> Vec<u64> {
    version
        .split(['-', '+'])
        .next()
        .unwrap_or("")
        .split('.')
        .map(|n| n.parse().unwrap_or(0))
        .collect()
}

/// The changelog's entries win; releases only add versions it does not have. Newest first.
fn merge(mut entries: Vec<Entry>, releases: Vec<Entry>) -> Vec<Entry> {
    for release in releases {
        if !entries
            .iter()
            .any(|e| version_key(&e.version) == version_key(&release.version))
        {
            entries.push(release);
        }
    }
    entries.sort_by_key(|e| std::cmp::Reverse(version_key(&e.version)));
    entries
}

fn fetch_releases() -> Result<Vec<Entry>, String> {
    let json: serde_json::Value = ureq::get(RELEASES)
        .set("User-Agent", "OpenCatbox-Launcher")
        .set("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(10))
        .call()
        .map_err(|e| format!("Cannot reach GitHub: {e}"))?
        .into_json()
        .map_err(|e| e.to_string())?;
    Ok(parse_releases(&json))
}

pub fn history(online: bool) -> History {
    let (releases, online_error) = match online.then(fetch_releases) {
        Some(Ok(releases)) => (releases, None),
        Some(Err(e)) => (Vec::new(), Some(e)),
        None => (Vec::new(), None),
    };
    History {
        installed: env!("CARGO_PKG_VERSION"),
        entries: merge(parse_changelog(CHANGELOG), releases),
        online_error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Changelog\n\nIntro.\n\n## v0.2.0 — 2027-01-02\n\n- Two\n- Long\n  line\n\n\
        ## v0.1.0 — 2026-11-06\n* One <b>raw</b>\n\n## Before the fork\n\n# Version 0.8.1\n- upstream\n";

    #[test]
    fn parses_releases_and_stops_at_other_sections() {
        let entries = parse_changelog(SAMPLE);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].version, "0.2.0");
        assert_eq!(entries[0].date, "2027-01-02");
        assert_eq!(entries[0].notes, ["Two", "Long line"]);
        assert_eq!(entries[1].notes, ["One <b>raw</b>"]);
    }

    #[test]
    fn merges_newer_releases_newest_first() {
        let json = serde_json::json!([
            {"tag_name": "v0.10.0", "published_at": "2027-03-01T10:00:00Z", "body": "- Ten\r\n- Eleven"},
            {"tag_name": "v0.2.0", "published_at": "2027-01-02T10:00:00Z", "body": "- from GitHub"},
            {"tag_name": "v0.3.0-beta", "prerelease": true, "body": ""},
        ]);
        let merged = merge(parse_changelog(SAMPLE), parse_releases(&json));
        let versions: Vec<_> = merged.iter().map(|e| e.version.as_str()).collect();
        assert_eq!(versions, ["0.10.0", "0.2.0", "0.1.0"]);
        assert_eq!(merged[0].date, "2027-03-01");
        assert_eq!(merged[0].notes, ["Ten", "Eleven"]);
        assert_eq!(merged[1].notes, ["Two", "Long line"]);
    }

    #[test]
    fn hard_wrapped_bullets_are_joined_one_per_line() {
        let body = "- The launcher is now its own installer: one Setup installs it (no administrator\r\n  rights), with \
            shortcuts.\r\n  Updates swap it in place.\r\n- **Controls**: rebind\n  - a sub-item\n";
        assert_eq!(
            notes(body),
            [
                "The launcher is now its own installer: one Setup installs it (no administrator rights), with shortcuts. Updates swap it in place.",
                "**Controls**: rebind",
                "a sub-item",
            ]
        );
    }

    #[test]
    fn embedded_changelog_lists_this_version() {
        let entries = parse_changelog(CHANGELOG);
        assert!(entries
            .iter()
            .any(|e| e.version == env!("CARGO_PKG_VERSION") && !e.notes.is_empty()));
    }
}
