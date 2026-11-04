use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::audit::{audit_jar, JarAudit};
use crate::gui::builder::{self, TARGETS};
use crate::gui::process::{self, JvmProcess};
use crate::packaging;

pub const APP_TITLE: &str = "JAR to Dynamic Library";
pub const GITHUB_URL: &str = "https://github.com/26140070-cpu/jar-to-dynamic-library";
pub const DISCORD_URL: &str = "https://discord.gg/G6CwQHBJF6";

const MAX_LOG_LINES: usize = 800;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Build,
    Injector,
    Changelog,
}

impl View {
    pub const ALL: [View; 3] = [View::Build, View::Injector, View::Changelog];

    pub fn label(self) -> &'static str {
        match self {
            View::Build => "Build",
            View::Injector => "Injector",
            View::Changelog => "Changelog",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Info,
    Ok,
    Warn,
    Error,
    Detail,
}

impl Level {
    pub fn tag(self) -> &'static str {
        match self {
            Level::Info => "INFO",
            Level::Ok => "OK",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
            Level::Detail => "DETAIL",
        }
    }
}

#[derive(Clone, Debug)]
pub struct LogLine {
    pub level: Level,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct StartupInstance {
    pub name: String,
    pub path: PathBuf,
    pub launcher: &'static str,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Injection {
    Idle,
    Scanning,
    Injecting(u32),
    Reporting(u32),
    Done(u32),
    Failed,
}

impl Injection {
    pub fn is_busy(self) -> bool {
        matches!(
            self,
            Injection::Scanning | Injection::Injecting(_) | Injection::Reporting(_)
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    JavaAgent,

    NativeLibrary,
}

impl Route {
    pub const ALL: [Route; 2] = [Route::JavaAgent, Route::NativeLibrary];

    pub fn label(self) -> &'static str {
        match self {
            Route::JavaAgent => "Java agent JAR",
            Route::NativeLibrary => "Native library",
        }
    }
}

pub enum Msg {
    Log(Level, String),
    Progress(f32),
    Scanned(Vec<JvmProcess>),
    Injected(Result<u32, String>),
    ReportFinished(u32),
    Done,
}

pub struct AppCore {
    pub view: View,
    pub jar: Option<PathBuf>,

    pub audit: Option<JarAudit>,
    pub selected_targets: Vec<usize>,
    pub building: bool,
    pub progress: f32,
    pub log: VecDeque<LogLine>,
    pub processes: Vec<JvmProcess>,
    pub selected_pid: Option<u32>,
    pub pending_confirm: Option<u32>,
    pub injection: Injection,
    pub route: Route,
    receiver: Receiver<Msg>,
    sender: Sender<Msg>,
}

impl Default for AppCore {
    fn default() -> Self {
        let (sender, receiver) = channel();
        let host = builder::host_target_selection();
        let mut core = Self {
            view: View::Build,
            jar: None,
            audit: None,
            selected_targets: (0..TARGETS.len()).filter(|index| host[*index]).collect(),
            building: false,
            progress: 0.0,
            log: VecDeque::new(),
            processes: Vec::new(),
            selected_pid: None,
            pending_confirm: None,
            injection: Injection::Idle,
            route: Route::JavaAgent,
            receiver,
            sender,
        };
        core.log.push_back(LogLine {
            level: Level::Detail,
            text: "Select a .jar, build its libraries, then open the Injector tab.".into(),
        });
        core
    }
}

impl AppCore {
    pub fn log(&self, level: Level, text: impl Into<String>) {
        let _ = self.sender.send(Msg::Log(level, text.into()));
    }

    pub fn set_view(&mut self, view: View) {
        if self.view != view {
            self.view = view;
            if view == View::Injector && self.processes.is_empty() && !self.injection.is_busy() {
                self.scan();
            }
        }
    }

    pub fn target_selected(&self, index: usize) -> bool {
        self.selected_targets.contains(&index)
    }

    pub fn toggle_target(&mut self, index: usize) {
        if let Some(position) = self.selected_targets.iter().position(|value| *value == index) {
            self.selected_targets.remove(position);
        } else {
            self.selected_targets.push(index);
            self.selected_targets.sort_unstable();
        }
    }

    pub fn jar_label(&self) -> String {
        self.jar
            .as_deref()
            .map(display_file_name)
            .unwrap_or_else(|| "No JAR selected".to_string())
    }

    pub fn accept_jar(&mut self, path: PathBuf) {
        if !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("jar"))
        {
            self.log(Level::Error, "The selected file does not have a .jar extension.");
            return;
        }

        let path = match path.canonicalize() {
            Ok(absolute) => absolute,
            Err(error) => {
                self.log(
                    Level::Warn,
                    format!("Could not resolve an absolute path for the JAR ({error}); injection may fail."),
                );
                path
            }
        };
        self.jar = Some(path.clone());
        self.log(Level::Ok, format!("Selected JAR: {}", path.display()));
        match audit_jar(&path) {
            Ok(report) => {
                for warning in report.injector_warnings() {
                    self.log(Level::Warn, warning);
                }
                self.audit = Some(report);
            }
            Err(error) => {
                self.audit = None;
                self.log(Level::Warn, format!("Could not audit this JAR: {error}"));
            }
        }
    }

