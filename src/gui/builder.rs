use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use crate::gui::state::{Level, Msg};
use crate::gui::util::{
    command, host_os, path_with, project_root, run_streamed, tool_works, triple_os, works_with_path,
};

const INJECTOR_CLASS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ForgeInjector.class"));

pub struct BuildTarget {
    pub name: &'static str,
    pub detail: &'static str,
    pub icon: &'static [u8],
    pub triple: &'static str,
    pub artifact: &'static str,
    pub suffix: &'static str,
}

pub const TARGETS: [BuildTarget; 4] = [
    BuildTarget {
        name: "Linux",
        detail: ".so  x86_64",
        icon: include_bytes!("../../assets/images/linux.png"),
        triple: "x86_64-unknown-linux-gnu",
        artifact: "libjar_to_dynamic_library.so",
        suffix: "linux-x86_64.so",
    },
    BuildTarget {
        name: "Windows",
        detail: ".dll  x86_64",
        icon: include_bytes!("../../assets/images/windows.png"),
        triple: "x86_64-pc-windows-msvc",
        artifact: "jar_to_dynamic_library.dll",
        suffix: "windows-x86_64.dll",
    },
    BuildTarget {
        name: "macOS Intel",
        detail: ".dylib  x86_64",
        icon: include_bytes!("../../assets/images/macos.png"),
        triple: "x86_64-apple-darwin",
        artifact: "libjar_to_dynamic_library.dylib",
        suffix: "macos-x86_64.dylib",
    },
    BuildTarget {
        name: "macOS Apple Silicon",
        detail: ".dylib  arm64",
        icon: include_bytes!("../../assets/images/macos.png"),
        triple: "aarch64-apple-darwin",
        artifact: "libjar_to_dynamic_library.dylib",
        suffix: "macos-aarch64.dylib",
    },
];

pub fn host_target_selection() -> [bool; TARGETS.len()] {
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

fn zig_works(path: &std::ffi::OsStr) -> bool {
    works_with_path("zig", &["version"], path)
        || works_with_path("python3", &["-m", "ziglang", "version"], path)
        || works_with_path("python", &["-m", "ziglang", "version"], path)
}

fn ensure_zig(sender: &Sender<Msg>, base: f32, total: f32) -> Option<std::ffi::OsString> {
    let log = |kind: Level, text: &str| {
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
        Level::Info,
        "Installing Zig for cross-compilation (first run only)...",
    );
    let python = ["python3", "python", "py"]
        .into_iter()
        .find(|name| tool_works(name, "--version"));
    let Some(python) = python else {
        log(
            Level::Error,
            "Python is required to install Zig. Install Python 3 or install Zig with your package manager.",
        );
        return None;
    };

    let mut create = command(python);
    create.args(["-m", "venv"]).arg(&venv);
    if !run_streamed(create, sender, base, total, 50.0) {
        log(
            Level::Error,
            "Could not create the Python environment. On Debian/Ubuntu, install python3-venv.",
        );
        return None;
    }
    let venv_python = bin.join(if cfg!(windows) { "python.exe" } else { "python" });
    let mut install = command(&venv_python.to_string_lossy());
    install.args(["-m", "pip", "install", "--upgrade", "ziglang"]);
    if !run_streamed(install, sender, base, total, 50.0) {
        log(
            Level::Error,
            "Could not install Zig with pip. Install it with your package manager and try again.",
        );
        return None;
    }
    if zig_works(&with_venv) {
        log(Level::Ok, "Zig installed.");
        Some(with_venv)
    } else {
        log(Level::Error, "Zig was installed but did not run successfully.");
        None
    }
}

fn ensure_zigbuild(root: &Path, path: &std::ffi::OsStr, sender: &Sender<Msg>, base: f32, total: f32) -> bool {
    if works_with_path("cargo", &["zigbuild", "--help"], path) {
        return true;
    }
    let _ = sender.send(Msg::Log(
        Level::Info,
        "Installing cargo-zigbuild (this may take a few minutes the first time...)".into(),
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
            Level::Error,
            "Cross-compiling for Windows requires LLVM/Clang. Install LLVM, add clang to PATH, and try again."
                .into(),
        ));
        return false;
    }
    if works_with_path("cargo", &["xwin", "--help"], path) {
        return true;
    }
    let _ = sender.send(Msg::Log(
        Level::Info,
        "Installing cargo-xwin for Windows cross-compilation...".into(),
    ));
    let mut install = command("cargo");
    install
        .args(["install", "--locked", "cargo-xwin", "--color", "never"])
        .current_dir(root)
        .env("PATH", path);
    run_streamed(install, sender, base, total, 150.0) && works_with_path("cargo", &["xwin", "--help"], path)
}

