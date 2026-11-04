use std::path::{Path, PathBuf};

use crate::gui::util::command;

const ATTACH_HELPER_SOURCE: &str = include_str!("../../java/AttachHelper.java");

const MINECRAFT_HINTS: &[&str] = &[
    "net.minecraft.client.main.main",
    "net.minecraft.server.main",
    "net.fabricmc.loader.impl.launch.knot.knotclient",
    "net.fabricmc.loader.impl.launch.knot.knotserver",
    "org.quiltmc.loader.impl.launch.knot.knotclient",
    "cpw.mods.bootstraplauncher.bootstraplauncher",
    "cpw.mods.modlauncher.launcher",
    "net.minecraft.launchwrapper.launch",
    "net.minecraftforge.fml",
    "neoforge",
    "fabric",
    "quilt",
    "modlauncher",
    "launchwrapper",
    "minecraft",
];

const MINECRAFT_MAIN_CLASSES: &[&str] = &[
    "net.minecraft.client.main.Main",
    "net.minecraft.server.Main",
    "net.fabricmc.loader.impl.launch.knot.KnotClient",
    "net.fabricmc.loader.impl.launch.knot.KnotServer",
    "org.quiltmc.loader.impl.launch.knot.KnotClient",
    "cpw.mods.bootstraplauncher.BootstrapLauncher",
    "cpw.mods.modlauncher.Launcher",
    "net.minecraft.launchwrapper.Launch",
];

#[derive(Clone, Debug)]
pub struct JvmProcess {
    pub pid: u32,
    pub display_name: String,
    pub likely_minecraft: bool,
}

impl JvmProcess {
    pub fn short_description(&self) -> String {
        let text = self.display_name.trim();
        if text.is_empty() {
            return "unnamed JVM".to_string();
        }
        let head = text.split_whitespace().next().unwrap_or(text);
        let tail = head.rsplit(['/', '\\']).next().unwrap_or(head);
        if tail.chars().count() > 64 {
            let kept: String = tail.chars().take(61).collect();
            format!("{kept}...")
        } else {
            tail.to_string()
        }
    }
}

fn looks_like_minecraft(command_line: &str) -> bool {
    let lower = command_line.to_ascii_lowercase();
    if MINECRAFT_HINTS.iter().any(|hint| lower.contains(hint)) {
        return true;
    }

    lower.contains("--gamedir")
        && (lower.contains("--assetsdir") || lower.contains("--assetindex"))
        && (lower.contains("--version") || lower.contains("--username"))
}