    pub fn pick_jar(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Select a Minecraft mod JAR")
            .add_filter("Java archive", &["jar", "zip"])
            .pick_file()
        else {
            return;
        };
        self.accept_jar(path);
    }

    pub fn configure_startup_injection(&mut self, instance_dir: &Path) {
        let Some(jar) = self.jar.as_deref() else {
            self.log(
                Level::Warn,
                "Select a mod JAR before configuring startup injection.",
            );
            return;
        };

        let Some(audit) = self.audit.as_ref() else {
            self.log(
                Level::Error,
                "Audit the selected mod JAR before configuring startup injection.",
            );
            return;
        };
        match configure_startup_injection(jar, instance_dir, audit) {
            Ok((loader, Some(config), backup, _)) => {
                let backup_note = backup.map_or_else(
                    || "The startup injection was already configured.".to_string(),
                    |path| format!("Original launcher settings were backed up to {}.", path.display()),
                );
                self.log(
                    Level::Ok,
                    format!(
                        "Configured {loader} startup injection in {}. Prism/MultiMC will load the mod \
                         during the next game launch. {backup_note}",
                        config.display()
                    ),
                );
            }
            Ok((loader, None, _, args)) => self.log(
                Level::Warn,
                format!(
                    "Prepared {loader} startup injection, but this folder is not a Prism/MultiMC \
                     instance. Add these JVM arguments in your launcher settings: {}",
                    args.iter()
                        .map(|arg| quote_prism_argument(arg))
                        .collect::<Vec<_>>()
                        .join(" ")
                ),
            ),
            Err(error) => self.log(
                Level::Error,
                format!("Could not configure startup injection: {error}"),
            ),
        }
    }

    pub fn start_build(&mut self) {
        if self.building {
            return;
        }
        let Some(jar) = self.jar.clone() else {
            self.log(Level::Warn, "Select a .jar file first.");
            return;
        };
        if self.selected_targets.is_empty() {
            self.log(Level::Warn, "Select at least one target platform.");
            return;
        }
        let selected = self.selected_targets.clone();
        self.building = true;
        self.progress = 0.0;
        self.log(Level::Info, "Preparing the JVM injector build...");
        let sender = self.sender.clone();
        thread::spawn(move || builder::build_libraries(jar, selected, sender));
    }

    pub fn scan(&mut self) {
        if self.injection.is_busy() || self.building {
            return;
        }
        self.injection = Injection::Scanning;
        self.log(Level::Info, "Scanning for running Java/Minecraft processes...");
        let sender = self.sender.clone();
        thread::spawn(move || match process::list_jvm_processes() {
            Ok(processes) => {
                let minecraft_like = processes.iter().filter(|item| item.likely_minecraft).count();
                let _ = sender.send(Msg::Log(
                    Level::Ok,
                    format!(
                        "Found {} Java process(es), {minecraft_like} look like Minecraft.",
                        processes.len()
                    ),
                ));
                if !processes.is_empty() && minecraft_like == 0 {
                    let _ = sender.send(Msg::Log(
                        Level::Warn,
                        "No process matched a known Minecraft launch signature. All Java processes are still listed; select the game JVM manually if your launcher uses custom arguments.".to_string(),
                    ));
                }
                let _ = sender.send(Msg::Scanned(processes));
            }
            Err(error) => {
                let _ = sender.send(Msg::Log(
                    Level::Error,
                    format!("Could not scan processes: {error}"),
                ));

                let _ = sender.send(Msg::Scanned(Vec::new()));
            }
        });
    }

    pub fn select_process(&mut self, pid: u32) {
        if self.injection.is_busy() {
            return;
        }
        self.selected_pid = Some(pid);
        self.injection = Injection::Idle;
    }

    pub fn selected_process(&self) -> Option<&JvmProcess> {
        let pid = self.selected_pid?;
        self.processes.iter().find(|item| item.pid == pid)
    }

    pub fn request_injection(&mut self) {
        if self.injection.is_busy() || self.building {
            return;
        }
        let Some(pid) = self.selected_pid else {
            self.log(Level::Warn, "Select a running Minecraft process first.");
            return;
        };
        if self.jar.is_none() {
            self.log(
                Level::Warn,
                "Select the mod's JAR first so its built library can be found.",
            );
            return;
        }
        let Some(artifact) = self.injector_artifact() else {
            self.log(
                Level::Warn,
                match self.route {
                    Route::NativeLibrary => {
                        "No native library was built for this host OS yet. Click 'Build libraries' first."
                            .to_string()
                    }
                    Route::JavaAgent => {
                        "No agent JAR was built for this mod yet. Click 'Build libraries' first.".to_string()
                    }
                },
            );
            return;
        };
        if self.route == Route::NativeLibrary {
            if let Some(reason) = process::architecture_mismatch(&artifact, pid) {
                self.log(Level::Error, reason);
                return;
            }
        }
        self.pending_confirm = Some(pid);
    }

    pub fn cancel_injection(&mut self) {
        self.pending_confirm = None;
    }