pub fn build_libraries(jar: PathBuf, selected: Vec<usize>, sender: Sender<Msg>) {
    let log = |kind: Level, text: String| {
        let _ = sender.send(Msg::Log(kind, text));
    };
    let finish = || {
        let _ = sender.send(Msg::Done);
    };

    if selected.is_empty() {
        log(
            Level::Error,
            "Select at least one target platform before building.".into(),
        );
        return finish();
    }

    match crate::audit::audit_jar(&jar) {
        Ok(report) => {
            log(
                Level::Info,
                format!(
                    "Audited {} ({} classes, {} resources).",
                    jar.file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_else(|| jar.display().to_string()),
                    report.class_count,
                    report.resource_count
                ),
            );
            for warning in report.injector_warnings() {
                log(Level::Warn, warning);
            }
        }
        Err(error) => log(
            Level::Warn,
            format!("Could not complete the JAR compatibility audit: {error}"),
        ),
    }

    let Some(root) = project_root() else {
        log(
            Level::Error,
            "Project files were not found. Extract the complete desktop package or run the GUI from a project checkout."
                .into(),
        );
        return finish();
    };
    if !tool_works("cargo", "--version") {
        log(
            Level::Error,
            "Cargo was not found. Install Rust from https://rustup.rs.".into(),
        );
        return finish();
    }
    let workspace = std::env::temp_dir().join("jar-to-dynamic-library-gui");
    if let Err(error) = std::fs::create_dir_all(&workspace) {
        log(
            Level::Error,
            format!("Could not create the temporary build folder: {error}"),
        );
        return finish();
    }
    let injector_class = workspace.join("ForgeInjector.class");
    if let Err(error) = std::fs::write(&injector_class, INJECTOR_CLASS) {
        log(
            Level::Error,
            format!("Could not prepare the Java injector: {error}"),
        );
        return finish();
    }
    log(
        Level::Ok,
        "Cargo found. The bundled Java injector is ready.".to_string(),
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
            Level::Error,
            format!("Could not create the output folder: {error}"),
        );
        return finish();
    }

    let agent_jar = output_dir.join(format!("{stem}-agent.jar"));
    match std::fs::write(&agent_jar, crate::classes::AGENT_JAR_DATA) {
        Ok(_) => log(
            Level::Ok,
            format!(
                "Created: {} (Java instrumentation agent; works on every platform).",
                agent_jar.display()
            ),
        ),
        Err(error) => log(
            Level::Error,
            format!("Could not write the Java agent JAR: {error}"),
        ),
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
            Level::Info,
            format!("[{}/{}] {}", position + 1, selected.len(), target.name),
        );

        let cross = triple_os(triple) != host_os();
        let mut path = std::env::var_os("PATH").unwrap_or_default();
        let cross_windows = cross && triple_os(triple) == "windows";
        if cross {
            if cross_windows {
                log(
                    Level::Detail,
                    format!("Preparing a Windows cross-build from {}...", host_os()),
                );
                if !ensure_xwin(&root, &path, &sender, base, total) {
                    log(
                        Level::Error,
                        format!("Could not prepare the Windows build for {}.", target.name),
                    );
                    continue;
                }
            } else {
                log(
                    Level::Detail,
                    format!(
                        "{} is not your host system ({}); preparing a cross-build with Zig.",
                        target.name,
                        host_os()
                    ),
                );
                let Some(zig_path) = ensure_zig(&sender, base, total) else {
                    log(
                        Level::Error,
                        format!("Zig is required to cross-compile for {}.", target.name),
                    );
                    continue;
                };
                if !ensure_zigbuild(&root, &zig_path, &sender, base, total) {
                    log(
                        Level::Error,
                        format!("Could not install cargo-zigbuild; skipping {}.", target.name),
                    );
                    continue;
                }
                path = zig_path;
            }
        }

        if has_rustup {
            log(Level::Detail, format!("Checking the {triple} target..."));
            let mut add_target = command("rustup");
            add_target.args(["target", "add", triple]);
            if !run_streamed(add_target, &sender, base, total, 3.0) {
                log(
                    Level::Warn,
                    format!("Could not install the Rust target {triple}."),
                );
            }
        } else if cross {
            log(
                Level::Warn,
                "rustup was not found, so the cross-compilation target cannot be installed. Install Rust from https://rustup.rs."
                    .into(),
            );
        }
        log(
            Level::Detail,
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
                Level::Error,
                format!("The {} build failed. See the error messages above.", target.name),
            );
            continue;
        }

        let artifact = target_dir.join(triple).join("release").join(target.artifact);
        let destination = output_dir.join(format!("{stem}-{}", target.suffix));
        match std::fs::copy(&artifact, &destination) {
            Ok(_) => {
                built += 1;
                log(Level::Ok, format!("Created: {}", destination.display()));
            }
            Err(error) => log(
                Level::Error,
                format!("Could not copy the {} output: {error}", target.name),
            ),
        }
    }

    let _ = sender.send(Msg::Progress(built as f32 / total));
    if built > 0 {
        log(
            Level::Ok,
            format!(
                "Finished: built {built} of {} libraries in {}",
                selected.len(),
                output_dir.display()
            ),
        );
    } else {
        log(Level::Error, "No libraries were created.".into());
    }
    finish();
}
