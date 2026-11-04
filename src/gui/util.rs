use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;

use crate::gui::state::{Level, Msg};

pub fn command(program: &str) -> Command {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

pub fn tool_works(program: &str, arg: &str) -> bool {
    command(program)
        .arg(arg)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

pub fn works_with_path(program: &str, args: &[&str], path: &std::ffi::OsStr) -> bool {
    command(program)
        .args(args)
        .env("PATH", path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

pub fn host_os() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

pub fn host_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        std::env::consts::ARCH
    }
}

pub fn triple_os(triple: &str) -> &'static str {
    if triple.contains("windows") {
        "windows"
    } else if triple.contains("apple") {
        "macos"
    } else {
        "linux"
    }
}

pub fn triple_arch(triple: &str) -> &'static str {
    if triple.contains("x86_64") {
        "x86_64"
    } else if triple.contains("aarch64") {
        "aarch64"
    } else {
        "unknown"
    }
}

pub fn path_with(dir: &Path) -> std::ffi::OsString {
    let mut paths = vec![dir.to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    std::env::join_paths(paths).unwrap_or_default()
}

pub fn project_root() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            for ancestor in parent.ancestors() {
                candidates.push(ancestor.to_path_buf());
                candidates.push(ancestor.join("project"));
            }
        }
    }
    if let Ok(current_dir) = std::env::current_dir() {
        for ancestor in current_dir.ancestors() {
            candidates.push(ancestor.to_path_buf());
            candidates.push(ancestor.join("project"));
        }
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    candidates
        .into_iter()
        .find(|candidate| candidate.join("Cargo.toml").is_file())
}

pub fn read_optional_asset(relative: &str) -> Option<Vec<u8>> {
    let root = project_root()?;
    let path = root.join("assets").join(relative);
    std::fs::read(&path).ok()
}

pub fn run_streamed(
    mut cmd: Command,
    sender: &Sender<Msg>,
    base: f32,
    total: f32,
    expected_steps: f32,
) -> bool {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = sender.send(Msg::Log(
                Level::Error,
                format!("Could not run the command: {error}"),
            ));
            return false;
        }
    };

    let stdout_thread = child.stdout.take().map(|stdout| {
        let sender = sender.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let line = line.trim().to_string();
                if !line.is_empty() {
                    let _ = sender.send(Msg::Log(Level::Detail, line));
                }
            }
        })
    });

    let mut steps = 0f32;
    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let level = if trimmed.starts_with("Compiling")
                || trimmed.starts_with("Downloaded")
                || trimmed.starts_with("Downloading")
            {
                steps += 1.0;
                let within = (steps / expected_steps).min(0.95);
                let _ = sender.send(Msg::Progress(base + within / total));
                Level::Detail
            } else if trimmed.starts_with("error") {
                Level::Error
            } else if trimmed.starts_with("warning") {
                Level::Warn
            } else {
                Level::Info
            };
            let _ = sender.send(Msg::Log(level, trimmed.to_string()));
        }
    }
    if let Some(handle) = stdout_thread {
        let _ = handle.join();
    }
    child.wait().is_ok_and(|status| status.success())
}