    pub fn confirm_injection(&mut self) {
        let Some(pid) = self.pending_confirm.take() else {
            return;
        };
        let (Some(jar), Some(artifact)) = (self.jar.clone(), self.injector_artifact()) else {
            return;
        };
        self.injection = Injection::Injecting(pid);
        let route = self.route;
        let display_name = self
            .selected_process()
            .map(|item| item.short_description())
            .unwrap_or_default();
        self.log(
            Level::Info,
            format!(
                "Attaching {} to PID {pid} ({display_name}) via the JDK Attach API...",
                display_file_name(&artifact)
            ),
        );

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let session_id = format!("{}-{pid}-{nonce}", std::process::id());
        let options = format!("jar={};session={session_id}", jar.display());
        let sender = self.sender.clone();
        thread::spawn(move || {
            let result = match route {
                Route::JavaAgent => process::attach_agent_jar(pid, &artifact, Some(&options)).map(|()| pid),
                Route::NativeLibrary => {
                    process::attach_native_library(pid, &artifact, Some(&options)).map(|()| pid)
                }
            };
            let report_pid = match &result {
                Ok(pid) => {
                    let _ = sender.send(Msg::Log(
                        Level::Ok,
                        format!("Attached to PID {pid} via the JDK Attach API."),
                    ));
                    let _ = sender.send(Msg::Log(
                        Level::Detail,
                        "Waiting for this attach's injector report (~/jar-to-dynamic-library-log.txt)..."
                            .to_string(),
                    ));
                    Some(*pid)
                }
                Err(error) => {
                    let _ = sender.send(Msg::Log(Level::Error, format!("Attach failed: {error}")));
                    None
                }
            };

            let _ = sender.send(Msg::Injected(result));
            if let Some(pid) = report_pid {
                let reporter = sender.clone();
                thread::spawn(move || super::report::follow(reporter, session_id, pid));
            }
        });
    }

    pub fn run_diagnostics(&mut self) {
        self.log(Level::Info, "Collecting the injector diagnostics...");
        self.log(
            Level::Detail,
            format!(
                "Host: {} {}, selected route: {}.",
                std::env::consts::OS,
                std::env::consts::ARCH,
                self.route.label()
            ),
        );

        match process::java_executable() {
            Some(java) => {
                let major = process::java_major_version(&java);
                let mechanism = match major {
                    Some(version) if version >= 9 => {
                        "It runs the helper with '--add-modules jdk.attach'.".to_string()
                    }
                    Some(_) => match process::tools_jar_classpath(&java, major) {
                        Some(tools) => format!("Its Attach API comes from {}.", tools.display()),
                        None => "This JVM has no lib/tools.jar next to it, so a full JDK 8 is required \
                                to attach."
                            .to_string(),
                    },
                    None => {
                        "Its version could not be read, so the helper runs without module flags.".to_string()
                    }
                };
                self.log(
                    Level::Detail,
                    format!(
                        "Attach toolchain: {} (version {}). {mechanism}",
                        java.display(),
                        major
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "unknown".to_string())
                    ),
                );
            }
            None => self.log(
                Level::Error,
                "No 'java' executable was found, so nothing can be attached. Set JAVA_HOME or put java \
                 on PATH."
                    .to_string(),
            ),
        }

