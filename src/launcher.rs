use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use crate::model::{sanitize_component, split_exe_path, Game};
use crate::store::Paths;

pub fn fake_exe_path(game: &Game, paths: &Paths) -> io::Result<PathBuf> {
    let (dirs, file) = split_exe_path(&game.exe).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "game has no usable executable name",
        )
    })?;

    let id = game.app_id.trim();
    let id_folder = if id.is_empty() {
        sanitize_component(&game.name)
    } else if game.name.trim().is_empty() {
        sanitize_component(id)
    } else {
        let name: String = sanitize_component(&game.name).chars().take(48).collect();
        format!("{}-{}", name.trim_end(), sanitize_component(id))
    };

    let mut path = paths.games_root.join(id_folder);
    for d in dirs {
        path.push(d);
    }
    path.push(file);
    Ok(path)
}

static DUMMY: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/dummy.bin"));

pub fn ensure_fake_exe(game: &Game, paths: &Paths) -> io::Result<PathBuf> {
    let dest = fake_exe_path(game, paths)?;

    if let Ok(meta) = fs::metadata(&dest) {
        if meta.len() == DUMMY.len() as u64 {
            return Ok(dest);
        }
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&dest, DUMMY)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o755))?;
    }
    Ok(dest)
}

pub fn launch(game: &Game, paths: &Paths) -> io::Result<(Child, PathBuf)> {
    let exe = ensure_fake_exe(game, paths)?;
    let cwd = exe
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    let display = if game.name.trim().is_empty() {
        exe.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        game.name.clone()
    };

    let mut cmd = Command::new(&exe);
    cmd.arg("--dummy")
        .arg(display)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }

    let child = cmd.spawn()?;
    Ok((child, exe))
}

pub fn remove_fake_exe(game: &Game, paths: &Paths) -> io::Result<()> {
    let path = fake_exe_path(game, paths)?;
    match fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }

    let mut dir = path.parent();
    while let Some(d) = dir {
        if d == paths.games_root || !d.starts_with(&paths.games_root) {
            break;
        }
        if fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
    Ok(())
}
