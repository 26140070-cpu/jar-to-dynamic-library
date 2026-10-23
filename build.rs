#[path = "build/classfile.rs"]
mod classfile;
#[path = "build/jar.rs"]
mod jar;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const SOURCE: &str = "java/ForgeInjector.java";
const CLASS_NAME: &str = "ForgeInjector.class";
const MAX_MAJOR_VERSION: u16 = 52;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={SOURCE}");
    println!("cargo:rerun-if-env-changed=JAVAC");
    println!("cargo:rerun-if-env-changed=JAVA_HOME");
    println!("cargo:rerun-if-env-changed=INJECTOR_CLASS_FILE");

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

    fs::write(out_dir.join("jar_classes.rs"), generated).expect("can't write jar_classes.rs");
    fs::write(class_dir.join("manifest.txt"), manifest).expect("can't write manifest");
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

fn check_class_file(path: &Path) {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("can't read {}: {e}", path.display()));
    assert!(
        bytes.len() >= 8 && bytes[..4] == [0xCA, 0xFE, 0xBA, 0xBE],
        "{} is not a class file",
        path.display()
    );
    let major = u16::from_be_bytes([bytes[6], bytes[7]]);
    assert!(
        major <= MAX_MAJOR_VERSION,
        "injector class has major version {major}; it must be compiled for Java 8 (<= {MAX_MAJOR_VERSION})"
    );
}
