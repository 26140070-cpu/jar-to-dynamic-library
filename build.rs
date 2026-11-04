#[path = "build/classfile.rs"]
mod classfile;
#[path = "build/jar.rs"]
mod jar;

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const SOURCE: &str = "java/ForgeInjector.java";
const JAR_INJECTOR_SOURCE: &str = "java/JarInjector.java";
const AGENT_SOURCE: &str = "java/ModLoaderAgent.java";
const CLASS_NAME: &str = "ForgeInjector.class";
const JAR_INJECTOR_CLASS_NAME: &str = "JarInjector.class";
const AGENT_CLASS_NAME: &str = "ModLoaderAgent.class";
const AGENT_JAR_NAME: &str = "jtdl-agent.jar";
const MAX_MAJOR_VERSION: u16 = 52;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/windows.rc");
    println!("cargo:rerun-if-changed=assets/images/app-icon.ico");
    println!("cargo:rerun-if-changed={SOURCE}");
    println!("cargo:rerun-if-changed={JAR_INJECTOR_SOURCE}");
    println!("cargo:rerun-if-changed={AGENT_SOURCE}");
    println!("cargo:rerun-if-env-changed=JAVAC");
    println!("cargo:rerun-if-env-changed=JAVA_HOME");
    println!("cargo:rerun-if-env-changed=INJECTOR_CLASS_FILE");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && env::var_os("CARGO_FEATURE_GUI").is_some()
    {
        embed_resource::compile_for(
            "assets/windows.rc",
            ["jar-to-dynamic-library-gui"],
            embed_resource::NONE,
        )
        .manifest_optional()
        .expect("could not embed the Windows application icon");
    }

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let destination = out_dir.join(CLASS_NAME);

    if let Some(prebuilt) = env::var_os("INJECTOR_CLASS_FILE") {
        println!("cargo:rerun-if-changed={}", Path::new(&prebuilt).display());
        fs::copy(&prebuilt, &destination)
            .unwrap_or_else(|e| panic!("can't copy INJECTOR_CLASS_FILE {prebuilt:?}: {e}"));
    } else {
        compile_injector(&out_dir, &destination);
    }

    check_class_file(&destination);

    let jar_injector_dir = compile_java(&out_dir, JAR_INJECTOR_SOURCE, "jar-injector-classes");
    let jar_injector_class = jar_injector_dir.join(JAR_INJECTOR_CLASS_NAME);
    check_class_file(&jar_injector_class);
    fs::copy(&jar_injector_class, out_dir.join(JAR_INJECTOR_CLASS_NAME))
        .unwrap_or_else(|e| panic!("can't publish {JAR_INJECTOR_CLASS_NAME}: {e}"));
    let jar_injector_classes = collect_classes(&jar_injector_dir);
    write_agent_jar(&out_dir, &jar_injector_classes);

    write_jar_injector_classes(&out_dir, &jar_injector_classes);

    let agent_destination = out_dir.join(AGENT_CLASS_NAME);
    compile_modloader_agent(&out_dir, &agent_destination);
    check_class_file(&agent_destination);

    embed_input_jar(&out_dir);
}

fn embed_input_jar(out_dir: &Path) {
    println!("cargo:rerun-if-env-changed=INPUT_JAR");
    let jar_path = match env::var_os("INPUT_JAR") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("set by cargo")).join("input.jar"),
    };
    println!("cargo:rerun-if-changed={}", jar_path.display());

    let class_dir = out_dir.join("jar-classes");
    let _ = fs::remove_dir_all(&class_dir);
    fs::create_dir_all(&class_dir).expect("can't create jar-classes dir");

    let jar = if jar_path.is_file() {
        jar::read_jar(&jar_path).unwrap_or_else(|e| panic!("input jar: {e}"))
    } else if env::var("PROFILE").as_deref() == Ok("release") && env::var_os("CARGO_FEATURE_GUI").is_none() {
        panic!(
            "input jar not found at {}.\nPut your mod there as input.jar or set INPUT_JAR=/path/to/mod.jar",
            jar_path.display()
        );
    } else {
        println!(
            "cargo:warning=no input jar at {}: building with ZERO mod classes (debug build or GUI build)",
            jar_path.display()
        );
        jar::JarClasses {
            classes: Vec::new(),
            skipped: Vec::new(),
        }
    };

    for note in &jar.skipped {
        println!("cargo:warning=skipped jar entry: {note}");
    }

    let mut generated = String::from("pub static JAR_CLASSES: &[&[u8]] = &[\n");
    let mut manifest = String::new();
    for (index, class) in jar.classes.iter().enumerate() {
        let file = class_dir.join(format!("{index}.class"));
        fs::write(&file, &class.data).expect("can't write class file");
        generated.push_str(&format!("    include_bytes!({:?}),\n", file.to_string_lossy()));
        manifest.push_str(&format!("{index}\tpriority={}\t{}\n", class.priority, class.name));
    }
    generated.push_str("];\n");
    let baked = if jar_path.is_file() {
        fs::canonicalize(&jar_path)
            .unwrap_or_else(|_| jar_path.clone())
            .to_string_lossy()
            .into_owned()
    } else {
        String::new()
    };
    generated.push_str(&format!("pub static BAKED_JAR_PATH: &str = {:?};\n", baked));

    fs::write(out_dir.join("jar_classes.rs"), generated).expect("can't write jar_classes.rs");
    fs::write(class_dir.join("manifest.txt"), manifest).expect("can't write manifest");
}