        if let Some(jar) = self.jar.clone() {
            for route in Route::ALL {
                let artifact = match route {
                    Route::JavaAgent => process::agent_jar_path(&jar),
                    Route::NativeLibrary => process::native_library_path(&jar),
                };
                let built = artifact.is_file();
                self.log(
                    if built { Level::Ok } else { Level::Warn },
                    format!(
                        "{} route: {}.",
                        route.label(),
                        if built {
                            format!("{} (built)", artifact.display())
                        } else {
                            format!("{} (not built yet — press 'Build libraries')", artifact.display())
                        }
                    ),
                );
                if route == Route::NativeLibrary && built {
                    if let Some(arch) = process::library_architecture(&artifact) {
                        self.log(
                            Level::Detail,
                            format!("This native library was built for {arch}.",),
                        );
                    }
                }
            }
            if let Some(audit) = &self.audit {
                self.log(
                    Level::Detail,
                    format!(
                        "{} contains {} class(es) and {} resource(s); every one of them is served by the \
                         injected archive, and the game's class loader name is reported by the \
                         '[game:loader]' line after the attach.",
                        display_file_name(&jar),
                        audit.class_count,
                        audit.resource_count
                    ),
                );
                if audit.has_fabric_metadata {
                    self.log(
                        Level::Detail,
                        format!(
                            "Fabric metadata: id={}, version={}.",
                            audit.fabric_mod_id.as_deref().unwrap_or("(missing)"),
                            audit.fabric_mod_version.as_deref().unwrap_or("(missing)")
                        ),
                    );
                }
                if !audit.fabric_entrypoints.is_empty() {
                    let list = audit
                        .fabric_entrypoints
                        .iter()
                        .map(|(group, class)| format!("{group} -> {class}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    self.log(
                        Level::Detail,
                        format!(
                            "Entrypoints the injector invokes, filtered by the game's environment: {list}."
                        ),
                    );
                }
                if !audit.mixin_configs.is_empty() {
                    self.log(
                        Level::Detail,
                        format!(
                            "Mixin configurations registered through Mixins.addConfiguration: {}.",
                            audit.mixin_configs.join(", ")
                        ),
                    );
                }
                if audit.declared_nested_jar_count > 0 || audit.has_nested_jars {
                    self.log(
                        Level::Detail,
                        "Bundled JARs are extracted and added to the class path before the entrypoints \
                         run."
                            .to_string(),
                    );
                }
                for (flag, feature) in [
                    (audit.has_mixin_plugin, "a custom Mixin config plugin"),
                    (audit.has_access_widener, "an access widener/class tweaker"),
                    (audit.has_access_transformer, "a Forge access transformer"),
                    (audit.has_legacy_coremod, "a legacy FML coremod/tweaker"),
                    (audit.has_manifest_class_path, "manifest Class-Path entries"),
                ] {
                    if flag {
                        self.log(
                            Level::Warn,
                            format!(
                                "This JAR declares {feature}, which an agent attached after the game \
                                 booted cannot honour; use the early-startup adapter kit for those."
                            ),
                        );
                    }
                }
            } else {
                self.log(
                    Level::Warn,
                    "This JAR could not be audited, so its entrypoints and mixins are unknown.".to_string(),
                );
            }
        } else {
            self.log(Level::Warn, "No JAR selected yet.".to_string());
        }

        match self.selected_pid {
            Some(pid) => {
                let name = self
                    .selected_process()
                    .map(|item| item.short_description())
                    .unwrap_or_default();
                let arch = process::process_architecture(pid).unwrap_or("an unknown architecture");
                self.log(
                    Level::Detail,
                    format!("Selected process: PID {pid} ({name}) running as {arch}."),
                );
                if self
                    .processes
                    .iter()
                    .any(|item| item.pid == pid && !item.likely_minecraft)
                {
                    self.log(
                        Level::Warn,
                        "This process does not look like Minecraft; the injector will still attach, but \
                         no game loader may be found."
                            .to_string(),
                    );
                }
            }
            None => self.log(
                Level::Warn,
                "No process selected. Start the game and press Scan.".to_string(),
            ),
        }

        self.log(
            Level::Detail,
            match super::report::log_path() {
                Some(path) => format!(
                    "The game writes its structured report to {}; the GUI reads it back after the attach.",
                    path.display()
                ),
                None => "HOME is unset, so the injector's report file cannot be located and the GUI \
                        cannot read the result back."
                    .to_string(),
            },
        );
        self.log(
            Level::Detail,
            "Only the Java agent route receives an Instrumentation object, which is what makes the \
             dynamic class-path entry and class re-transformation possible; the native library route \
             has neither."
                .to_string(),
        );
    }

    pub fn injector_artifact(&self) -> Option<PathBuf> {
        let jar = self.jar.as_ref()?;
        match self.route {
            Route::NativeLibrary => process::host_injector_library(jar),
            Route::JavaAgent => process::host_agent_jar(jar),
        }
    }

