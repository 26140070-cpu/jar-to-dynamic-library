use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use glacex::{
    Alignment, AnyWidget, App, Button, ButtonStyle, Color, Column, Fill, FontFamily, ImageHandle, Label,
    Measurable, MeasurablePath, Path as ShapePath, ProgressBar, ScrollView, ScrollViewStyle, Stateful,
    TextStyle, Ui, Widget,
};

const IMAGES: [(&str, &[u8]); 5] = [
    ("hero.png", include_bytes!("../../assets/images/hero.png")),
    ("jar.png", include_bytes!("../../assets/images/jar.png")),
    ("linux.png", include_bytes!("../../assets/images/linux.png")),
    ("windows.png", include_bytes!("../../assets/images/windows.png")),
    ("macos.png", include_bytes!("../../assets/images/macos.png")),
];

const UI_FONT: &[u8] = include_bytes!("../../assets/fonts/fingerpaint.ttf");
const UI_FONT_FAMILY: FontFamily = FontFamily::Custom("Finger Paint");

const INJECTOR_CLASS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ForgeInjector.class"));

const BG: &str = "#0b0e17";
const PANEL: &str = "#141927";
const PANEL_SELECTED: &str = "#1b2040";
const PANEL_HOVERED: &str = "#222941";
const PANEL_PRESSED: &str = "#29314d";
const BORDER: &str = "#272e47";
const ACCENT: &str = "#7c5cff";

const MARGIN: f32 = 24.0;
const MAX_CONTENT_W: f32 = 1080.0;
const CARD_MIN_W: f32 = 176.0;
const CARD_GAP: f32 = 16.0;
const CARD_H: f32 = 120.0;

struct BuildTarget {
    name: &'static str,
    detail: &'static str,
    icon: usize,
    triple: &'static str,
    artifact: &'static str,
    suffix: &'static str,
}

fn project_root() -> Option<PathBuf> {
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

const TARGETS: [BuildTarget; 4] = [
    BuildTarget {
        name: "Linux",
        detail: ".so  x86_64",
        icon: 2,
        triple: "x86_64-unknown-linux-gnu",
        artifact: "libjar_to_dynamic_library.so",
        suffix: "linux-x86_64.so",
    },
    BuildTarget {
        name: "Windows",
        detail: ".dll  x86_64",
        icon: 3,
        triple: "x86_64-pc-windows-msvc",
        artifact: "jar_to_dynamic_library.dll",
        suffix: "windows-x86_64.dll",
    },
    BuildTarget {
        name: "macOS Intel",
        detail: ".dylib  x86_64",
        icon: 4,
        triple: "x86_64-apple-darwin",
        artifact: "libjar_to_dynamic_library.dylib",
        suffix: "macos-x86_64.dylib",
    },
    BuildTarget {
        name: "macOS Apple Silicon",
        detail: ".dylib  arm64",
        icon: 4,
        triple: "aarch64-apple-darwin",
        artifact: "libjar_to_dynamic_library.dylib",
        suffix: "macos-aarch64.dylib",
    },
];

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Info,
    Ok,
    Warn,
    Error,
    Muted,
}

enum Msg {
    Log(Kind, String),
    Progress(f32),
    Done,
}

struct BuilderApp {
    jar: Option<PathBuf>,
    selected: [bool; TARGETS.len()],
    building: bool,
    progress: f32,
    log: Vec<(Kind, String)>,
    log_needs_follow: bool,
    receiver: Option<Receiver<Msg>>,
    images: Vec<Option<ImageHandle>>,
}

impl Default for BuilderApp {
    fn default() -> Self {
        Self {
            jar: None,
            selected: host_target_selection(),
            building: false,
            progress: 0.0,
            log: vec![(Kind::Muted, "Select a .jar file to get started.".into())],
            log_needs_follow: false,
            receiver: None,
            images: Vec::new(),
        }
    }
}

impl BuilderApp {
    fn push_log(&mut self, kind: Kind, text: impl AsRef<str>) {
        self.log.push((kind, text.as_ref().to_string()));
        if self.log.len() > 500 {
            let extra = self.log.len() - 500;
            self.log.drain(..extra);
        }
    }