fn compile_java(out_dir: &Path, source: &str, directory: &str) -> PathBuf {
    let javac = javac_path();
    let class_dir = out_dir.join(directory);
    let _ = fs::remove_dir_all(&class_dir);
    fs::create_dir_all(&class_dir).expect("can't create class output dir");

    let attempts: [&[&str]; 2] = [&["--release", "8"], &["-source", "1.8", "-target", "1.8"]];
    let mut last_error = String::new();
    for flags in attempts {
        let output = Command::new(&javac)
            .args(flags)
            .args(["-encoding", "UTF-8", "-nowarn", "-d"])
            .arg(&class_dir)
            .arg(source)
            .output();
        match output {
            Ok(out) if out.status.success() => return class_dir,
            Ok(out) => {
                last_error = format!(
                    "`javac {}` failed for {source} ({}):\n{}",
                    flags.join(" "),
                    out.status,
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            Err(e) => panic!(
                "can't run {javac:?} to compile {source}: {e}\n\
                 Install a JDK (8 or newer), set JAVAC / JAVA_HOME."
            ),
        }
    }
    panic!("{last_error}");
}

fn collect_classes(class_dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut classes: Vec<(String, Vec<u8>)> = Vec::new();
    for entry in fs::read_dir(class_dir).expect("jar-injector class dir is missing") {
        let path = entry.expect("readable class entry").path();
        if path.extension().and_then(|name| name.to_str()) != Some("class") {
            continue;
        }
        let name = path
            .file_name()
            .expect("class file name")
            .to_string_lossy()
            .to_string();
        let bytes = fs::read(&path).expect("readable class file");
        check_class_bytes(&name, &bytes);
        classes.push((name, bytes));
    }
    classes.sort_by(|left, right| left.0.cmp(&right.0));
    assert!(
        !classes.is_empty(),
        "no JarInjector class files were produced by javac"
    );
    classes
}

fn write_jar_injector_classes(out_dir: &Path, classes: &[(String, Vec<u8>)]) {
    let byte_dir = out_dir.join("jar-injector-bytes");
    let _ = fs::remove_dir_all(&byte_dir);
    fs::create_dir_all(&byte_dir).expect("can't create jar-injector-bytes dir");
    let mut generated = String::from("pub static JAR_INJECTOR_CLASSES: &[(&str, &[u8])] = &[\n");
    for (index, (file_name, bytes)) in classes.iter().enumerate() {
        let destination = byte_dir.join(format!("{index}.class"));
        fs::write(&destination, bytes).expect("can't write an injector class file");
        let class_name = file_name.trim_end_matches(".class");
        generated.push_str(&format!(
            "    ({:?}, include_bytes!({:?})),\n",
            class_name,
            destination.to_string_lossy()
        ));
    }
    generated.push_str("];\n");
    fs::write(out_dir.join("jar_injector_classes.rs"), generated)
        .expect("can't write jar_injector_classes.rs");
}

fn write_agent_jar(out_dir: &Path, classes: &[(String, Vec<u8>)]) {
    let manifest = "Manifest-Version: 1.0\r\n\
         Agent-Class: JarInjector\r\n\
         Premain-Class: JarInjector\r\n\
         Can-Redefine-Classes: true\r\n\
         Can-Retransform-Classes: true\r\n\
         Can-Set-Native-Method-Prefix: true\r\n\
         Jtdl-Injector: jar\r\n\
         \r\n";

    let target = out_dir.join(AGENT_JAR_NAME);
    let file = fs::File::create(&target).expect("can't create jtdl-agent.jar");
    let mut writer = ZipWriter::new(file);
    writer
        .start_file(
            "META-INF/MANIFEST.MF",
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )
        .expect("can't add the agent manifest");
    writer
        .write_all(manifest.as_bytes())
        .expect("can't write the manifest");
    for (name, bytes) in classes {
        writer
            .start_file(
                name.as_str(),
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .expect("can't add an agent class");
        writer.write_all(bytes).expect("can't write an agent class");
    }
    writer
        .finish()
        .expect("can't finalize the agent JAR")
        .sync_all()
        .expect("can't flush the agent JAR");
}

fn javac_path() -> PathBuf {
    if let Some(javac) = env::var_os("JAVAC") {
        return PathBuf::from(javac);
    }
    if let Some(home) = env::var_os("JAVA_HOME") {
        let candidate = Path::new(&home)
            .join("bin")
            .join(if cfg!(windows) { "javac.exe" } else { "javac" });
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from("javac")
}

fn compile_injector(out_dir: &Path, destination: &Path) {
    let javac = javac_path();
    let class_dir = out_dir.join("injector-classes");
    let _ = fs::remove_dir_all(&class_dir);
    fs::create_dir_all(&class_dir).expect("can't create class output dir");

    let attempts: [&[&str]; 2] = [&["--release", "8"], &["-source", "1.8", "-target", "1.8"]];
    let mut last_error = String::new();

    for flags in attempts {
        let output = Command::new(&javac)
            .args(flags)
            .args(["-encoding", "UTF-8", "-d"])
            .arg(&class_dir)
            .arg(SOURCE)
            .output();

        match output {
            Ok(out) if out.status.success() => {
                fs::copy(class_dir.join(CLASS_NAME), destination)
                    .expect("javac succeeded but ForgeInjector.class is missing");
                return;
            }
            Ok(out) => {
                last_error = format!(
                    "`javac {}` failed ({}):\n{}",
                    flags.join(" "),
                    out.status,
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            Err(e) => {
                panic!(
                    "can't run {javac:?}: {e}\n\
                     Install a JDK (8 or newer), set JAVAC / JAVA_HOME, \
                     or point INJECTOR_CLASS_FILE at a prebuilt ForgeInjector.class"
                );
            }
        }
    }
    panic!("{last_error}");
}

fn compile_modloader_agent(out_dir: &Path, destination: &Path) {
    let javac = javac_path();
    let class_dir = out_dir.join("modloader-agent-classes");
    let _ = fs::remove_dir_all(&class_dir);
    fs::create_dir_all(&class_dir).expect("can't create mod-loader agent class dir");

    let attempts: [&[&str]; 2] = [&["--release", "8"], &["-source", "1.8", "-target", "1.8"]];
    let mut last_error = String::new();
    for flags in attempts {
        let output = Command::new(&javac)
            .args(flags)
            .args(["-encoding", "UTF-8", "-d"])
            .arg(&class_dir)
            .arg(AGENT_SOURCE)
            .output();
        match output {
            Ok(out) if out.status.success() => {
                let compiled = class_dir.join("io").join("jtdl").join("agent").join(AGENT_CLASS_NAME);
                fs::copy(&compiled, destination)
                    .expect("javac succeeded but ModLoaderAgent.class is missing");
                return;
            }
            Ok(out) => {
                last_error = format!(
                    "`javac {}` failed for mod-loader agent ({}):\n{}",
                    flags.join(" "),
                    out.status,
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            Err(e) => panic!(
                "can't run {javac:?} to compile the mod-loader agent: {e}\nInstall a JDK (8 or newer), set JAVAC / JAVA_HOME."
            ),
        }
    }
    panic!("{last_error}");
}

fn check_class_file(path: &Path) {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("can't read {}: {e}", path.display()));
    check_class_bytes(&path.display().to_string(), &bytes);
}

fn check_class_bytes(label: &str, bytes: &[u8]) {
    assert!(
        bytes.len() >= 8 && bytes[..4] == [0xCA, 0xFE, 0xBA, 0xBE],
        "{label} is not a class file"
    );
    let major = u16::from_be_bytes([bytes[6], bytes[7]]);
    assert!(
        major <= MAX_MAJOR_VERSION,
        "{label} has major version {major}; it must be compiled for Java 8 (<= {MAX_MAJOR_VERSION})"
    );
}
