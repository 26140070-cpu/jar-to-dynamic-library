use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

use crate::gui::state::{Level, Msg};

const FILE_NAME: &str = "jar-to-dynamic-library-log.txt";
const PREFIX: &str = "JTDL|";
const TIMEOUT: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(300);

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

pub fn log_path() -> Option<PathBuf> {
    home_dir().map(|dir| dir.join(FILE_NAME))
}

fn level_of(tag: &str) -> Level {
    match tag {
        "ok" => Level::Ok,
        "warn" => Level::Warn,
        "error" => Level::Error,
        "detail" => Level::Detail,
        _ => Level::Info,
    }
}

fn parse(line: &str) -> Option<(Level, String)> {
    let rest = line.strip_prefix(PREFIX)?;
    let mut parts = rest.splitn(3, '|');
    let level = parts.next()?;
    let component = parts.next()?;
    let message = parts.next().unwrap_or("");
    Some((level_of(level), format!("[game:{component}] {message}")))
}

pub fn follow(sender: Sender<Msg>, session_id: String, pid: u32) {
    let Some(path) = log_path() else {
        let _ = sender.send(Msg::Log(
            Level::Warn,
            "Cannot locate the injector report file (HOME is unset); check ~/jar-to-dynamic-library-log.txt."
                .to_string(),
        ));
        let _ = sender.send(Msg::ReportFinished(pid));
        return;
    };

    let marker = format!("JTDL|detail|session|{session_id}");
    let deadline = Instant::now() + TIMEOUT;
    let mut marker_seen = false;
    let mut emitted = 0usize;
    let mut saw_report = false;
    let mut completed = false;

    while Instant::now() < deadline {
        let lines = structured_lines(&path);
        if !marker_seen {
            if let Some(index) = lines.iter().position(|line| line == &marker) {
                marker_seen = true;
                emitted = index + 1;
            }
        }

        if marker_seen && lines.len() > emitted {
            saw_report = true;
            for line in lines.iter().skip(emitted) {
                if let Some((level, text)) = parse(line) {
                    let _ = sender.send(Msg::Log(level, text));
                }
                if line.contains("|done|") {
                    completed = true;
                    break;
                }
            }
            emitted = lines.len();
            if completed {
                break;
            }
        }
        thread::sleep(POLL);
    }

    if !completed {
        if saw_report {
            let _ = sender.send(Msg::Log(
                Level::Warn,
                "Timed out before the injector wrote its final report line.".to_string(),
            ));
        } else {
            let _ = sender.send(Msg::Log(
                Level::Warn,
                format!(
                    "No report for attach session {session_id} appeared in {} within 60s. \
                     The JVM accepted the attach, but the agent may not have started or the target may use a different home directory.",
                    path.display()
                ),
            ));
        }
    }
    let _ = sender.send(Msg::ReportFinished(pid));
}

fn structured_lines(path: &Path) -> Vec<String> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter(|line| line.starts_with(PREFIX))
        .collect()
}