    pub fn poll(&mut self) {
        let mut messages = Vec::new();
        while let Ok(message) = self.receiver.try_recv() {
            messages.push(message);
        }
        for message in messages {
            match message {
                Msg::Log(level, text) => {
                    self.log.push_back(LogLine { level, text });
                    if self.log.len() > MAX_LOG_LINES {
                        self.log.pop_front();
                    }
                }
                Msg::Progress(value) => self.progress = value.clamp(0.0, 1.0),
                Msg::Scanned(processes) => {
                    let preferred = processes
                        .iter()
                        .find(|item| item.likely_minecraft)
                        .map(|item| item.pid);
                    self.processes = processes;
                    if !self
                        .selected_pid
                        .is_some_and(|pid| self.processes.iter().any(|item| item.pid == pid))
                    {
                        self.selected_pid = preferred;
                    }
                    self.injection = Injection::Idle;
                }
                Msg::Injected(Ok(pid)) => self.injection = Injection::Reporting(pid),
                Msg::Injected(Err(_)) => self.injection = Injection::Failed,
                Msg::ReportFinished(pid) => {
                    if self.injection == Injection::Reporting(pid) {
                        self.injection = Injection::Done(pid);
                    }
                }
                Msg::Done => self.building = false,
            }
        }
    }
}

fn display_file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

fn configure_startup_injection(
    jar: &Path,
    instance_dir: &Path,
    audit: &JarAudit,
) -> Result<(&'static str, Option<PathBuf>, Option<PathBuf>, Vec<String>), String> {
    if !jar.is_file() {
        return Err(format!("the selected JAR is unavailable: {}", jar.display()));
    }
    let config = instance_dir.join("instance.cfg");
    let instance_root = instance_dir
        .canonicalize()
        .map_err(|error| format!("cannot resolve instance folder: {error}"))?;
    let jar = jar
        .canonicalize()
        .map_err(|error| format!("cannot resolve selected mod JAR: {error}"))?;
    let loader = match (
        audit.has_fabric_metadata,
        audit.has_forge_metadata,
        audit.has_neoforge_metadata,
    ) {
        (true, false, false) => StartupLoader::Fabric,
        (false, true, false) => StartupLoader::Forge,
        (false, false, true) => StartupLoader::NeoForge,
        (true, true, _) | (true, _, true) | (_, true, true) => return Err(
            "the JAR declares metadata for more than one supported loader; select a loader-specific mod JAR."
                .into(),
        ),
        (false, false, false) => return Err("the JAR has no Fabric, Forge, or NeoForge mod metadata.".into()),
    };

    let game_dir = if instance_root.join("minecraft").is_dir() {
        instance_root.join("minecraft")
    } else {
        instance_root.clone()
    };
    let mut args = Vec::new();
    match loader {
        StartupLoader::Fabric => {
            let mod_path = jar.to_string_lossy();
            let path_list_separator = if cfg!(windows) { ';' } else { ':' };
            if mod_path.contains(path_list_separator) {
                return Err(format!(
                    "the mod path contains '{path_list_separator}', Fabric's fabric.addMods path-list separator."
                ));
            }
            args.push(format!("-Dfabric.addMods={mod_path}"));
        }
        StartupLoader::Forge | StartupLoader::NeoForge => {
            let mods_dir = game_dir.join("mods");
            fs::create_dir_all(&mods_dir)
                .map_err(|error| format!("cannot create {}: {error}", mods_dir.display()))?;
            let support_dir = instance_root.join("jtdl-startup");
            fs::create_dir_all(&support_dir)
                .map_err(|error| format!("cannot create {}: {error}", support_dir.display()))?;
            let kit = match loader {
                StartupLoader::Forge => packaging::package_forge_adapter_kit(&jar)?,
                StartupLoader::NeoForge => packaging::package_neoforge_adapter_kit(&jar)?,
                StartupLoader::Fabric => unreachable!(),
            };
            let (agent, bundle) = extract_adapter_files(&kit, &support_dir, loader)?;
            args.push(format!("-Djtdl.modsDir={}", mods_dir.display()));
            args.push(format!("-javaagent:{}={}", agent.display(), bundle.display()));
        }
    }

    if !config.is_file() {
        return Ok((loader.label(), None, None, args));
    }
    let original =
        fs::read_to_string(&config).map_err(|error| format!("cannot read {}: {error}", config.display()))?;
    let updated = update_prism_jvm_args(&original, &args)?;
    if original == updated {
        return Ok((loader.label(), Some(config), None, args));
    }

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock error: {error}"))?
        .as_nanos();
    let backup = config.with_file_name(format!("instance.cfg.jtdl-{stamp}.bak"));
    fs::copy(&config, &backup).map_err(|error| format!("cannot back up {}: {error}", config.display()))?;

    let temp = config.with_file_name(format!("instance.cfg.jtdl-{stamp}.tmp"));
    let mut temp_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|error| format!("cannot create temporary launcher config: {error}"))?;
    temp_file
        .write_all(updated.as_bytes())
        .and_then(|()| temp_file.sync_all())
        .map_err(|error| {
            let _ = fs::remove_file(&temp);
            format!("cannot write temporary launcher config: {error}")
        })?;

    #[cfg(windows)]
    {
        fs::remove_file(&config).map_err(|error| {
            let _ = fs::remove_file(&temp);
            format!("cannot replace {}: {error}", config.display())
        })?;
        if let Err(error) = fs::rename(&temp, &config) {
            let _ = fs::copy(&backup, &config);
            let _ = fs::remove_file(&temp);
            return Err(format!("cannot install updated launcher config: {error}"));
        }
    }
    #[cfg(not(windows))]
    fs::rename(&temp, &config).map_err(|error| {
        let _ = fs::remove_file(&temp);
        format!("cannot install updated launcher config: {error}")
    })?;

    Ok((loader.label(), Some(config), Some(backup), args))
}

#[derive(Clone, Copy)]
enum StartupLoader {
    Fabric,
    Forge,
    NeoForge,
}

impl StartupLoader {
    fn label(self) -> &'static str {
        match self {
            Self::Fabric => "Fabric",
            Self::Forge => "Forge",
            Self::NeoForge => "NeoForge",
        }
    }
}