    fn save_log(&mut self) {
        let default_name = "jar-to-dynamic-library-build.log";
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Build log", &["log", "txt"])
            .set_file_name(default_name)
            .save_file()
        else {
            return;
        };

        let content = self
            .log
            .iter()
            .map(|(kind, line)| format!("[{}] {line}", kind_name(*kind)))
            .collect::<Vec<_>>()
            .join("\n");
        match std::fs::write(&path, format!("{content}\n")) {
            Ok(()) => self.push_log(Kind::Ok, format!("Log saved to {}", path.display())),
            Err(error) => self.push_log(Kind::Error, format!("Could not save the log: {error}")),
        }
    }

    fn accept_jar(&mut self, path: PathBuf) {
        if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("jar"))
        {
            self.push_log(Kind::Ok, format!("Selected JAR: {}", path.display()));
            self.jar = Some(path);
        } else {
            self.push_log(Kind::Error, "The selected file does not have a .jar extension.");
        }
    }

    fn start_build(&mut self) {
        let Some(jar) = self.jar.clone() else {
            self.push_log(Kind::Warn, "Select a .jar file first.");
            return;
        };
        let selected: Vec<usize> = (0..TARGETS.len()).filter(|i| self.selected[*i]).collect();
        if selected.is_empty() {
            self.push_log(Kind::Warn, "Select at least one target platform.");
            return;
        }

        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.building = true;
        self.progress = 0.0;
        self.log.clear();
        self.push_log(Kind::Info, "Preparing the build...");
        thread::spawn(move || build_libraries(jar, selected, sender));
    }

    fn poll_build(&mut self) {
        let mut messages = Vec::new();
        if let Some(receiver) = &self.receiver {
            while let Ok(message) = receiver.try_recv() {
                messages.push(message);
            }
        }
        for message in messages {
            match message {
                Msg::Log(kind, text) => self.push_log(kind, text),
                Msg::Progress(value) => self.progress = value.clamp(0.0, 1.0),
                Msg::Done => {
                    self.building = false;
                    self.receiver = None;
                }
            }
        }
    }

    fn label(text: impl Into<String>, size: f32) -> Label {
        Label::new(text)
            .size_with_line_height(size, size * 1.5)
            .family(UI_FONT_FAMILY)
    }

    fn button_text() -> TextStyle {
        TextStyle::new().family(UI_FONT_FAMILY)
    }

    fn panel(ui: &mut Ui, pos: [f32; 2], size: [f32; 2], fill: &str, border: &str, bw: f32) {
        ui.draw_shape(
            MeasurablePath::rect(pos, size, [14.0; 4]),
            Fill::Solid(Color::hex_str(fill)),
            bw,
            Color::hex_str(border),
            0.0,
            false,
            0.0,
        );
    }

    fn image(&self, ui: &mut Ui, index: usize, pos: [f32; 2], size: [f32; 2]) {
        if let Some(Some(handle)) = self.images.get(index) {
            ui.draw_shape(
                MeasurablePath::rect(pos, size, [0.0; 4]),
                Fill::Image(*handle),
                0.0,
                Color::TRANSPARENT,
                0.0,
                false,
                0.0,
            );
        }
    }
}

