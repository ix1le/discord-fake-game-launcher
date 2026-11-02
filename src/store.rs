use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::model::{DetectableApp, Game};

const DETECTABLE_URL: &str = "https://discord.com/api/applications/detectable";
const USER_AGENT: &str = "Mozilla/5.0";

const LATEST_RELEASE_API: &str = "https://api.github.com/repos/ix1le/discord-fake-game-launcher/releases/latest";
const RELEASES_PAGE: &str = "https://github.com/ix1le/discord-fake-game-launcher/releases/latest";
const RELEASES_PREFIX: &str = "https://github.com/ix1le/discord-fake-game-launcher/";

#[derive(Debug, Clone)]
pub struct Paths {
    pub dir: PathBuf,
    pub gamelist: PathBuf,
    pub my_games: PathBuf,
    pub games_root: PathBuf,
}

impl Paths {
    pub fn new() -> Self {
        let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        let dir = base.join("discord-fake-game-launcher-tui");
        Paths {
            gamelist: dir.join("gamelist.json"),
            my_games: dir.join("myGames.json"),
            games_root: dir.join("games"),
            dir,
        }
    }

    pub fn ensure_dir(&self) -> std::io::Result<()> {
        fs::create_dir_all(&self.dir)
    }
}

pub fn sync_gamelist(paths: &Paths) -> Result<bool, String> {
    paths.ensure_dir().map_err(|e| io_text(&e))?;
    let tmp = paths.gamelist.with_extension("json.tmp");
    let result = download_and_install(paths, &tmp);
    let _ = fs::remove_file(&tmp);
    result
}

fn download_and_install(paths: &Paths, tmp: &Path) -> Result<bool, String> {
    let resp = ureq::get(DETECTABLE_URL)
        .set("User-Agent", USER_AGENT)
        .set("Accept", "application/json")
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(code, _) => format!("Discord API error: HTTP {code}"),
            ureq::Error::Transport(t) => format!("Discord API error: {}", t.kind()),
        })?;

    {
        let file = File::create(tmp).map_err(|e| io_text(&e))?;
        let mut out = BufWriter::new(file);
        io::copy(&mut resp.into_reader(), &mut out)
            .map_err(|e| format!("download error: {}", io_text(&e)))?;
        out.flush().map_err(|e| io_text(&e))?;
    }

    let reader = BufReader::new(File::open(tmp).map_err(|e| io_text(&e))?);
    serde_json::from_reader::<_, Vec<DetectableApp>>(reader)
        .map_err(|e| format!("Discord returned an invalid game list: {e}"))?;

    if files_equal(tmp, &paths.gamelist) {
        return Ok(false);
    }
    fs::rename(tmp, &paths.gamelist).map_err(|e| io_text(&e))?;
    Ok(true)
}

fn files_equal(a: &Path, b: &Path) -> bool {
    let (Ok(fa), Ok(fb)) = (File::open(a), File::open(b)) else {
        return false;
    };
    let (Ok(ma), Ok(mb)) = (fa.metadata(), fb.metadata()) else {
        return false;
    };
    if ma.len() != mb.len() {
        return false;
    }
    let mut ra = BufReader::new(fa);
    let mut rb = BufReader::new(fb);
    let mut buf_a = [0u8; 8192];
    let mut buf_b = [0u8; 8192];
    loop {
        let n = match ra.read(&mut buf_a) {
            Ok(n) => n,
            Err(_) => return false,
        };
        if n == 0 {
            return true;
        }
        if rb.read_exact(&mut buf_b[..n]).is_err() {
            return false;
        }
        if buf_a[..n] != buf_b[..n] {
            return false;
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Update {
    pub tag: String,
    pub url: String,
}

#[derive(Deserialize)]
struct LatestRelease {
    tag_name: String,
    html_url: String,
}

fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let core = s.trim().trim_start_matches(['v', 'V']);
    let core = core.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().map_or(Some(0), |p| p.parse().ok())?;
    let patch = parts.next().map_or(Some(0), |p| p.parse().ok())?;
    Some((major, minor, patch))
}

fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

pub fn check_update() -> Option<Update> {
    let resp = ureq::get(LATEST_RELEASE_API)
        .set("User-Agent", USER_AGENT)
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .ok()?;
    let rel: LatestRelease = serde_json::from_reader(resp.into_reader()).ok()?;
    if !is_newer(&rel.tag_name, env!("CARGO_PKG_VERSION")) {
        return None;
    }
    let url = if rel.html_url.starts_with(RELEASES_PREFIX) {
        rel.html_url
    } else {
        RELEASES_PAGE.to_string()
    };
    Some(Update { tag: rel.tag_name, url })
}

pub fn load_gamelist(paths: &Paths) -> Result<Vec<DetectableApp>, String> {
    let raw = fs::read_to_string(&paths.gamelist).map_err(|e| io_text(&e))?;
    serde_json::from_str(&raw).map_err(|e| format!("failed to parse gamelist.json: {e}"))
}

pub fn load_library(paths: &Paths) -> Vec<Game> {
    fs::read_to_string(&paths.my_games)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save_library(paths: &Paths, list: &[Game]) -> Result<(), String> {
    paths.ensure_dir().map_err(|e| io_text(&e))?;
    let json = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    fs::write(&paths.my_games, json + "\n").map_err(|e| io_text(&e))
}

pub fn io_text(e: &std::io::Error) -> String {
    let Some(code) = e.raw_os_error() else {
        return e.to_string();
    };
    let kind = e.kind().to_string();
    if kind == "uncategorized error" || kind == "other error" {
        return format!("os error {code}");
    }
    let mut chars = kind.chars();
    let first = chars.next().map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default();
    format!("{first}{} (os error {code})", chars.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_numerically() {
        assert!(is_newer("v2.1.0", "2.0.0"));
        assert!(is_newer("2.10.0", "2.9.0"));
        assert!(is_newer("v3", "2.9.9"));
        assert!(!is_newer("v2.0.0", "2.0.0"));
        assert!(!is_newer("v1.9.0", "2.0.0"));
        assert!(!is_newer("nightly", "2.0.0"));
        assert!(!is_newer("v2.1.0-beta", "2.1.0"));
    }
}