pub fn discover_startup_instances() -> Vec<StartupInstance> {
    let mut instances = Vec::new();
    let mut roots = Vec::new();
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    if let Some(home) = home {
        roots.push((home.join(".minecraft"), "Minecraft launcher"));
        roots.push((home.join(".tlauncher/legacy/Minecraft/game"), "TLauncher"));
        roots.push((home.join(".tlauncher/instances"), "TLauncher"));
        roots.push((home.join(".sklauncher"), "SKlauncher"));
        roots.push((home.join(".pine"), "Pine launcher"));
        roots.push((
            home.join(".local/share/PrismLauncher/instances"),
            "Prism Launcher",
        ));
        roots.push((home.join(".local/share/multimc/instances"), "MultiMC"));
        roots.push((home.join(".local/share/PolyMC/instances"), "PolyMC"));
        roots.push((home.join(".local/share/PrismLauncher"), "Prism Launcher"));
        roots.push((home.join(".local/share/SKlauncher"), "SKlauncher"));
        roots.push((home.join(".local/share/TLauncher"), "TLauncher"));
        roots.push((home.join(".config/PrismLauncher/instances"), "Prism Launcher"));
        roots.push((home.join(".config/SKlauncher"), "SKlauncher"));
        roots.push((
            home.join("Library/Application Support/minecraft"),
            "Minecraft launcher",
        ));
        roots.push((
            home.join("Library/Application Support/PrismLauncher/instances"),
            "Prism Launcher",
        ));
        roots.push((home.join("Library/Application Support/TLauncher"), "TLauncher"));
    }

    #[cfg(windows)]
    if let Some(app_data) = std::env::var_os("APPDATA").map(PathBuf::from) {
        roots.push((app_data.join(".minecraft"), "Minecraft launcher"));
        roots.push((app_data.join("PrismLauncher/instances"), "Prism Launcher"));
        roots.push((app_data.join("MultiMC/instances"), "MultiMC"));
        roots.push((app_data.join("TLauncher"), "TLauncher"));
        roots.push((app_data.join("SKlauncher"), "SKlauncher"));
    }

    for (root, launcher) in roots {
        add_startup_instance(&mut instances, &root, launcher);
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                let path = entry.path();
                add_startup_instance(&mut instances, &path, launcher);
                if path.join("instance.cfg").is_file() {
                    continue;
                }
                add_startup_instance(&mut instances, &path.join("minecraft"), launcher);
                add_startup_instance(&mut instances, &path.join(".minecraft"), launcher);
            }
        }
    }

    instances.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.path.cmp(&right.path))
    });
    instances
}

fn add_startup_instance(instances: &mut Vec<StartupInstance>, candidate: &Path, launcher: &'static str) {
    let Ok(path) = candidate.canonicalize() else {
        return;
    };
    if !path.is_dir()
        || instances.iter().any(|instance| instance.path == path)
        || !(path.join("instance.cfg").is_file() || is_game_directory(&path))
    {
        return;
    }
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| launcher.to_string());
    instances.push(StartupInstance {
        name,
        path,
        launcher: if candidate.join("instance.cfg").is_file() {
            launcher
        } else {
            "Minecraft game folder"
        },
    });
}

fn is_game_directory(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    name == ".minecraft"
        || name == "minecraft"
        || (path.join("versions").is_dir()
            && (path.join("mods").is_dir()
                || path.join("assets").is_dir()
                || path.join("libraries").is_dir()))
}

fn update_prism_jvm_args(config: &str, additions: &[String]) -> Result<String, String> {
    let mut lines: Vec<String> = config.lines().map(str::to_string).collect();
    let mut override_line = None;
    let mut args_line = None;
    for (index, line) in lines.iter().enumerate() {
        if line.starts_with("OverrideJavaArgs=") {
            if override_line.replace(index).is_some() {
                return Err("instance.cfg contains duplicate OverrideJavaArgs settings.".into());
            }
        } else if line.starts_with("JvmArgs=") && args_line.replace(index).is_some() {
            return Err("instance.cfg contains duplicate JvmArgs settings.".into());
        }
    }

    let raw_args = args_line
        .map(|index| lines[index]["JvmArgs=".len()..].to_string())
        .unwrap_or_default();
    let wraps_value = is_quoted_config_value(&raw_args);
    let mut args = if wraps_value {
        unescape_config_value(&raw_args[1..raw_args.len() - 1])
    } else {
        raw_args
    };
    for property in ["-Dfabric.addMods=", "-Djtdl.modsDir="] {
        let mut offset = 0;
        while let Some(relative) = args[offset..].find(property) {
            let position = offset + relative;
            if position > 0
                && !args[..position]
                    .chars()
                    .next_back()
                    .is_some_and(|character| character.is_whitespace() || character == '"')
            {
                args.insert(position, ' ');
                offset = position + 1 + property.len();
            } else {
                offset = position + property.len();
            }
        }
    }
    for addition in additions {
        for property in ["-Dfabric.addMods=", "-Djtdl.modsDir="] {
            if addition.starts_with(property) && args.contains(property) && !args.contains(addition) {
                return Err(format!(
                    "instance.cfg already defines {property}; merge the mod path or remove the existing setting before enabling JAR to Dynamic Library."
                ));
            }
        }
        if !args.contains(addition) {
            if !args.is_empty() && !args.chars().last().is_some_and(char::is_whitespace) {
                args.push(' ');
            }
            args.push_str(&quote_prism_argument(addition));
        }
    }

    if let Some(index) = override_line {
        lines[index] = "OverrideJavaArgs=true".into();
    } else {
        lines.push("OverrideJavaArgs=true".into());
    }
    let args = if wraps_value {
        format!("\"{}\"", args.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        args
    };
    if let Some(index) = args_line {
        lines[index] = format!("JvmArgs={args}");
    } else {
        lines.push(format!("JvmArgs={args}"));
    }
    Ok(format!("{}\n", lines.join("\n")))
}

fn quote_prism_argument(argument: &str) -> String {
    format!("\"{}\"", argument.replace('\\', "\\\\").replace('"', "\\\""))
}

fn is_quoted_config_value(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'"' || bytes[bytes.len() - 1] != b'"' {
        return false;
    }

    let mut escaped = false;
    for byte in &bytes[1..bytes.len() - 1] {
        if escaped {
            escaped = false;
        } else if *byte == b'\\' {
            escaped = true;
        } else if *byte == b'"' {
            return false;
        }
    }
    true
}

fn unescape_config_value(value: &str) -> String {
    let mut unescaped = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            match characters.next() {
                Some(escaped @ ('\\' | '"')) => unescaped.push(escaped),
                Some(next) => {
                    unescaped.push(character);
                    unescaped.push(next);
                }
                None => unescaped.push(character),
            }
        } else {
            unescaped.push(character);
        }
    }
    unescaped
}