fn put<'a>(ui: &mut Ui, pos: [f32; 2], widget: &'a mut impl Measurable) {
    let mut col = Column::new(vec![Box::new(widget) as Box<dyn AnyWidget + 'a>])
        .align(Alignment::Start)
        .padding([0.0, 0.0]);
    col.arrange_at(pos, ui);
}

impl Widget for BuilderApp {
    type Output = ();

    fn on_start(&mut self, ui: &mut Ui) {
        let _ = ui.load_font_bytes(UI_FONT.to_vec());
        let dir = std::env::temp_dir().join("jar-to-dynamic-library-gui");
        let _ = std::fs::create_dir_all(&dir);
        self.images = IMAGES
            .iter()
            .map(|(name, bytes)| {
                let path = dir.join(name);
                std::fs::write(&path, bytes).ok()?;
                ui.load_image(path.to_str()?).ok()
            })
            .collect();
    }

    fn ui(&mut self, ui: &mut Ui) {
        ui.set_bgcolor(Color::hex_str(BG));
        let window = ui.window_size();
        let compact = window[0] < 620.0;
        let margin = if compact { 14.0 } else { MARGIN };
        let content_w = (window[0] - margin * 2.0).max(260.0).min(MAX_CONTENT_W);
        let left = ((window[0] - content_w) / 2.0).max(margin);
        let hero_h = if compact { 112.0 } else { 150.0 };

        self.image(ui, 0, [left, 20.0], [content_w, hero_h]);
        let mut title = Self::label("JAR to Dynamic Library", 30.0).bold();
        let mut subtitle =
            Self::label("Turn your .jar mod into a .dll / .so / .dylib library", 14.0).secondary();
        put(ui, [left + 22.0, 42.0], &mut title);
        put(ui, [left + 22.0, 92.0], &mut subtitle);

        let mut step1 = Self::label("1   Choose your JAR file", 15.0).bold();
        let step1_y = hero_h + 38.0;
        put(ui, [left, step1_y], &mut step1);
        let jar_y = step1_y + 32.0;
        let jar_h = if compact { 116.0 } else { 84.0 };
        Self::panel(ui, [left, jar_y], [content_w, jar_h], PANEL, BORDER, 1.0);
        self.image(ui, 1, [left + 16.0, jar_y + 14.0], [56.0, 56.0]);

        let jar_name = self
            .jar
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "No file selected".into());
        let jar_dir = self
            .jar
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| shorten(&p.display().to_string(), 58))
            .unwrap_or_else(|| "Select a JAR file to choose your mod".into());
        let text_w = if compact {
            content_w - 32.0
        } else {
            content_w - 270.0
        };
        let mut jar_label = Self::label(shorten(&jar_name, if compact { 34 } else { 72 }), 16.0)
            .width(text_w)
            .bold();
        let mut jar_path = Self::label(shorten(&jar_dir, if compact { 42 } else { 88 }), 12.0)
            .width(text_w)
            .muted();
        let text_x = if compact { left + 16.0 } else { left + 90.0 };
        let text_y = if compact { jar_y + 74.0 } else { jar_y + 14.0 };
        put(ui, [text_x, text_y], &mut jar_label);
        put(ui, [text_x, text_y + 29.0], &mut jar_path);

        let mut browse = Button::new("Select JAR")
            .id("browse-jar")
            .size([150.0, 44.0])
            .text_style(Self::button_text())
            .primary();
        let browse_pos = if compact {
            [left + content_w - 166.0, jar_y + 14.0]
        } else {
            [left + content_w - 166.0, jar_y + 20.0]
        };
        put(ui, browse_pos, &mut browse);

        let selected_count = self.selected.iter().filter(|selected| **selected).count();
        let mut step2 = Self::label(
            format!("2   Select target platforms ({selected_count} selected)"),
            15.0,
        )
        .bold();
        let step2_y = jar_y + jar_h + 18.0;
        put(ui, [left, step2_y], &mut step2);

        let mut cards: Vec<Button> = Vec::new();
        let columns = ((content_w + CARD_GAP) / (CARD_MIN_W + CARD_GAP))
            .floor()
            .clamp(1.0, TARGETS.len() as f32) as usize;
        let card_w = (content_w - CARD_GAP * (columns.saturating_sub(1) as f32)) / columns as f32;
        let cards_y = step2_y + 32.0;
        for (i, target) in TARGETS.iter().enumerate() {
            let column = i % columns;
            let row = i / columns;
            let x = left + column as f32 * (card_w + CARD_GAP);
            let y = cards_y + row as f32 * (CARD_H + CARD_GAP);
            let on = self.selected[i];
            if on {
                Self::panel(ui, [x, y], [card_w, CARD_H], PANEL_SELECTED, ACCENT, 2.0);
            } else {
                Self::panel(ui, [x, y], [card_w, CARD_H], PANEL, BORDER, 1.0);
            }

            let mut hit_style = ButtonStyle::ghost(ui.theme());
            hit_style.fill = Stateful::new(Fill::Solid(Color::TRANSPARENT))
                .hovered(Fill::Solid(Color::hex_str(PANEL_HOVERED)))
                .pressed(Fill::Solid(Color::hex_str(PANEL_PRESSED)));
            hit_style.path = Stateful::new(ShapePath::rect([14.0; 4]));
            let mut hit = Button::new("")
                .id(format!("target-{}", target.triple))
                .size([card_w, CARD_H])
                .style(hit_style)
                .ghost();
            put(ui, [x, y], &mut hit);
            cards.push(hit);

            self.image(
                ui,
                target.icon,
                [x + (card_w - 44.0) / 2.0, y + 12.0],
                [44.0, 44.0],
            );

            let mut name = Self::label(target.name, 14.0)
                .width(card_w)
                .align(Alignment::Center)
                .bold();
            let mut detail = Self::label(target.detail, 11.0)
                .width(card_w)
                .align(Alignment::Center)
                .muted();
            let mut state = if on {
                Self::label("Selected", 11.0).success()
            } else {
                Self::label("Click to select", 11.0).muted()
            }
            .width(card_w)
            .align(Alignment::Center);
            put(ui, [x, y + 60.0], &mut name);
            put(ui, [x, y + 82.0], &mut detail);
            put(ui, [x, y + 98.0], &mut state);
        }

        let after_cards = cards_y
            + ((TARGETS.len() + columns - 1) / columns) as f32 * CARD_H
            + ((TARGETS.len() + columns - 1) / columns).saturating_sub(1) as f32 * CARD_GAP;
        let output_location = self
            .jar
            .as_ref()
            .and_then(|path| {
                let stem = path.file_stem()?.to_string_lossy();
                let parent = path.parent().unwrap_or_else(|| Path::new("."));
                Some(shorten(
                    &parent.join(format!("{stem}-libraries")).display().to_string(),
                    100,
                ))
            })
            .unwrap_or_else(|| "The output folder will appear next to your JAR.".into());
        let mut output_hint = Self::label(format!("Output: {output_location}"), 11.0)
            .width(content_w)
            .muted();
        put(ui, [left, after_cards + 14.0], &mut output_hint);

        let mut build = Button::new(if self.building {
            "Building..."
        } else {
            "Build libraries"
        })
        .id("build-libraries")
        .size([content_w, 52.0])
        .text_style(Self::button_text())
        .primary();
        put(ui, [left, after_cards + 38.0], &mut build);

        let mut bar = ProgressBar::new(self.progress)
            .id("progress")
            .size([content_w, 10.0]);
        if !self.building && self.progress >= 1.0 {
            bar = bar.success();
        }
        put(ui, [left, after_cards + 102.0], &mut bar);

        let panel_y = after_cards + 126.0;
        let panel_h = (window[1] - panel_y - margin).clamp(120.0, 330.0);
        Self::panel(ui, [left, panel_y], [content_w, panel_h], PANEL, BORDER, 1.0);
        let mut log_title = Self::label(format!("Build log · {} entries", self.log.len()), 13.0).bold();
        put(ui, [left + 16.0, panel_y + 8.0], &mut log_title);
        let mut download = Button::new("Download")
            .id("download-build-log")
            .size([92.0, 28.0])
            .text_style(Self::button_text())
            .ghost();
        let mut top = Button::new("↑")
            .id("log-top")
            .size([30.0, 28.0])
            .text_style(Self::button_text())
            .ghost();
        let mut bottom = Button::new("↓")
            .id("log-bottom")
            .size([30.0, 28.0])
            .text_style(Self::button_text())
            .ghost();
        put(ui, [left + content_w - 176.0, panel_y + 6.0], &mut top);
        put(ui, [left + content_w - 140.0, panel_y + 6.0], &mut bottom);
        put(ui, [left + content_w - 98.0, panel_y + 6.0], &mut download);

        let mut lines: Vec<Label> = self
            .log
            .iter()
            .map(|(kind, text)| {
                let label = Self::label(text.clone(), 12.0).width(content_w - 48.0);
                match kind {
                    Kind::Info => label,
                    Kind::Ok => label.success(),
                    Kind::Warn => label.warning(),
                    Kind::Error => label.error(),
                    Kind::Muted => label.muted(),
                }
            })
            .collect();
        let children: Vec<Box<dyn AnyWidget + '_>> = lines
            .iter_mut()
            .map(|l| Box::new(l) as Box<dyn AnyWidget + '_>)
            .collect();
        let mut log_content = Column::new(children)
            .spacing(0.0)
            .align(Alignment::Start)
            .padding([0.0, 0.0]);
        let mut log_view = ScrollView::new(&mut log_content)
            .id("build-log-scroll")
            .size([content_w - 20.0, panel_h - 44.0])
            .padding([0.0, 0.0])
            .style(ScrollViewStyle::panel(ui.theme()));
        put(ui, [left + 10.0, panel_y + 38.0], &mut log_view);
        if top.clicked() {
            log_view.set_offset(ui, [0.0, 0.0]);
        }
        if bottom.clicked() || self.log_needs_follow {
            log_view.set_offset(ui, [0.0, 1_000_000.0]);
            self.log_needs_follow = false;
        }

        for (index, card) in cards.iter().enumerate() {
            if card.clicked() && !self.building {
                self.selected[index] = !self.selected[index];
            }
        }
        if browse.clicked() && !self.building {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Java archive", &["jar"])
                .pick_file()
            {
                self.accept_jar(path);
            }
        }
        if build.clicked() && !self.building {
            self.start_build();
            self.log_needs_follow = true;
        }
        if download.clicked() {
            self.save_log();
        }
    }
}

fn host_target_selection() -> [bool; TARGETS.len()] {
    let host = if cfg!(target_os = "windows") {
        "x86_64-pc-windows-msvc"
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "aarch64-apple-darwin"
        } else {
            "x86_64-apple-darwin"
        }
    } else {
        "x86_64-unknown-linux-gnu"
    };
    TARGETS.map(|target| target.triple == host)
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Info => "INFO",
        Kind::Ok => "OK",
        Kind::Warn => "WARN",
        Kind::Error => "ERROR",
        Kind::Muted => "DETAIL",
    }
}