fn executable_name(path: &str) -> &str {
    let path = path.trim().trim_matches('"');
    let path = path.strip_suffix(" (deleted)").unwrap_or(path);
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn is_java_executable(path: &str) -> bool {
    matches!(
        executable_name(path).to_ascii_lowercase().as_str(),
        "java" | "java.exe" | "javaw.exe"
    )
}

fn first_command_token(command_line: &str) -> &str {
    let command_line = command_line.trim();
    if let Some(quote) = command_line.chars().next().filter(|c| *c == '"' || *c == '\'') {
        return command_line[1..].split(quote).next().unwrap_or(command_line);
    }
    command_line.split_whitespace().next().unwrap_or(command_line)
}

fn process_from_metadata(pid: u32, executable: &str, command_line: &str) -> Option<JvmProcess> {
    if pid == std::process::id() {
        return None;
    }

    let argv0 = first_command_token(command_line);
    if !is_java_executable(executable) && !is_java_executable(argv0) {
        return None;
    }

    let searchable = format!("{executable} {command_line}");
    let likely_minecraft = looks_like_minecraft(&searchable);
    let display_name = MINECRAFT_MAIN_CLASSES
        .iter()
        .find(|class| searchable.contains(**class))
        .map(|class| class.rsplit('.').next().unwrap_or(class).to_string())
        .or_else(|| {
            if likely_minecraft {
                Some("Minecraft".to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| {
            let name = executable_name(executable);
            if name.is_empty() {
                "Java".to_string()
            } else {
                name.to_string()
            }
        });

    Some(JvmProcess {
        pid,
        display_name,
        likely_minecraft,
    })
}

#[cfg(target_os = "linux")]
fn enumerate_host_processes() -> Result<Vec<JvmProcess>, String> {
    let entries = std::fs::read_dir("/proc").map_err(|error| format!("cannot enumerate /proc: {error}"))?;
    let mut processes = Vec::new();

    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        let root = entry.path();
        let executable = std::fs::read_link(root.join("exe"))
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|_| {
                std::fs::read_to_string(root.join("comm"))
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            });
        let raw_args = std::fs::read(root.join("cmdline")).unwrap_or_default();
        let command_line = raw_args
            .split(|byte| *byte == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect::<Vec<_>>()
            .join(" ");

        if let Some(process) = process_from_metadata(pid, &executable, &command_line) {
            processes.push(process);
        }
    }

    Ok(processes)
}

#[cfg(target_os = "macos")]
fn enumerate_host_processes() -> Result<Vec<JvmProcess>, String> {
    let output = command("ps")
        .args(["-wwaxo", "pid=,ucomm=,args="])
        .output()
        .map_err(|error| format!("cannot run ps to enumerate processes: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "ps could not enumerate processes: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let mut processes = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut fields = line.split_whitespace();
        let Some(pid) = fields.next().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        let executable = fields.next().unwrap_or("");
        let command_line = fields.collect::<Vec<_>>().join(" ");
        if let Some(process) = process_from_metadata(pid, executable, &command_line) {
            processes.push(process);
        }
    }
    Ok(processes)
}

#[cfg(windows)]
fn enumerate_host_processes() -> Result<Vec<JvmProcess>, String> {
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
if (Get-Command Get-CimInstance -ErrorAction SilentlyContinue) {
    $items = Get-CimInstance -ClassName Win32_Process -Filter "Name='java.exe' OR Name='javaw.exe'"
} else {
    $items = Get-WmiObject -Class Win32_Process -Filter "Name='java.exe' OR Name='javaw.exe'"
}
foreach ($item in $items) {
    $commandLine = [regex]::Replace([string]$item.CommandLine, '[\t\r\n]', ' ')
    $line = [string]$item.ProcessId + [char]9 + [string]$item.Name + [char]9 + $commandLine
    [Console]::Out.WriteLine($line)
}
"#;
    let output = command("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .output()
        .map_err(|error| format!("cannot run PowerShell to enumerate Java processes: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Windows process enumeration failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let mut processes = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut fields = line.splitn(3, '\t');
        let Some(pid) = fields.next().and_then(|value| value.trim().parse::<u32>().ok()) else {
            continue;
        };
        let executable = fields.next().unwrap_or("").trim();
        let command_line = fields.next().unwrap_or("").trim();
        if let Some(process) = process_from_metadata(pid, executable, command_line) {
            processes.push(process);
        }
    }
    Ok(processes)
}

pub fn java_executable() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        let bin = PathBuf::from(&home)
            .join("bin")
            .join(if cfg!(windows) { "java.exe" } else { "java" });
        if bin.is_file() {
            return Some(bin);
        }
    }
    let candidate = if cfg!(windows) { "java.exe" } else { "java" };
    if command(candidate)
        .arg("-version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
    {
        return Some(PathBuf::from(candidate));
    }
    None
}

fn javac_executable(java: &Path) -> PathBuf {
    let name = if cfg!(windows) { "javac.exe" } else { "javac" };
    let sibling = java.parent().map(|dir| dir.join(name));
    match sibling {
        Some(path) if path.is_file() => path,
        _ => PathBuf::from(name),
    }
}

pub(crate) fn java_major_version(java: &Path) -> Option<u32> {
    let output = command(&java.to_string_lossy()).arg("-version").output().ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let quoted = text.split('"').nth(1)?;
    let first = quoted.split(['.', '-', '+']).next()?;
    let first: u32 = first.parse().ok()?;
    if first == 1 {
        let second = quoted.split('.').nth(1)?.parse().ok()?;
        Some(second)
    } else {
        Some(first)
    }
}

fn attach_module_args(major: Option<u32>) -> Vec<&'static str> {
    match major {
        Some(version) if version >= 9 => vec!["--add-modules", "jdk.attach"],
        _ => Vec::new(),
    }
}

pub(crate) fn tools_jar_classpath(java: &Path, major: Option<u32>) -> Option<PathBuf> {
    if major.unwrap_or(9) >= 9 {
        return None;
    }
    let candidate = java.parent()?.parent()?.join("lib").join("tools.jar");
    candidate.is_file().then_some(candidate)
}

fn helper_dir() -> PathBuf {
    std::env::temp_dir()
        .join("jar-to-dynamic-library-gui")
        .join("attach-helper")
}

fn ensure_attach_helper(java: &Path, major: Option<u32>) -> Result<PathBuf, String> {
    let dir = helper_dir();
    std::fs::create_dir_all(&dir).map_err(|error| format!("cannot create helper dir: {error}"))?;
    let source_path = dir.join("AttachHelper.java");
    let class_path = dir.join("AttachHelper.class");

    let needs_write = std::fs::read_to_string(&source_path)
        .map(|existing| existing != ATTACH_HELPER_SOURCE)
        .unwrap_or(true);
    if needs_write {
        std::fs::write(&source_path, ATTACH_HELPER_SOURCE)
            .map_err(|error| format!("cannot write AttachHelper.java: {error}"))?;
        let _ = std::fs::remove_file(&class_path);
    }
    if class_path.is_file() {
        return Ok(dir);
    }

    let javac = javac_executable(java);
    let mut cmd = command(&javac.to_string_lossy());
    cmd.args(attach_module_args(major));
    if let Some(tools) = tools_jar_classpath(java, major) {
        cmd.arg("-cp").arg(tools);
    }
    cmd.arg("-d").arg(&dir).arg(&source_path);
    let output = cmd
        .output()
        .map_err(|error| format!("cannot run {}: {error}", javac.display()))?;
    if !output.status.success() {
        return Err(format!(
            "failed to compile the attach helper (requires a full JDK, not just a JRE):\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(dir)
}

fn run_helper(java: &Path, major: Option<u32>, helper_dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = command(&java.to_string_lossy());
    cmd.args(attach_module_args(major));
    let mut classpath = helper_dir.to_path_buf().into_os_string();
    if let Some(tools) = tools_jar_classpath(java, major) {
        classpath.push(if cfg!(windows) { ";" } else { ":" });
        classpath.push(tools);
    }
    cmd.arg("-cp").arg(classpath).arg("AttachHelper").args(args);
    let output = cmd
        .output()
        .map_err(|error| format!("cannot run java: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.starts_with("ERROR") {
        return Err(stdout);
    }
    if !output.status.success() && stdout.is_empty() {
        return Err(format!(
            "attach helper exited with {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(stdout)
}

pub fn list_jvm_processes() -> Result<Vec<JvmProcess>, String> {
    let mut processes = enumerate_host_processes()?;
    processes.sort_by(|a, b| {
        b.likely_minecraft
            .cmp(&a.likely_minecraft)
            .then(a.pid.cmp(&b.pid))
    });
    Ok(processes)
}

pub fn attach_native_library(pid: u32, library_path: &Path, options: Option<&str>) -> Result<(), String> {
    let java = java_executable().ok_or_else(|| {
        "A Java process was found, but no JDK launcher was found for Attach API. Install a full JDK (not only a JRE), then set JAVA_HOME or add its bin directory to PATH. Process detection itself does not require a system Java installation.".to_string()
    })?;
    let major = java_major_version(&java);
    let dir = ensure_attach_helper(&java, major)?;
    let pid_string = pid.to_string();
    let path_string = library_path.to_string_lossy().to_string();
    let mut args = vec!["inject", pid_string.as_str(), path_string.as_str()];
    if let Some(options) = options {
        args.push(options);
    }
    let output = run_helper(&java, major, &dir, &args)?;
    if output.trim() == "OK" {
        Ok(())
    } else {
        Err(format!("attach helper returned: {output}"))
    }
}

pub fn attach_agent_jar(pid: u32, agent_jar: &Path, agent_args: Option<&str>) -> Result<(), String> {
    let java = java_executable().ok_or_else(|| {
        "A Java process was found, but no JDK launcher was found for Attach API. Install a full JDK (not only a JRE), then set JAVA_HOME or add its bin directory to PATH. Process detection itself does not require a system Java installation.".to_string()
    })?;
    let major = java_major_version(&java);
    let dir = ensure_attach_helper(&java, major)?;
    let jar_string = agent_jar.to_string_lossy().to_string();
    let pid_string = pid.to_string();
    let mut args = vec!["inject-jar", pid_string.as_str(), jar_string.as_str()];
    if let Some(extra) = agent_args {
        args.push(extra);
    }
    let output = run_helper(&java, major, &dir, &args)?;
    if output.trim() == "OK" {
        Ok(())
    } else {
        Err(format!("attach helper returned: {output}"))
    }
}

fn libraries_dir(jar: &Path) -> PathBuf {
    let stem = jar
        .file_stem()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "mod".to_string());
    jar.parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{stem}-libraries"))
}

fn stem_of(jar: &Path) -> String {
    jar.file_stem()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "mod".to_string())
}

fn host_library_name(jar: &Path) -> String {
    let stem = stem_of(jar);
    if cfg!(windows) {
        format!("{stem}-windows-x86_64.dll")
    } else if cfg!(target_os = "macos") {
        format!("{stem}-macos-{}.dylib", std::env::consts::ARCH)
    } else {
        format!("{stem}-linux-x86_64.so")
    }
}

pub fn native_library_path(jar: &Path) -> PathBuf {
    libraries_dir(jar).join(host_library_name(jar))
}

pub fn agent_jar_path(jar: &Path) -> PathBuf {
    libraries_dir(jar).join(format!("{}-agent.jar", stem_of(jar)))
}

pub fn host_injector_library(jar: &Path) -> Option<PathBuf> {
    let candidate = native_library_path(jar);
    candidate.is_file().then_some(candidate)
}

pub fn host_agent_jar(jar: &Path) -> Option<PathBuf> {
    let candidate = agent_jar_path(jar);
    candidate.is_file().then_some(candidate)
}

pub fn process_architecture(pid: u32) -> Option<&'static str> {
    #[cfg(target_os = "linux")]
    {
        let head = read_head(&PathBuf::from(format!("/proc/{pid}/exe")), 21)?;
        if head[0..4] != [0x7f, b'E', b'L', b'F'] {
            return None;
        }

        let machine = u16::from_le_bytes([head[18], head[19]]);
        Some(match machine {
            0x3E => "x86_64",
            0x03 => "x86",
            0xB7 => "aarch64",
            0x28 => "arm",
            0xF3 => "riscv64",
            _ => return None,
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        None
    }
}

#[cfg(target_os = "linux")]
fn read_head(path: &Path, bytes: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut buffer = vec![0u8; bytes];
    file.read_exact(&mut buffer).ok()?;
    Some(buffer)
}

pub(crate) fn library_architecture(path: &Path) -> Option<&'static str> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    ["x86_64", "aarch64", "arm", "riscv64", "x86"]
        .iter()
        .find(|arch| name.contains(*arch))
        .copied()
}

pub fn architecture_mismatch(library: &Path, pid: u32) -> Option<String> {
    let wanted = library_architecture(library)?;
    let running = process_architecture(pid)?;
    if wanted == running {
        return None;
    }
    Some(format!(
        "The library was built for {wanted} but the JVM of PID {pid} runs as {running}. Build that \
         target (or switch the route to the Java agent, which is architecture independent and needs no \
         native library at all)."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_artefacts_are_expected_next_to_the_jar() {
        let jar = Path::new("/tmp/examplemod-1.0.jar");
        assert_eq!(
            agent_jar_path(jar),
            PathBuf::from("/tmp/examplemod-1.0-libraries/examplemod-1.0-agent.jar")
        );
        let library = native_library_path(jar);
        assert_eq!(
            library.parent().unwrap(),
            Path::new("/tmp/examplemod-1.0-libraries")
        );
        assert!(library
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("examplemod-1.0-"));
    }

    #[test]
    fn an_artefact_name_states_the_architecture_it_was_built_for() {
        assert_eq!(
            library_architecture(Path::new("/x/mod-linux-x86_64.so")),
            Some("x86_64")
        );
        assert_eq!(
            library_architecture(Path::new("/x/mod-macos-aarch64.dylib")),
            Some("aarch64")
        );
        assert_eq!(library_architecture(Path::new("/x/mod-agent.jar")), None);
    }

    #[test]
    fn a_process_whose_architecture_is_unknown_is_never_blocked() {
        let no_process = u32::MAX;
        assert!(process_architecture(no_process).is_none());
        assert!(architecture_mismatch(Path::new("/x/mod-linux-aarch64.so"), no_process).is_none());
    }

    #[test]
    fn recognizes_java_and_javaw_from_bundled_runtimes() {
        assert!(is_java_executable("/opt/launcher/runtime/bin/java"));
        assert!(is_java_executable("C:\\Games\\Launcher\\runtime\\bin\\javaw.exe"));
        assert!(is_java_executable("/opt/runtime/bin/java (deleted)"));
        assert!(!is_java_executable("/usr/bin/node"));
    }

    #[test]
    fn detects_vanilla_and_modded_minecraft_without_knowing_the_launcher() {
        let vanilla = process_from_metadata(
            1001,
            "/opt/custom-runtime/bin/java",
            "/opt/custom-runtime/bin/java -Xmx4G net.minecraft.client.main.Main --username Player --version 1.21 --gameDir /home/player/.minecraft --assetsDir /home/player/.minecraft/assets --assetIndex 17",
        )
        .expect("vanilla Java process should be listed");
        assert!(vanilla.likely_minecraft);
        assert_eq!(vanilla.display_name, "Main");

        let fabric = process_from_metadata(
            1002,
            "/custom/java",
            "/custom/java -Xmx4G net.fabricmc.loader.impl.launch.knot.KnotClient --gameDir /games/instance",
        )
        .expect("Fabric Java process should be listed");
        assert!(fabric.likely_minecraft);
        assert_eq!(fabric.display_name, "KnotClient");

        let forge = process_from_metadata(
            1003,
            "javaw.exe",
            "javaw.exe -cp libraries\\* cpw.mods.bootstraplauncher.BootstrapLauncher --launchTarget forgeclient",
        )
        .expect("Forge Java process should be listed");
        assert!(forge.likely_minecraft);
        assert_eq!(forge.display_name, "BootstrapLauncher");
    }

    #[test]
    fn keeps_unrecognized_java_apps_selectable_without_marking_them_as_minecraft() {
        let process = process_from_metadata(
            1004,
            "/opt/jdk/bin/java",
            "/opt/jdk/bin/java -jar /opt/apps/idea.jar",
        )
        .expect("other Java applications should remain visible");
        assert!(!process.likely_minecraft);
        assert_eq!(process.display_name, "java");
    }

    #[test]
    fn minecraft_flags_only_match_when_the_launcher_arguments_are_consistent() {
        assert!(!looks_like_minecraft("java --gameDir /tmp/test"));
        assert!(looks_like_minecraft(
            "java --gameDir /tmp/game --assetsDir /tmp/assets --version 1.21"
        ));
    }
}