fn extract_adapter_files(
    kit: &Path,
    destination: &Path,
    loader: StartupLoader,
) -> Result<(PathBuf, PathBuf), String> {
    let loader_id = match loader {
        StartupLoader::Forge => "forge",
        StartupLoader::NeoForge => "neoforge",
        StartupLoader::Fabric => return Err("Fabric does not use a loader adapter bundle.".into()),
    };
    let agent_name = format!("jtdl-{loader_id}-adapter-agent.jar");
    let file = File::open(kit).map_err(|error| format!("cannot open adapter kit: {error}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| format!("invalid adapter kit: {error}"))?;
    let mut agent_path = None;
    let mut bundle_path = None;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("cannot read adapter kit entry {index}: {error}"))?;
        let name = entry.name().to_string();
        if name != agent_name && !(name.ends_with(".jtdl") && !name.contains('/')) {
            continue;
        }
        let output = destination.join(&name);
        if output.exists() {
            let mut existing = File::open(&output)
                .map_err(|error| format!("cannot inspect {}: {error}", output.display()))?;
            let mut same_contents = true;
            let mut existing_chunk = [0; 8192];
            let mut generated_chunk = [0; 8192];
            loop {
                let existing_len = existing
                    .read(&mut existing_chunk)
                    .map_err(|error| format!("cannot read {}: {error}", output.display()))?;
                let generated_len = entry
                    .read(&mut generated_chunk)
                    .map_err(|error| format!("cannot read {name} from adapter kit: {error}"))?;
                if existing_len != generated_len
                    || existing_chunk[..existing_len] != generated_chunk[..generated_len]
                {
                    same_contents = false;
                    break;
                }
                if existing_len == 0 {
                    break;
                }
            }
            if !same_contents {
                return Err(format!(
                    "{} already exists with different content; it was not overwritten.",
                    output.display()
                ));
            }
        } else {
            let mut output_file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&output)
                .map_err(|error| format!("cannot create {}: {error}", output.display()))?;
            std::io::copy(&mut entry, &mut output_file)
                .and_then(|_| output_file.sync_all())
                .map_err(|error| format!("cannot extract {}: {error}", output.display()))?;
        }

        if name == agent_name {
            agent_path = Some(output);
        } else {
            bundle_path = Some(output);
        }
    }

    let agent = agent_path.ok_or_else(|| format!("adapter kit is missing {agent_name}"))?;
    let bundle = bundle_path.ok_or_else(|| "adapter kit is missing its .jtdl loader bundle".to_string())?;
    Ok((agent, bundle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;

    fn test_directory() -> PathBuf {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("jtdl-startup-test-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn startup_instance_detection_finds_game_folders_and_launcher_instances_once() {
        let root = test_directory();
        let game = root.join(".minecraft");
        let prism = root.join("PrismInstance");
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(&prism).unwrap();
        fs::write(prism.join("instance.cfg"), "InstanceType=OneSix").unwrap();

        let mut instances = Vec::new();
        add_startup_instance(&mut instances, &game, "Minecraft launcher");
        add_startup_instance(&mut instances, &game, "TLauncher");
        add_startup_instance(&mut instances, &prism, "Prism Launcher");

        assert_eq!(instances.len(), 2);
        assert!(instances
            .iter()
            .any(|instance| { instance.path == game && instance.launcher == "Minecraft game folder" }));
        assert!(instances
            .iter()
            .any(|instance| { instance.path == prism && instance.launcher == "Prism Launcher" }));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fabric_startup_config_uses_loader_property_without_copying_into_mods() {
        let root = test_directory();
        fs::create_dir_all(&root).unwrap();
        let instance = root.join("PrismInstance");
        fs::create_dir_all(&instance).unwrap();
        let config = instance.join("instance.cfg");
        fs::write(&config, "InstanceType=OneSix\nJvmArgs=-Xmx4G\n").unwrap();

        let jar = root.join("example.jar");
        let output = File::create(&jar).unwrap();
        let mut archive = zip::ZipWriter::new(output);
        archive
            .start_file("fabric.mod.json", SimpleFileOptions::default())
            .unwrap();
        archive
            .write_all(br#"{"schemaVersion":1,"id":"example","version":"1.0.0"}"#)
            .unwrap();
        archive.finish().unwrap();
        let audit = audit_jar(&jar).unwrap();

        let (loader, configured, backup, _) = configure_startup_injection(&jar, &instance, &audit).unwrap();
        assert_eq!(loader, "Fabric");
        assert_eq!(configured.as_deref(), Some(config.as_path()));
        let settings = fs::read_to_string(&config).unwrap();
        assert!(settings.contains("OverrideJavaArgs=true"));
        assert!(settings.contains(&format!(
            "-Dfabric.addMods={}",
            jar.canonicalize().unwrap().display()
        )));
        assert!(backup.unwrap().is_file());
        assert!(!instance.join("mods").exists());
        assert!(configure_startup_injection(&jar, &instance, &audit)
            .unwrap()
            .2
            .is_none());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn forge_startup_config_installs_early_agent_before_loader_scan() {
        let root = test_directory();
        fs::create_dir_all(&root).unwrap();
        let instance = root.join("PrismInstance");
        let game_dir = instance.join("minecraft");
        fs::create_dir_all(&game_dir).unwrap();
        let config = instance.join("instance.cfg");
        fs::write(&config, "InstanceType=OneSix\nJvmArgs=-Xmx4G\n").unwrap();

        let jar = root.join("example-forge.jar");
        let output = File::create(&jar).unwrap();
        let mut archive = zip::ZipWriter::new(output);
        archive
            .start_file("META-INF/mods.toml", SimpleFileOptions::default())
            .unwrap();
        archive
            .write_all(
                br#"modLoader="javafml"
loaderVersion="[1,)"
license="All Rights Reserved"
[[mods]]
modId="examplemod"
version="1.0.0"
displayName="Example Mod"
"#,
            )
            .unwrap();
        archive.finish().unwrap();
        let audit = audit_jar(&jar).unwrap();

        let (loader, configured, backup, args) =
            configure_startup_injection(&jar, &instance, &audit).unwrap();
        assert_eq!(loader, "Forge");
        assert_eq!(configured.as_deref(), Some(config.as_path()));
        assert!(backup.unwrap().is_file());
        assert_eq!(args.len(), 2);
        assert!(game_dir.join("mods").is_dir());
        assert!(instance
            .join("jtdl-startup/jtdl-forge-adapter-agent.jar")
            .is_file());
        assert!(fs::read_to_string(&config).unwrap().contains("-javaagent:"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn prism_startup_args_preserve_existing_settings_and_are_idempotent() {
        let original = "InstanceType=OneSix\nOverrideJavaArgs=false\nJvmArgs=-Xmx4G\n";
        let additions = vec![
            "-Dfabric.addMods=/home/user/mod files/example.jar".into(),
            "-Dexample=true".into(),
        ];
        let configured = update_prism_jvm_args(original, &additions).unwrap();
        assert!(configured.contains("OverrideJavaArgs=true"));
        assert!(configured.contains("JvmArgs=-Xmx4G \"-Dfabric.addMods=/home/user/mod files/example.jar\""));
        assert!(configured.contains("\"-Dexample=true\""));
        assert_eq!(
            update_prism_jvm_args(&configured, &additions).unwrap(),
            configured
        );
    }

    #[test]
    fn prism_startup_args_preserve_wrapped_values_and_repair_joined_properties() {
        let original = concat!(
            "JvmArgs=\"-Djavax.net.ssl.trustStorePassword=changeit",
            "-Dfabric.addMods=/home/user/mod.jar\"\n"
        );
        let configured =
            update_prism_jvm_args(original, &["-Dfabric.addMods=/home/user/mod.jar".into()]).unwrap();
        assert_eq!(
            configured,
            concat!(
                "JvmArgs=\"-Djavax.net.ssl.trustStorePassword=changeit ",
                "-Dfabric.addMods=/home/user/mod.jar\"\n",
                "OverrideJavaArgs=true\n"
            )
        );
        assert_eq!(
            update_prism_jvm_args(&configured, &["-Dfabric.addMods=/home/user/mod.jar".into()]).unwrap(),
            configured
        );
    }

    #[test]
    fn prism_startup_args_escape_additions_inside_wrapped_values() {
        let configured = update_prism_jvm_args(
            "JvmArgs=\"-Xmx4G\"\n",
            &["-Dfabric.addMods=/home/user/mod files/example.jar".into()],
        )
        .unwrap();
        assert!(
            configured.contains(r#"JvmArgs="-Xmx4G \"-Dfabric.addMods=/home/user/mod files/example.jar\"""#)
        );
    }

    #[test]
    fn prism_startup_args_reject_duplicate_settings() {
        let error = update_prism_jvm_args("JvmArgs=-Xmx2G\nJvmArgs=-Xmx4G", &[]).unwrap_err();
        assert!(error.contains("duplicate JvmArgs"));
    }

    #[test]
    fn prism_startup_args_preserve_existing_fabric_mod_property() {
        let error = update_prism_jvm_args(
            "JvmArgs=-Dfabric.addMods=/other/mod.jar",
            &["-Dfabric.addMods=/jtdl/mod.jar".into()],
        )
        .unwrap_err();
        assert!(error.contains("already defines -Dfabric.addMods"));
    }

    #[test]
    fn prism_argument_quoting_escapes_windows_paths() {
        let configured = update_prism_jvm_args(
            "JvmArgs=",
            &[r#"-javaagent:C:\Users\Player\JTDL\agent.jar=C:\Users\Player\JTDL\mod.jtdl"#.into()],
        )
        .unwrap();
        assert!(configured.contains(
            r#""-javaagent:C:\\Users\\Player\\JTDL\\agent.jar=C:\\Users\\Player\\JTDL\\mod.jtdl""#
        ));
    }
}