fn shorten(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    let tail: String = chars[chars.len() - (max - 3)..].iter().collect();
    format!("...{tail}")
}

fn command(program: &str) -> Command {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

fn tool_works(program: &str, arg: &str) -> bool {
    command(program)
        .arg(arg)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn host_os() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

fn triple_os(triple: &str) -> &'static str {
    if triple.contains("windows") {
        "windows"
    } else if triple.contains("apple") {
        "macos"
    } else {
        "linux"
    }
}

fn path_with(dir: &Path) -> std::ffi::OsString {
    let mut paths = vec![dir.to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    std::env::join_paths(paths).unwrap_or_default()
}

fn works_with_path(program: &str, args: &[&str], path: &std::ffi::OsStr) -> bool {
    command(program)
        .args(args)
        .env("PATH", path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn run_streamed(mut cmd: Command, sender: &Sender<Msg>, base: f32, total: f32, expected_steps: f32) -> bool {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = sender.send(Msg::Log(
                Kind::Error,
                format!("Could not run the command: {error}"),
            ));
            return false;
        }
    };

    let stdout_thread = child.stdout.take().map(|stdout| {
        let sender = sender.clone();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let line = line.trim().to_string();
                if !line.is_empty() {
                    let _ = sender.send(Msg::Log(Kind::Muted, line));
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
            let kind = if trimmed.starts_with("Compiling")
                || trimmed.starts_with("Downloaded")
                || trimmed.starts_with("Downloading")
            {
                steps += 1.0;
                let within = (steps / expected_steps).min(0.95);
                let _ = sender.send(Msg::Progress(base + within / total));
                Kind::Muted
            } else if trimmed.starts_with("error") {
                Kind::Error
            } else if trimmed.starts_with("warning") {
                Kind::Warn
            } else {
                Kind::Info
            };
            let _ = sender.send(Msg::Log(kind, trimmed.to_string()));
        }
    }
    if let Some(handle) = stdout_thread {
        let _ = handle.join();
    }
    child.wait().is_ok_and(|status| status.success())
}

fn zig_works(path: &std::ffi::OsStr) -> bool {
    works_with_path("zig", &["version"], path)
        || works_with_path("python3", &["-m", "ziglang", "version"], path)
        || works_with_path("python", &["-m", "ziglang", "version"], path)
}

fn ensure_zig(sender: &Sender<Msg>, base: f32, total: f32) -> Option<std::ffi::OsString> {
    let log = |kind: Kind, text: &str| {
        let _ = sender.send(Msg::Log(kind, text.to_string()));
    };
    let current = std::env::var_os("PATH").unwrap_or_default();
    if zig_works(&current) {
        return Some(current);
    }

    let venv = std::env::temp_dir()
        .join("jar-to-dynamic-library-gui")
        .join("zig-venv");
    let bin = if cfg!(windows) {
        venv.join("Scripts")
    } else {
        venv.join("bin")
    };
    let with_venv = path_with(&bin);
    if zig_works(&with_venv) {
        return Some(with_venv);
    }

    log(
        Kind::Info,
        "Installing Zig for cross-compilation (first run only)...",
    );
    let python = ["python3", "python", "py"]
        .into_iter()
        .find(|name| tool_works(name, "--version"));
    let Some(python) = python else {
        log(
            Kind::Error,
            "Python is required to install Zig. Install Python 3 or install Zig with your package manager.",
        );
        return None;
    };

    let mut create = command(python);
    create.args(["-m", "venv"]).arg(&venv);
    if !run_streamed(create, sender, base, total, 50.0) {
        log(
            Kind::Error,
            "Could not create the Python environment. On Debian/Ubuntu, install python3-venv.",
        );
        return None;
    }
    let venv_python = bin.join(if cfg!(windows) { "python.exe" } else { "python" });
    let mut install = command(&venv_python.to_string_lossy());
    install.args(["-m", "pip", "install", "--upgrade", "ziglang"]);
    if !run_streamed(install, sender, base, total, 50.0) {
        log(
            Kind::Error,
            "Could not install Zig with pip. Install it with your package manager and try again.",
        );
        return None;
    }
    if zig_works(&with_venv) {
        log(Kind::Ok, "Zig installed.");
        Some(with_venv)
    } else {
        log(Kind::Error, "Zig was installed but did not run successfully.");
        None
    }
}

fn ensure_zigbuild(root: &Path, path: &std::ffi::OsStr, sender: &Sender<Msg>, base: f32, total: f32) -> bool {
    if works_with_path("cargo", &["zigbuild", "--help"], path) {
        return true;
    }
    let _ = sender.send(Msg::Log(
        Kind::Info,
        "Installing cargo-zigbuild (this may take a few minutes the first time)...".into(),
    ));
    let mut install = command("cargo");
    install
        .args(["install", "--locked", "cargo-zigbuild", "--color", "never"])
        .current_dir(root)
        .env("PATH", path);
    run_streamed(install, sender, base, total, 150.0)
        && works_with_path("cargo", &["zigbuild", "--help"], path)
}

fn ensure_xwin(root: &Path, path: &std::ffi::OsStr, sender: &Sender<Msg>, base: f32, total: f32) -> bool {
    if !tool_works("clang", "--version") {
        let _ = sender.send(Msg::Log(
            Kind::Error,
            "Cross-compiling for Windows requires LLVM/Clang. Install LLVM, add clang to PATH, and try again."
                .into(),
        ));
        return false;
    }
    if works_with_path("cargo", &["xwin", "--help"], path) {
        return true;
    }
    let _ = sender.send(Msg::Log(
        Kind::Info,
        "Installing cargo-xwin for Windows cross-compilation...".into(),
    ));
    let mut install = command("cargo");
    install
        .args(["install", "--locked", "cargo-xwin", "--color", "never"])
        .current_dir(root)
        .env("PATH", path);
    run_streamed(install, sender, base, total, 150.0) && works_with_path("cargo", &["xwin", "--help"], path)
}

fn build_libraries(jar: PathBuf, selected: Vec<usize>, sender: Sender<Msg>) {
    let log = |kind: Kind, text: String| {
        let _ = sender.send(Msg::Log(kind, text));
    };
    let finish = || {
        let _ = sender.send(Msg::Done);
    };

    let Some(root) = project_root() else {
        log(
            Kind::Error,
            "Project files were not found. Extract the complete desktop package or run the GUI from a project checkout."
                .into(),
        );
        return finish();
    };
    if !tool_works("cargo", "--version") {
        log(
            Kind::Error,
            "Cargo was not found. Install Rust from https://rustup.rs.".into(),
        );
        return finish();
    }
    let workspace = std::env::temp_dir().join("jar-to-dynamic-library-gui");
    if let Err(error) = std::fs::create_dir_all(&workspace) {
        log(
            Kind::Error,
            format!("Could not create the temporary build folder: {error}"),
        );
        return finish();
    }
    let injector_class = workspace.join("ForgeInjector.class");
    if let Err(error) = std::fs::write(&injector_class, INJECTOR_CLASS) {
        log(
            Kind::Error,
            format!("Could not prepare the Java injector: {error}"),
        );
        return finish();
    }
    log(
        Kind::Ok,
        "Cargo found. The bundled Java injector is ready.".into(),
    );

    let target_dir = workspace.join("build");
    let stem = jar
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("mod")
        .to_string();
    let output_dir = jar
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{stem}-libraries"));
    if let Err(error) = std::fs::create_dir_all(&output_dir) {
        log(
            Kind::Error,
            format!("Could not create the output folder: {error}"),
        );
        return finish();
    }

    let total = selected.len() as f32;
    let has_rustup = tool_works("rustup", "--version");
    let mut built = 0usize;

    for (position, index) in selected.iter().enumerate() {
        let target = &TARGETS[*index];
        let triple = target.triple;
        let base = position as f32 / total;
        let _ = sender.send(Msg::Progress(base));
        log(
            Kind::Info,
            format!("[{}/{}] {}", position + 1, selected.len(), target.name),
        );

        let cross = triple_os(triple) != host_os();
        let mut path = std::env::var_os("PATH").unwrap_or_default();
        let cross_windows = cross && triple_os(triple) == "windows";
        if cross {
            if cross_windows {
                log(
                    Kind::Muted,
                    format!("Preparing a Windows cross-build from {}...", host_os()),
                );
                if !ensure_xwin(&root, &path, &sender, base, total) {
                    log(
                        Kind::Error,
                        format!("Could not prepare the Windows build for {}.", target.name),
                    );
                    continue;
                }
            } else {
                log(
                    Kind::Muted,
                    format!(
                        "{} is not your host system ({}); preparing a cross-build with Zig.",
                        target.name,
                        host_os()
                    ),
                );
                let Some(zig_path) = ensure_zig(&sender, base, total) else {
                    log(
                        Kind::Error,
                        format!("Zig is required to cross-compile for {}.", target.name),
                    );
                    continue;
                };
                if !ensure_zigbuild(&root, &zig_path, &sender, base, total) {
                    log(
                        Kind::Error,
                        format!("Could not install cargo-zigbuild; skipping {}.", target.name),
                    );
                    continue;
                }
                path = zig_path;
            }
        }

        if has_rustup {
            log(Kind::Muted, format!("Checking the {triple} target..."));
            let mut add_target = command("rustup");
            add_target.args(["target", "add", triple]);
            let added = run_streamed(add_target, &sender, base, total, 3.0);
            if !added {
                log(Kind::Warn, format!("Could not install the Rust target {triple}."));
            }
        } else if cross {
            log(
                Kind::Warn,
                "rustup was not found, so the cross-compilation target cannot be installed. Install Rust from https://rustup.rs."
                    .into(),
            );
        }
        log(
            Kind::Muted,
            "Building (the first build may take several minutes)...".into(),
        );

        let mut build = command("cargo");
        if cross_windows {
            build.args(["xwin", "build"]).env("XWIN_CROSS_COMPILER", "clang");
        } else {
            build.arg(if cross { "zigbuild" } else { "build" });
        }
        build
            .args([
                "--release",
                "--lib",
                "--color",
                "never",
                "--locked",
                "--target",
                triple,
            ])
            .arg("--target-dir")
            .arg(&target_dir)
            .current_dir(&root)
            .env("INPUT_JAR", &jar)
            .env("INJECTOR_CLASS_FILE", &injector_class)
            .env("PATH", &path);
        if !run_streamed(build, &sender, base, total, 60.0) {
            log(
                Kind::Error,
                format!("The {} build failed. See the error messages above.", target.name),
            );
            continue;
        }

        let artifact = target_dir.join(triple).join("release").join(target.artifact);
        let destination = output_dir.join(format!("{stem}-{}", target.suffix));
        match std::fs::copy(&artifact, &destination) {
            Ok(_) => {
                built += 1;
                log(Kind::Ok, format!("Created: {}", destination.display()));
            }
            Err(error) => log(
                Kind::Error,
                format!("Could not copy the {} output: {error}", target.name),
            ),
        }
    }

    let _ = sender.send(Msg::Progress(built as f32 / total));
    if built > 0 {
        log(
            Kind::Ok,
            format!(
                "Finished: built {built} of {} libraries in {}",
                selected.len(),
                output_dir.display()
            ),
        );
    } else {
        log(Kind::Error, "No libraries were created.".into());
    }
    finish();
}

fn main() {
    App::new(BuilderApp::default())
        .window_size(860, 840)
        .title("JAR to Dynamic Library")
        .update(BuilderApp::poll_build)
        .run();
}
