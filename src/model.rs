use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct DetectableApp {
    pub id: Option<String>,
    pub name: Option<String>,
    pub executables: Option<Vec<AppExecutable>>,
}

#[derive(Debug, Deserialize)]
pub struct AppExecutable {
    pub name: Option<String>,
    pub os: Option<String>,
    #[serde(default)]
    pub is_launcher: bool,
}

#[derive(Debug, Clone)]
pub struct DbGame {
    pub id: String,
    pub name: String,
    pub exe: String,

    pub uses_new_detection: bool,
    pub name_lower: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Game {
    #[serde(rename = "appId", default)]
    pub app_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub exe: String,
    #[serde(rename = "isFavorite", default)]
    pub is_favorite: bool,
}

impl Game {
    pub fn key(&self) -> String {
        make_game_key(&self.app_id, &self.exe)
    }
}

pub fn make_game_key(app_id: &str, exe: &str) -> String {
    format!("{app_id}::{exe}")
}

impl From<&DbGame> for Game {
    fn from(g: &DbGame) -> Self {
        Game {
            app_id: g.id.clone(),
            name: g.name.clone(),
            exe: g.exe.clone(),
            is_favorite: false,
        }
    }
}

pub fn host_os() -> &'static str {
    match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        _ => "linux",
    }
}

fn os_matches(e: &AppExecutable, os: &str) -> bool {
    e.os.as_deref()
        .map(|o| o.eq_ignore_ascii_case(os))
        .unwrap_or(false)
        && e.name
            .as_deref()
            .map(|n| !n.trim().is_empty())
            .unwrap_or(false)
}

pub fn pick_best_executable<'a>(app: &'a DetectableApp, os: &str) -> Option<&'a AppExecutable> {
    let exes = app.executables.as_deref().unwrap_or(&[]);
    exes.iter()
        .find(|e| os_matches(e, os) && !e.is_launcher)
        .or_else(|| exes.iter().find(|e| os_matches(e, os)))
}

pub fn sanitize_component(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed: String = cleaned.trim().chars().take(128).collect();
    if trimmed.is_empty() {
        "_".to_string()
    } else {
        trimmed
    }
}

pub fn fallback_exe_name(name: &str, os: &str) -> String {
    let base = sanitize_component(name);
    let base = base.split_whitespace().collect::<Vec<_>>().join(" ");
    let base: String = base.chars().take(120).collect();
    let base = if base.is_empty() {
        "Game".to_string()
    } else {
        base
    };
    if os == "win32" && !base.to_lowercase().ends_with(".exe") {
        format!("{base}.exe")
    } else {
        base
    }
}

pub fn to_db_games(apps: &[DetectableApp], os: &str) -> Vec<DbGame> {
    let mut out: Vec<DbGame> = apps
        .iter()
        .filter_map(|app| {
            let name = app.name.as_deref()?.trim();
            if name.is_empty() {
                return None;
            }
            let best = pick_best_executable(app, os);
            let exe = match best.and_then(|b| b.name.clone()) {
                Some(n) => n,
                None => fallback_exe_name(name, os),
            };
            Some(DbGame {
                id: app.id.clone().unwrap_or_default(),
                name: name.to_string(),
                exe,
                uses_new_detection: best.is_none(),
                name_lower: name.to_lowercase(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.name_lower.cmp(&b.name_lower));
    out
}

pub fn filter_indices(games: &[DbGame], filter: &str) -> Vec<usize> {
    let term = filter.trim().to_lowercase();
    games
        .iter()
        .enumerate()
        .filter(|(_, g)| term.is_empty() || g.name_lower.contains(&term))
        .map(|(i, _)| i)
        .collect()
}

pub fn split_exe_path(exe: &str) -> Option<(Vec<String>, String)> {
    let mut parts: Vec<String> = exe
        .split(|c| c == '/' || c == '\\')
        .filter(|p| !p.is_empty() && *p != "." && *p != "..")
        .map(sanitize_component)
        .collect();
    let file = parts.pop()?;
    Some((parts, file))
}

pub fn sort_library(list: &mut [Game]) {
    list.sort_by(|a, b| {
        b.is_favorite
            .cmp(&a.is_favorite)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<DetectableApp> {
        serde_json::from_str(
            r#"[
              {"id":"1","name":"Counter-Strike 2","executables":[
                {"name":"cs2.exe","os":"win32","is_launcher":false},
                {"name":"cs2","os":"linux","is_launcher":false}]},
              {"id":"2","name":"Apex Legends","executables":[
                {"name":"launcher.exe","os":"win32","is_launcher":true},
                {"name":"r5apex.exe","os":"win32","is_launcher":false}]},
              {"id":"3","name":"Mac Only","executables":[{"name":"mac","os":"darwin"}]},
              {"id":"4","name":"No Exes","executables":null},
              {"id":"5","name":"","executables":[]}
            ]"#,
        )
        .unwrap()
    }

    #[test]
    fn prefers_non_launcher() {
        let apps = sample();
        let best = pick_best_executable(&apps[1], "win32").unwrap();
        assert_eq!(best.name.as_deref(), Some("r5apex.exe"));
    }

    #[test]
    fn builds_sorted_list_and_skips_nameless() {
        let games = to_db_games(&sample(), "win32");
        let names: Vec<_> = games.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(
            names,
            ["Apex Legends", "Counter-Strike 2", "Mac Only", "No Exes"]
        );
    }

    #[test]
    fn marks_new_detection_with_fallback_exe() {
        let games = to_db_games(&sample(), "win32");
        let mac = games.iter().find(|g| g.name == "Mac Only").unwrap();
        assert!(mac.uses_new_detection);
        assert_eq!(mac.exe, "Mac Only.exe");
        let cs = games.iter().find(|g| g.id == "1").unwrap();
        assert!(!cs.uses_new_detection);
    }

    #[test]
    fn linux_fallback_has_no_exe_suffix() {
        assert_eq!(fallback_exe_name("Some  Game", "linux"), "Some Game");
    }

    #[test]
    fn filter_is_case_insensitive() {
        let games = to_db_games(&sample(), "win32");
        let idx = filter_indices(&games, "  STRIKE ");
        assert_eq!(idx.len(), 1);
        assert_eq!(games[idx[0]].id, "1");
        assert_eq!(filter_indices(&games, "").len(), games.len());
    }

    #[test]
    fn exe_path_cannot_escape() {
        let (dirs, file) = split_exe_path("..\\..\\evil/../win64\\cs2.exe").unwrap();
        assert_eq!(dirs, ["evil", "win64"]);
        assert_eq!(file, "cs2.exe");
        assert!(split_exe_path("../..").is_none());
    }

    #[test]
    fn library_sorts_favorites_first() {
        let g = |n: &str, f: bool| Game {
            app_id: "1".into(),
            name: n.into(),
            exe: "x".into(),
            is_favorite: f,
        };
        let mut l = vec![g("b", false), g("c", true), g("a", false)];
        sort_library(&mut l);
        let names: Vec<_> = l.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["c", "a", "b"]);
    }

    #[test]
    fn library_json_matches_electron_format() {
        let g: Game =
            serde_json::from_str(r#"{"appId":"9","name":"N","exe":"a/b.exe","isFavorite":true}"#)
                .unwrap();
        assert_eq!(g.key(), "9::a/b.exe");
        assert!(g.is_favorite);
    }
}
