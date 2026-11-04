use std::fs::{self, File};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use toml::Value as TomlValue;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::audit::audit_jar;

const MAX_METADATA_BYTES: u64 = 1024 * 1024;
const MAX_INPUT_JAR_BYTES: u64 = 1024 * 1024 * 1024;
const NESTED_MOD_PATH: &str = "META-INF/jars/embedded-mod.jar";
const JTDL_EMBEDDED_MOD_PATH: &str = "META-INF/jtdl/embedded-mod.jar";
const JTDL_BUNDLE_METADATA_PATH: &str = "META-INF/jtdl/bundle.properties";
const MAX_ADAPTER_METADATA_BYTES: u64 = 1024 * 1024;
const MAX_ADAPTER_INPUT_BYTES: u64 = 1024 * 1024 * 1024;
const MODLOADER_AGENT_CLASS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ModLoaderAgent.class"));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoaderKind {
    Forge,
    NeoForge,
}

impl LoaderKind {
    fn id(self) -> &'static str {
        match self {
            Self::Forge => "forge",
            Self::NeoForge => "neoforge",
        }
    }

    fn metadata_paths(self) -> &'static [&'static str] {
        match self {
            Self::Forge => &["META-INF/mods.toml", "mcmod.info"],
            Self::NeoForge => &["META-INF/neoforge.mods.toml"],
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Forge => "Forge",
            Self::NeoForge => "NeoForge",
        }
    }
}

#[derive(Clone, Debug)]
pub struct AdapterDiagnostics {
    pub mod_ids: Vec<String>,
    pub display_names: Vec<String>,
    pub versions: Vec<String>,
    pub dependencies: Vec<String>,
    pub minecraft_version_range: Option<String>,
    pub loader_version_range: Option<String>,
    pub side: Option<String>,
}

pub fn package_fabric_bundle(input_path: &Path) -> Result<PathBuf, String> {
    let metadata = fs::metadata(input_path)
        .map_err(|error| format!("cannot inspect {}: {error}", input_path.display()))?;
    if !metadata.is_file() {
        return Err("the selected input is not a regular file".into());
    }
    if metadata.len() > MAX_INPUT_JAR_BYTES {
        return Err(format!(
            "input JAR is {} bytes; the packaging safety limit is {} bytes",
            metadata.len(),
            MAX_INPUT_JAR_BYTES
        ));
    }

    let audit = audit_jar(input_path)?;
    if !audit.has_fabric_metadata {
        return Err(
            "Fabric bundle mode requires a Fabric mod with fabric.mod.json at the JAR root. Use the original loader for Forge/NeoForge/Quilt mods.".into(),
        );
    }
    if audit.fabric_metadata_valid == Some(false) {
        return Err("fabric.mod.json is not valid JSON or is missing a valid mod id/version".into());
    }

    let input_file =
        File::open(input_path).map_err(|error| format!("cannot open {}: {error}", input_path.display()))?;
    let mut source_archive =
        ZipArchive::new(input_file).map_err(|error| format!("input is not a valid JAR/ZIP: {error}"))?;
    let mut metadata_entry = source_archive
        .by_name("fabric.mod.json")
        .map_err(|_| "fabric.mod.json was not found at the JAR root".to_string())?;
    let mut metadata_bytes = Vec::new();
    metadata_entry
        .by_ref()
        .take(MAX_METADATA_BYTES + 1)
        .read_to_end(&mut metadata_bytes)
        .map_err(|error| format!("cannot read fabric.mod.json: {error}"))?;
    if metadata_bytes.len() as u64 > MAX_METADATA_BYTES {
        return Err("fabric.mod.json exceeds the 1 MiB safety limit".into());
    }
    drop(metadata_entry);
    let fabric_metadata: Value = serde_json::from_slice(&metadata_bytes)
        .map_err(|error| format!("fabric.mod.json is invalid JSON: {error}"))?;
    let source_mod_id = fabric_metadata
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "fabric.mod.json must contain a string 'id'".to_string())?;
    validate_fabric_mod_id(source_mod_id)?;
    let source_version = fabric_metadata
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    let stem = input_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("fabric-mod");
    let parent = input_path.parent().unwrap_or_else(|| Path::new("."));
    let output_path = parent.join(format!("{stem}-fabric-bundle.jar"));
    if input_path.canonicalize().ok() == output_path.canonicalize().ok() && output_path.exists() {
        return Err("refusing to overwrite the input JAR".into());
    }

    let carrier_id = carrier_mod_id(source_mod_id)?;
    let carrier_metadata = json!({
        "schemaVersion": 1,
        "id": carrier_id,
        "version": "1.0.0",
        "name": format!("JAR to Dynamic Library bundle ({source_mod_id})"),
        "description": format!(
            "A Fabric Loader carrier embedding {source_mod_id} version {source_version}. The embedded mod remains Java bytecode; this file is a compatibility package, not native AOT compilation."
        ),
        "environment": "*",
        "depends": {
            "fabricloader": ">=0.14.0"
        },
        "jars": [{ "file": NESTED_MOD_PATH }]
    });
    let carrier_json = serde_json::to_vec_pretty(&carrier_metadata)
        .map_err(|error| format!("cannot serialize carrier metadata: {error}"))?;

    drop(source_archive);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock error: {error}"))?
        .as_nanos();
    let temp_path = parent.join(format!(
        ".{stem}-fabric-bundle.{}.{}.tmp.jar",
        std::process::id(),
        stamp
    ));

    let write_result = (|| -> Result<(), String> {
        let mut input_file =
            File::open(input_path).map_err(|error| format!("cannot reopen input JAR: {error}"))?;
        let output_file =
            File::create(&temp_path).map_err(|error| format!("cannot create temporary bundle: {error}"))?;
        let mut writer = ZipWriter::new(output_file);
        let metadata_options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        writer
            .start_file("fabric.mod.json", metadata_options)
            .map_err(|error| format!("cannot write carrier metadata: {error}"))?;
        writer
            .write_all(&carrier_json)
            .map_err(|error| format!("cannot write carrier metadata: {error}"))?;

        let nested_options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        writer
            .start_file(NESTED_MOD_PATH, nested_options)
            .map_err(|error| format!("cannot create nested mod entry: {error}"))?;
        std::io::copy(&mut input_file, &mut writer)
            .map_err(|error| format!("cannot copy nested mod data: {error}"))?;
        let output_file = writer
            .finish()
            .map_err(|error| format!("cannot finalize output JAR: {error}"))?;
        output_file
            .sync_all()
            .map_err(|error| format!("cannot flush output JAR: {error}"))?;
        Ok(())
    })();

    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }

    validate_fabric_bundle_archive(&temp_path)?;
    fs::rename(&temp_path, &output_path).map_err(|error| {
        let _ = fs::remove_file(&temp_path);
        format!("cannot replace {}: {error}", output_path.display())
    })?;

    Ok(output_path)
}

pub fn package_quilt_bundle(input_path: &Path) -> Result<PathBuf, String> {
    let metadata = fs::metadata(input_path)
        .map_err(|error| format!("cannot inspect {}: {error}", input_path.display()))?;
    if !metadata.is_file() {
        return Err("the selected input is not a regular file".into());
    }
    if metadata.len() > MAX_INPUT_JAR_BYTES {
        return Err(format!(
            "input JAR is {} bytes; the packaging safety limit is {} bytes",
            metadata.len(),
            MAX_INPUT_JAR_BYTES
        ));
    }

    let audit = audit_jar(input_path)?;
    if !audit.has_quilt_metadata && !audit.has_fabric_metadata {
        return Err(
            "Quilt bundle mode requires quilt.mod.json or fabric.mod.json at the JAR root. Use the Forge/NeoForge adapter kits for those loaders.".into(),
        );
    }

    let input_file =
        File::open(input_path).map_err(|error| format!("cannot open {}: {error}", input_path.display()))?;
    let mut source_archive =
        ZipArchive::new(input_file).map_err(|error| format!("input is not a valid JAR/ZIP: {error}"))?;

    let (source_mod_id, source_version) = if audit.has_quilt_metadata {
        let mut entry = source_archive
            .by_name("quilt.mod.json")
            .map_err(|_| "quilt.mod.json was not found at the JAR root".to_string())?;
        let mut bytes = Vec::new();
        entry
            .by_ref()
            .take(MAX_METADATA_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("cannot read quilt.mod.json: {error}"))?;
        if bytes.len() as u64 > MAX_METADATA_BYTES {
            return Err("quilt.mod.json exceeds the 1 MiB safety limit".into());
        }
        drop(entry);
        let quilt_metadata: Value = serde_json::from_slice(&bytes)
            .map_err(|error| format!("quilt.mod.json is invalid JSON: {error}"))?;
        let loader = quilt_metadata.get("quilt_loader").cloned().unwrap_or(Value::Null);
        let id = loader
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| "quilt.mod.json must contain quilt_loader.id".to_string())?
            .to_string();
        let version = loader
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        (id, version)
    } else {
        let mut entry = source_archive
            .by_name("fabric.mod.json")
            .map_err(|_| "fabric.mod.json was not found at the JAR root".to_string())?;
        let mut bytes = Vec::new();
        entry
            .by_ref()
            .take(MAX_METADATA_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("cannot read fabric.mod.json: {error}"))?;
        if bytes.len() as u64 > MAX_METADATA_BYTES {
            return Err("fabric.mod.json exceeds the 1 MiB safety limit".into());
        }
        drop(entry);
        let fabric_metadata: Value = serde_json::from_slice(&bytes)
            .map_err(|error| format!("fabric.mod.json is invalid JSON: {error}"))?;
        let id = fabric_metadata
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| "fabric.mod.json must contain a string 'id'".to_string())?
            .to_string();
        let version = fabric_metadata
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        (id, version)
    };
    validate_fabric_mod_id(&source_mod_id)?;

    let stem = input_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("quilt-mod");
    let parent = input_path.parent().unwrap_or_else(|| Path::new("."));
    let output_path = parent.join(format!("{stem}-quilt-bundle.jar"));
    if input_path.canonicalize().ok() == output_path.canonicalize().ok() && output_path.exists() {
        return Err("refusing to overwrite the input JAR".into());
    }

    let carrier_id = carrier_mod_id(&source_mod_id)?;
    let carrier_metadata = json!({
        "schema_version": 1,
        "quilt_loader": {
            "group": "io.jtdl",
            "id": carrier_id,
            "version": "1.0.0",
            "metadata": {
                "name": format!("JAR to Dynamic Library bundle ({source_mod_id})"),
                "description": format!(
                    "A Quilt Loader carrier embedding {source_mod_id} version {source_version}. The embedded mod remains Java bytecode; this file is a compatibility package, not native AOT compilation."
                )
            },
            "intermediate_mappings": "net.fabricmc:intermediary",
            "jars": [NESTED_MOD_PATH]
        }
    });
    let carrier_json = serde_json::to_vec_pretty(&carrier_metadata)
        .map_err(|error| format!("cannot serialize carrier metadata: {error}"))?;

    drop(source_archive);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock error: {error}"))?
        .as_nanos();
    let temp_path = parent.join(format!(
        ".{stem}-quilt-bundle.{}.{}.tmp.jar",
        std::process::id(),
        stamp
    ));

    let write_result = (|| -> Result<(), String> {
        let mut input_file =
            File::open(input_path).map_err(|error| format!("cannot reopen input JAR: {error}"))?;
        let output_file =
            File::create(&temp_path).map_err(|error| format!("cannot create temporary bundle: {error}"))?;
        let mut writer = ZipWriter::new(output_file);
        let metadata_options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        writer
            .start_file("quilt.mod.json", metadata_options)
            .map_err(|error| format!("cannot write carrier metadata: {error}"))?;
        writer
            .write_all(&carrier_json)
            .map_err(|error| format!("cannot write carrier metadata: {error}"))?;

        let nested_options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        writer
            .start_file(NESTED_MOD_PATH, nested_options)
            .map_err(|error| format!("cannot create nested mod entry: {error}"))?;
        std::io::copy(&mut input_file, &mut writer)
            .map_err(|error| format!("cannot copy nested mod data: {error}"))?;
        let output_file = writer
            .finish()
            .map_err(|error| format!("cannot finalize output JAR: {error}"))?;
        output_file
            .sync_all()
            .map_err(|error| format!("cannot flush output JAR: {error}"))?;
        Ok(())
    })();

    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }

    let file =
        File::open(&temp_path).map_err(|error| format!("cannot validate temporary bundle: {error}"))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| format!("temporary Quilt bundle is not a valid ZIP: {error}"))?;
    archive
        .by_name("quilt.mod.json")
        .map_err(|_| "temporary Quilt bundle is missing quilt.mod.json".to_string())?;
    archive
        .by_name(NESTED_MOD_PATH)
        .map_err(|_| "temporary Quilt bundle is missing the nested mod JAR".to_string())?;
    drop(archive);

    fs::rename(&temp_path, &output_path).map_err(|error| {
        let _ = fs::remove_file(&temp_path);
        format!("cannot replace {}: {error}", output_path.display())
    })?;

    Ok(output_path)
}

pub fn package_forge_adapter_kit(input_path: &Path) -> Result<PathBuf, String> {
    package_loader_adapter_kit(input_path, LoaderKind::Forge)
}

pub fn package_neoforge_adapter_kit(input_path: &Path) -> Result<PathBuf, String> {
    package_loader_adapter_kit(input_path, LoaderKind::NeoForge)
}

pub fn analyze_loader_mod(input_path: &Path, loader: LoaderKind) -> Result<AdapterDiagnostics, String> {
    extract_adapter_diagnostics(input_path, loader)
}

pub fn uninstall_extracted_adapter_mods(
    mods_dir: &Path,
    expected_mod_ids: &[String],
) -> Result<Vec<PathBuf>, String> {
    if expected_mod_ids.is_empty() {
        return Err("no mod IDs provided for uninstall".into());
    }
    for id in expected_mod_ids {
        validate_framework_mod_id(id)?;
    }
    if !mods_dir.is_dir() {
        return Err(format!("mods directory not found: {}", mods_dir.display()));
    }

    let mut removed = Vec::new();
    let entries =
        fs::read_dir(mods_dir).map_err(|error| format!("cannot read {}: {error}", mods_dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("cannot read mods entry: {error}"))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !name.starts_with("jtdl-") || !name.ends_with(".jar") {
            continue;
        }
        let Ok(found_ids) = read_mod_ids_from_jar(&path, LoaderKind::Forge)
            .or_else(|_| read_mod_ids_from_jar(&path, LoaderKind::NeoForge))
        else {
            continue;
        };
        let matches = expected_mod_ids
            .iter()
            .all(|id| found_ids.iter().any(|found| found == id))
            && found_ids.len() == expected_mod_ids.len();
        if !matches {
            continue;
        }
        fs::remove_file(&path).map_err(|error| format!("cannot remove {}: {error}", path.display()))?;
        removed.push(path);
    }
    Ok(removed)
}

fn package_loader_adapter_kit(input_path: &Path, loader: LoaderKind) -> Result<PathBuf, String> {
    let metadata = fs::metadata(input_path)
        .map_err(|error| format!("cannot inspect {}: {error}", input_path.display()))?;
    if !metadata.is_file() {
        return Err("the selected input is not a regular file".into());
    }
    if metadata.len() > MAX_ADAPTER_INPUT_BYTES {
        return Err(format!(
            "input JAR is {} bytes; the adapter safety limit is {} bytes",
            metadata.len(),
            MAX_ADAPTER_INPUT_BYTES
        ));
    }

    let audit = audit_jar(input_path)?;
    let has_expected_metadata = match loader {
        LoaderKind::Forge => audit.has_forge_metadata,
        LoaderKind::NeoForge => audit.has_neoforge_metadata,
    };
    if !has_expected_metadata {
        let expected = match loader {
            LoaderKind::Forge => "META-INF/mods.toml (or legacy mcmod.info)",
            LoaderKind::NeoForge => "META-INF/neoforge.mods.toml",
        };
        return Err(format!(
            "{} adapter requires a JAR containing {expected} at the archive root. This tool does not port Fabric/Quilt metadata into {}.",
            loader.display_name(),
            loader.display_name()
        ));
    }

    let diagnostics = extract_adapter_diagnostics(input_path, loader)?;
    if diagnostics.mod_ids.is_empty() {
        return Err(format!(
            "could not find a mod ID in the {} metadata; refusing to create a bundle that the loader may reject",
            loader.display_name()
        ));
    }

    let source_stem = safe_archive_stem(input_path);
    let parent = input_path.parent().unwrap_or_else(|| Path::new("."));
    let kit_path = parent.join(format!("{source_stem}-{}-adapter-kit.zip", loader.id()));
    if input_path.canonicalize().ok() == kit_path.canonicalize().ok() && kit_path.exists() {
        return Err("refusing to overwrite the input file".into());
    }

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock error: {error}"))?
        .as_nanos();
    let bundle_temp = std::env::temp_dir().join(format!(
        "jtdl-{}-{stamp}-{}.jtdl",
        std::process::id(),
        loader.id()
    ));
    let kit_temp = parent.join(format!(
        ".{source_stem}-{}-adapter-kit.{}.{}.tmp.zip",
        loader.id(),
        std::process::id(),
        stamp
    ));
    let backup_path = if kit_path.exists() {
        let backup = parent.join(format!("{source_stem}-{}-adapter-kit.bak.zip", loader.id()));
        fs::rename(&kit_path, &backup).map_err(|error| format!("cannot backup existing kit: {error}"))?;
        Some(backup)
    } else {
        None
    };

    let result = (|| -> Result<PathBuf, String> {
        create_loader_bundle(
            input_path,
            &bundle_temp,
            loader,
            &source_stem,
            &diagnostics.mod_ids,
        )?;
        create_adapter_kit(&kit_temp, &bundle_temp, loader, &source_stem, &diagnostics)?;
        fs::rename(&kit_temp, &kit_path).map_err(|error| format!("cannot publish adapter kit: {error}"))?;
        Ok(kit_path.clone())
    })();

    let _ = fs::remove_file(&bundle_temp);
    let _ = fs::remove_file(&kit_temp);
    match &result {
        Ok(_) => {
            if let Some(backup) = backup_path {
                let _ = fs::remove_file(backup);
            }
        }
        Err(_) => {
            if let Some(backup) = backup_path {
                let _ = fs::rename(backup, &kit_path);
            }
        }
    }
    result
}

fn create_loader_bundle(
    input_path: &Path,
    bundle_path: &Path,
    loader: LoaderKind,
    source_stem: &str,
    source_ids: &[String],
) -> Result<(), String> {
    let output_file =
        File::create(bundle_path).map_err(|error| format!("cannot create temporary bundle: {error}"))?;
    let mut writer = ZipWriter::new(output_file);
    let metadata = format!(
        "bundleVersion=1\nloader={}\nembeddedPath={}\nsourceFileName={}\nmodIds={}\n",
        loader.id(),
        JTDL_EMBEDDED_MOD_PATH,
        source_stem,
        source_ids.join(",")
    );
    writer
        .start_file(
            JTDL_BUNDLE_METADATA_PATH,
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .map_err(|error| format!("cannot create adapter metadata: {error}"))?;
    writer
        .write_all(metadata.as_bytes())
        .map_err(|error| format!("cannot write adapter metadata: {error}"))?;

    writer
        .start_file(
            JTDL_EMBEDDED_MOD_PATH,
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )
        .map_err(|error| format!("cannot create embedded mod entry: {error}"))?;
    let mut input = File::open(input_path).map_err(|error| format!("cannot reopen input JAR: {error}"))?;
    std::io::copy(&mut input, &mut writer)
        .map_err(|error| format!("cannot copy original mod JAR: {error}"))?;
    let output = writer
        .finish()
        .map_err(|error| format!("cannot finalize adapter bundle: {error}"))?;
    output
        .sync_all()
        .map_err(|error| format!("cannot flush adapter bundle: {error}"))?;
    Ok(())
}

fn create_adapter_kit(
    kit_path: &Path,
    bundle_path: &Path,
    loader: LoaderKind,
    source_stem: &str,
    diagnostics: &AdapterDiagnostics,
) -> Result<(), String> {
    let output_file =
        File::create(kit_path).map_err(|error| format!("cannot create {}: {error}", kit_path.display()))?;
    let mut writer = ZipWriter::new(output_file);
    let bundle_name = format!("{source_stem}-{}.jtdl", loader.id());
    writer
        .start_file(
            &bundle_name,
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )
        .map_err(|error| format!("cannot add bundle to kit: {error}"))?;
    let mut bundle_file =
        File::open(bundle_path).map_err(|error| format!("cannot reopen adapter bundle: {error}"))?;
    std::io::copy(&mut bundle_file, &mut writer)
        .map_err(|error| format!("cannot copy bundle into kit: {error}"))?;

    let agent_jar = create_agent_jar(loader)?;
    writer
        .start_file(
            format!("jtdl-{}-adapter-agent.jar", loader.id()),
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )
        .map_err(|error| format!("cannot add Java agent to kit: {error}"))?;
    writer
        .write_all(&agent_jar)
        .map_err(|error| format!("cannot write Java agent: {error}"))?;

    let readme = adapter_readme(loader, &bundle_name, diagnostics);
    writer
        .start_file(
            "INSTALL.txt",
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .map_err(|error| format!("cannot add adapter instructions: {error}"))?;
    writer
        .write_all(readme.as_bytes())
        .map_err(|error| format!("cannot write adapter instructions: {error}"))?;

    let output = writer
        .finish()
        .map_err(|error| format!("cannot finalize adapter kit: {error}"))?;
    output
        .sync_all()
        .map_err(|error| format!("cannot flush adapter kit: {error}"))?;
    Ok(())
}

fn create_agent_jar(loader: LoaderKind) -> Result<Vec<u8>, String> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut cursor);
        let manifest = format!(
            "Manifest-Version: 1.0\r\nPremain-Class: io.jtdl.agent.ModLoaderAgent\r\nAgent-Class: io.jtdl.agent.ModLoaderAgent\r\nCan-Redefine-Classes: false\r\nCan-Retransform-Classes: false\r\nJtdl-Loader: {}\r\n\r\n",
            loader.id()
        );
        writer
            .start_file(
                "META-INF/MANIFEST.MF",
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .map_err(|error| format!("cannot create Java agent manifest: {error}"))?;
        writer
            .write_all(manifest.as_bytes())
            .map_err(|error| format!("cannot write Java agent manifest: {error}"))?;
        writer
            .start_file(
                "io/jtdl/agent/ModLoaderAgent.class",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .map_err(|error| format!("cannot add Java agent class: {error}"))?;
        writer
            .write_all(MODLOADER_AGENT_CLASS)
            .map_err(|error| format!("cannot write Java agent class: {error}"))?;
        writer
            .finish()
            .map_err(|error| format!("cannot finalize Java agent JAR: {error}"))?;
    }
    Ok(cursor.into_inner())
}

fn adapter_readme(loader: LoaderKind, bundle_name: &str, diagnostics: &AdapterDiagnostics) -> String {
    let agent_name = format!("jtdl-{}-adapter-agent.jar", loader.id());
    let mut deps = if diagnostics.dependencies.is_empty() {
        "none declared in metadata".to_string()
    } else {
        diagnostics.dependencies.join(", ")
    };
    if deps.len() > 500 {
        deps.truncate(500);
        deps.push_str("...");
    }
    format!(
        "JAR TO DYNAMIC LIBRARY — {} EARLY-STARTUP ADAPTER\n\n\
This kit is packaging/automation only. The original mod JAR stays Java bytecode and is extracted before {} scans mods. No native AOT conversion of Minecraft mods is performed.\n\n\
DECLARED MOD IDS\n  {}\n\
DEPENDENCIES (informational; install via the official loader)\n  {deps}\n\
MINECRAFT VERSION RANGE\n  {}\n\
LOADER VERSION RANGE\n  {}\n\
SIDE\n  {}\n\n\
FILES\n  {bundle_name}\n  {agent_name}\n\n\
INSTALLATION\n1. Extract all files from this kit to a permanent folder outside the Minecraft mods folder.\n2. Remove the original copy of this same mod from the instance mods folder to avoid duplicate mod IDs.\n3. In your Minecraft launcher JVM arguments, add BOTH of these (absolute paths required):\n\n\
   -Djtdl.modsDir=/absolute/path/to/your/instance/mods\n   -javaagent:/absolute/path/to/{agent_name}=/absolute/path/to/{bundle_name}\n\n\
4. Start Minecraft with the matching {} installation.\n\n\
UNINSTALL\nRemove the extracted jtdl-*.jar that this agent wrote into mods, then remove the JVM arguments. Deleting only the adapter kit leaves the extracted JAR installed.\n\n\
IMPORTANT\n- jtdl.modsDir is mandatory. The agent refuses to invent a mods folder from user.dir.\n- Do NOT put the .jtdl bundle into mods.\n- Duplicate-ID scanning covers .jar files under the configured mods tree (including one level of subdirectories) and ignores unreadable foreign JARs.\n- Forge and NeoForge are not interchangeable.\n",
        loader.display_name(),
        loader.display_name(),
        diagnostics.mod_ids.join(", "),
        diagnostics
            .minecraft_version_range
            .as_deref()
            .unwrap_or("not declared"),
        diagnostics
            .loader_version_range
            .as_deref()
            .unwrap_or("not declared"),
        diagnostics.side.as_deref().unwrap_or("not declared"),
        loader.display_name()
    )
}

fn extract_adapter_diagnostics(input_path: &Path, loader: LoaderKind) -> Result<AdapterDiagnostics, String> {
    let input =
        File::open(input_path).map_err(|error| format!("cannot open {}: {error}", input_path.display()))?;
    let mut archive =
        ZipArchive::new(input).map_err(|error| format!("input is not a valid JAR/ZIP: {error}"))?;

    for metadata_path in loader.metadata_paths() {
        let Ok(mut entry) = archive.by_name(metadata_path) else {
            continue;
        };
        let mut bytes = Vec::new();
        entry
            .by_ref()
            .take(MAX_ADAPTER_METADATA_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("cannot read {metadata_path}: {error}"))?;
        if bytes.len() as u64 > MAX_ADAPTER_METADATA_BYTES {
            return Err(format!("{metadata_path} exceeds the 1 MiB safety limit"));
        }
        drop(entry);
        let text = String::from_utf8_lossy(&bytes);
        let mut diagnostics = if metadata_path.ends_with(".toml") {
            parse_toml_diagnostics(&text)?
        } else {
            parse_legacy_diagnostics(&text)?
        };
        diagnostics.mod_ids.sort();
        diagnostics.mod_ids.dedup();
        if diagnostics.mod_ids.is_empty() {
            continue;
        }
        for id in &diagnostics.mod_ids {
            validate_framework_mod_id(id)?;
        }
        return Ok(diagnostics);
    }
    Err(format!("no mod IDs found in {} metadata", loader.display_name()))
}

fn read_mod_ids_from_jar(path: &Path, loader: LoaderKind) -> Result<Vec<String>, String> {
    Ok(extract_adapter_diagnostics(path, loader)?.mod_ids)
}

fn parse_toml_diagnostics(text: &str) -> Result<AdapterDiagnostics, String> {
    let value: TomlValue =
        toml::from_str(text).map_err(|error| format!("mods.toml is invalid TOML: {error}"))?;
    let mut diagnostics = AdapterDiagnostics {
        mod_ids: Vec::new(),
        display_names: Vec::new(),
        versions: Vec::new(),
        dependencies: Vec::new(),
        minecraft_version_range: None,
        loader_version_range: value
            .get("loaderVersion")
            .and_then(TomlValue::as_str)
            .map(str::to_string),
        side: None,
    };

    if let Some(mods) = value.get("mods").and_then(TomlValue::as_array) {
        for entry in mods {
            if let Some(id) = entry.get("modId").and_then(TomlValue::as_str) {
                diagnostics.mod_ids.push(id.to_string());
            }
            if let Some(name) = entry.get("displayName").and_then(TomlValue::as_str) {
                diagnostics.display_names.push(name.to_string());
            }
            if let Some(version) = entry.get("version").and_then(TomlValue::as_str) {
                diagnostics.versions.push(version.to_string());
            }
            if diagnostics.side.is_none() {
                diagnostics.side = entry.get("side").and_then(TomlValue::as_str).map(str::to_string);
            }
        }
    }

    if let Some(deps) = value.get("dependencies").and_then(TomlValue::as_table) {
        for (owner, entries) in deps {
            let Some(array) = entries.as_array() else {
                continue;
            };
            for entry in array {
                let dep_id = entry
                    .get("modId")
                    .and_then(TomlValue::as_str)
                    .unwrap_or("unknown");
                let version_range = entry
                    .get("versionRange")
                    .and_then(TomlValue::as_str)
                    .unwrap_or("*");
                let mandatory = entry
                    .get("mandatory")
                    .and_then(TomlValue::as_bool)
                    .unwrap_or(false);
                let side = entry.get("side").and_then(TomlValue::as_str).unwrap_or("BOTH");
                if dep_id.eq_ignore_ascii_case("minecraft") {
                    diagnostics.minecraft_version_range = Some(version_range.to_string());
                }
                diagnostics.dependencies.push(format!(
                    "{owner} -> {dep_id} {version_range} mandatory={mandatory} side={side}"
                ));
            }
        }
    }

    if diagnostics.mod_ids.is_empty() {
        return Err("TOML metadata has no [[mods]] modId entries".into());
    }
    Ok(diagnostics)
}

fn parse_legacy_diagnostics(text: &str) -> Result<AdapterDiagnostics, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("mcmod.info is invalid JSON: {error}"))?;
    let mut diagnostics = AdapterDiagnostics {
        mod_ids: Vec::new(),
        display_names: Vec::new(),
        versions: Vec::new(),
        dependencies: Vec::new(),
        minecraft_version_range: None,
        loader_version_range: None,
        side: None,
    };

    fn visit(value: &Value, diagnostics: &mut AdapterDiagnostics) {
        match value {
            Value::Object(object) => {
                if let Some(id) = object
                    .get("modid")
                    .or_else(|| object.get("modId"))
                    .and_then(Value::as_str)
                {
                    diagnostics.mod_ids.push(id.to_string());
                }
                if let Some(name) = object.get("name").and_then(Value::as_str) {
                    diagnostics.display_names.push(name.to_string());
                }
                if let Some(version) = object.get("version").and_then(Value::as_str) {
                    diagnostics.versions.push(version.to_string());
                }
                if let Some(mc) = object.get("mcversion").and_then(Value::as_str) {
                    diagnostics.minecraft_version_range = Some(mc.to_string());
                }
                if let Some(deps) = object.get("dependencies").and_then(Value::as_array) {
                    for dep in deps {
                        if let Some(dep_id) = dep.as_str() {
                            diagnostics.dependencies.push(dep_id.to_string());
                        }
                    }
                }
                for child in object.values() {
                    visit(child, diagnostics);
                }
            }
            Value::Array(values) => values.iter().for_each(|child| visit(child, diagnostics)),
            _ => {}
        }
    }
    visit(&value, &mut diagnostics);
    diagnostics.mod_ids.sort();
    diagnostics.mod_ids.dedup();
    if diagnostics.mod_ids.is_empty() {
        return Err("legacy mcmod.info has no modid fields".into());
    }
    Ok(diagnostics)
}

fn validate_framework_mod_id(id: &str) -> Result<(), String> {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return Err("Forge/NeoForge modId cannot be empty".into());
    };
    if id.len() < 2 || id.len() > 64 {
        return Err(format!(
            "'{id}' is not a valid Forge/NeoForge modId (length must be 2-64, got {})",
            id.len()
        ));
    }
    if !first.is_ascii_lowercase()
        || !chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
    {
        return Err(format!(
            "'{id}' is not a valid Forge/NeoForge modId (must start with a lowercase letter and use only [a-z0-9_])"
        ));
    }
    Ok(())
}

fn validate_fabric_bundle_archive(path: &Path) -> Result<(), String> {
    let file = File::open(path).map_err(|error| format!("cannot validate temporary bundle: {error}"))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| format!("temporary Fabric bundle is not a valid ZIP: {error}"))?;
    archive
        .by_name("fabric.mod.json")
        .map_err(|_| "temporary Fabric bundle is missing fabric.mod.json".to_string())?;
    archive
        .by_name(NESTED_MOD_PATH)
        .map_err(|_| "temporary Fabric bundle is missing the nested mod JAR".to_string())?;
    Ok(())
}

fn safe_archive_stem(path: &Path) -> String {
    let raw = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("embedded-mod");
    let mut output = String::new();
    for ch in raw.chars().take(70) {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            output.push(ch);
        } else {
            output.push('-');
        }
    }
    if output.is_empty() || output == "." || output == ".." {
        "embedded-mod".into()
    } else {
        output
    }
}

fn validate_fabric_mod_id(id: &str) -> Result<(), String> {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return Err("Fabric mod id cannot be empty".into());
    };
    if !first.is_ascii_lowercase()
        || id.len() > 64
        || !chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
    {
        return Err(format!(
            "'{id}' is not a valid Fabric mod id (use 1-64 lowercase letters, digits, '_' or '-', starting with a letter)"
        ));
    }
    Ok(())
}

fn carrier_mod_id(source_id: &str) -> Result<String, String> {
    if source_id.len() <= 59 {
        return Ok(format!("{source_id}-jtdl"));
    }
    if source_id.len() < 64 {
        let mut id = source_id.to_string();
        while id.len() < 64 {
            id.push('z');
        }
        return Ok(id);
    }

    let mut chars: Vec<char> = source_id.chars().collect();
    for index in (0..chars.len()).rev() {
        if chars[index] != 'z' {
            chars[index] = 'z';
            chars.truncate(index + 1);
            let id: String = chars.into_iter().collect();
            validate_fabric_mod_id(&id)?;
            return Ok(id);
        }
    }

    Err("cannot generate a distinct carrier ID after the lexicographically maximum valid 64-character Fabric ID; install the original mod JAR directly".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;

    fn temp_root() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after UNIX epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("jtdl-packaging-{}-{stamp}", std::process::id()))
    }

    #[test]
    fn quilt_bundle_preserves_original_jar_as_nested_mod() {
        let root = temp_root();
        fs::create_dir_all(&root).unwrap();
        let source = root.join("sample-quilt-mod.jar");
        let original = br#"{"schema_version":1,"quilt_loader":{"group":"com.example","id":"samplequilt","version":"2.0.0","metadata":{"name":"Sample"}}}"#;
        let mut input = ZipWriter::new(File::create(&source).unwrap());
        input
            .start_file("quilt.mod.json", SimpleFileOptions::default())
            .unwrap();
        input.write_all(original).unwrap();
        input.finish().unwrap();
        let original_bytes = fs::read(&source).unwrap();

        let output = package_quilt_bundle(&source).unwrap();
        let mut archive = ZipArchive::new(File::open(&output).unwrap()).unwrap();
        let mut nested = archive.by_name(NESTED_MOD_PATH).unwrap();
        let mut nested_bytes = Vec::new();
        nested.read_to_end(&mut nested_bytes).unwrap();
        assert_eq!(nested_bytes, original_bytes);
        drop(nested);

        let mut metadata = archive.by_name("quilt.mod.json").unwrap();
        let mut metadata_text = String::new();
        metadata.read_to_string(&mut metadata_text).unwrap();
        drop(metadata);
        let json: Value = serde_json::from_str(&metadata_text).unwrap();
        assert_eq!(json["quilt_loader"]["id"], "samplequilt-jtdl");
        assert_eq!(json["quilt_loader"]["jars"][0], NESTED_MOD_PATH);
        assert!(json["quilt_loader"]["metadata"]["description"]
            .as_str()
            .unwrap()
            .contains("not native AOT compilation"));
        drop(archive);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn quilt_bundle_accepts_fabric_metadata_for_compat_mods() {
        let root = temp_root();
        fs::create_dir_all(&root).unwrap();
        let source = root.join("sample-fabric-on-quilt.jar");
        let original =
            br#"{"schemaVersion":1,"id":"compatmod","version":"1.0.0","entrypoints":{"main":["test.Compat"]}}"#;
        let mut input = ZipWriter::new(File::create(&source).unwrap());
        input
            .start_file("fabric.mod.json", SimpleFileOptions::default())
            .unwrap();
        input.write_all(original).unwrap();
        input.finish().unwrap();

        let output = package_quilt_bundle(&source).unwrap();
        let mut archive = ZipArchive::new(File::open(&output).unwrap()).unwrap();
        let mut metadata = archive.by_name("quilt.mod.json").unwrap();
        let mut metadata_text = String::new();
        metadata.read_to_string(&mut metadata_text).unwrap();
        drop(metadata);
        let json: Value = serde_json::from_str(&metadata_text).unwrap();
        assert_eq!(json["quilt_loader"]["id"], "compatmod-jtdl");
        drop(archive);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn quilt_bundle_rejects_forge_mod() {
        let root = temp_root();
        fs::create_dir_all(&root).unwrap();
        let source = root.join("sample-forge-mod.jar");
        let mut input = ZipWriter::new(File::create(&source).unwrap());
        input
            .start_file("META-INF/mods.toml", SimpleFileOptions::default())
            .unwrap();
        input
            .write_all(b"modLoader=\"javafml\"\n[[mods]]\nmodId=\"forgemod\"\n")
            .unwrap();
        input.finish().unwrap();

        let error = package_quilt_bundle(&source).unwrap_err();
        assert!(error.contains("quilt.mod.json or fabric.mod.json"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn fabric_bundle_preserves_original_jar_as_nested_mod() {
        let root = temp_root();
        fs::create_dir_all(&root).unwrap();
        let source = root.join("sample-mod.jar");
        let original = br#"{"schemaVersion":1,"id":"samplemod","version":"1.2.3","entrypoints":{"main":["test.SampleMod"]},"mixins":["samplemod.mixins.json"]}"#;
        let mut input = ZipWriter::new(File::create(&source).unwrap());
        input
            .start_file("fabric.mod.json", SimpleFileOptions::default())
            .unwrap();
        input.write_all(original).unwrap();
        input
            .start_file("samplemod.mixins.json", SimpleFileOptions::default())
            .unwrap();
        input
            .write_all(br#"{"required":true,"package":"test","mixins":["ExampleMixin"]}"#)
            .unwrap();
        input.finish().unwrap();
        let original_bytes = fs::read(&source).unwrap();

        let output = package_fabric_bundle(&source).unwrap();
        let mut archive = ZipArchive::new(File::open(&output).unwrap()).unwrap();
        let mut nested = archive.by_name(NESTED_MOD_PATH).unwrap();
        let mut nested_bytes = Vec::new();
        nested.read_to_end(&mut nested_bytes).unwrap();
        assert_eq!(nested_bytes, original_bytes);
        drop(nested);

        let mut metadata = archive.by_name("fabric.mod.json").unwrap();
        let mut metadata_text = String::new();
        metadata.read_to_string(&mut metadata_text).unwrap();
        drop(metadata);
        let json: Value = serde_json::from_str(&metadata_text).unwrap();
        assert_eq!(json["id"], "samplemod-jtdl");
        assert!(json["id"].as_str().unwrap() > "samplemod");
        assert_eq!(json["jars"][0]["file"], NESTED_MOD_PATH);
        assert!(json["description"]
            .as_str()
            .unwrap()
            .contains("not native AOT compilation"));
        drop(archive);

        fs::remove_dir_all(root).unwrap();
    }

    fn make_test_toml_mod(path: &Path, metadata_path: &str, mod_id: &str) -> Vec<u8> {
        let metadata = format!(
            "modLoader=\"javafml\"\nloaderVersion=\"[1,)\"\nlicense=\"MIT\"\n[[mods]]\nmodId=\"{mod_id}\"\nversion=\"1.0.0\"\ndisplayName=\"Adapter Test Mod\"\n[[dependencies.{mod_id}]]\nmodId=\"minecraft\"\nmandatory=true\nversionRange=\"[1.20.1,1.21)\"\nordering=\"NONE\"\nside=\"BOTH\"\n"
        );
        let mut writer = ZipWriter::new(File::create(path).unwrap());
        writer
            .start_file(metadata_path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(metadata.as_bytes()).unwrap();
        writer
            .start_file("assets/testmod/lang/en_us.json", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(br#"{"testmod.name":"Test"}"#).unwrap();
        writer.finish().unwrap();
        fs::read(path).unwrap()
    }

    fn assert_adapter_kit_preserves_mod(
        root: &Path,
        source: &Path,
        original_bytes: &[u8],
        loader: LoaderKind,
    ) {
        let kit = match loader {
            LoaderKind::Forge => package_forge_adapter_kit(source).unwrap(),
            LoaderKind::NeoForge => package_neoforge_adapter_kit(source).unwrap(),
        };
        let mut kit_archive = ZipArchive::new(File::open(&kit).unwrap()).unwrap();
        let bundle_name = format!("{}-{}.jtdl", safe_archive_stem(source), loader.id());
        let mut bundle_entry = kit_archive.by_name(&bundle_name).unwrap();
        let mut bundle_bytes = Vec::new();
        bundle_entry.read_to_end(&mut bundle_bytes).unwrap();
        drop(bundle_entry);

        let bundle_path = root.join("test-bundle.jtdl");
        fs::write(&bundle_path, bundle_bytes).unwrap();
        let mut bundle_archive = ZipArchive::new(File::open(&bundle_path).unwrap()).unwrap();
        let mut embedded = bundle_archive.by_name(JTDL_EMBEDDED_MOD_PATH).unwrap();
        let mut embedded_bytes = Vec::new();
        embedded.read_to_end(&mut embedded_bytes).unwrap();
        assert_eq!(embedded_bytes, original_bytes);
        drop(embedded);

        let mut bundle_metadata = bundle_archive.by_name(JTDL_BUNDLE_METADATA_PATH).unwrap();
        let mut metadata_text = String::new();
        bundle_metadata.read_to_string(&mut metadata_text).unwrap();
        assert!(metadata_text.contains(&format!("loader={}\n", loader.id())));
        assert!(metadata_text.contains("modIds=jtdl_testmod\n"));
        drop(bundle_metadata);
        drop(bundle_archive);

        let agent_name = format!("jtdl-{}-adapter-agent.jar", loader.id());
        let mut agent_entry = kit_archive.by_name(&agent_name).unwrap();
        let mut agent_bytes = Vec::new();
        agent_entry.read_to_end(&mut agent_bytes).unwrap();
        drop(agent_entry);
        let agent_path = root.join(&agent_name);
        fs::write(&agent_path, agent_bytes).unwrap();
        let mut agent_archive = ZipArchive::new(File::open(&agent_path).unwrap()).unwrap();
        let mut manifest = agent_archive.by_name("META-INF/MANIFEST.MF").unwrap();
        let mut manifest_text = String::new();
        manifest.read_to_string(&mut manifest_text).unwrap();
        assert!(manifest_text.contains("Premain-Class: io.jtdl.agent.ModLoaderAgent"));
        assert!(manifest_text.contains(&format!("Jtdl-Loader: {}", loader.id())));
        drop(manifest);
        let class = agent_archive
            .by_name("io/jtdl/agent/ModLoaderAgent.class")
            .unwrap();
        assert!(class.size() > 1000);
        assert!(kit_archive.by_name("INSTALL.txt").is_ok());
    }

    #[test]
    fn forge_adapter_kit_embeds_original_mod_and_agent() {
        let root = temp_root();
        fs::create_dir_all(&root).unwrap();
        let source = root.join("sample-mod.jar");
        let original = make_test_toml_mod(&source, "META-INF/mods.toml", "jtdl_testmod");
        assert_adapter_kit_preserves_mod(&root, &source, &original, LoaderKind::Forge);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn neoforge_adapter_kit_embeds_original_mod_and_agent() {
        let root = temp_root();
        fs::create_dir_all(&root).unwrap();
        let source = root.join("sample-mod.jar");
        let original = make_test_toml_mod(&source, "META-INF/neoforge.mods.toml", "jtdl_testmod");
        assert_adapter_kit_preserves_mod(&root, &source, &original, LoaderKind::NeoForge);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn wrong_loader_metadata_is_rejected() {
        let root = temp_root();
        fs::create_dir_all(&root).unwrap();
        let source = root.join("fabric-only.jar");
        let mut writer = ZipWriter::new(File::create(&source).unwrap());
        writer
            .start_file("fabric.mod.json", SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(br#"{"schemaVersion":1,"id":"testmod","version":"1.0.0"}"#)
            .unwrap();
        writer.finish().unwrap();
        assert!(package_forge_adapter_kit(&source).is_err());
        assert!(package_neoforge_adapter_kit(&source).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn carrier_id_respects_fabric_length_limit() {
        let source_id = "a".repeat(64);
        let id = carrier_mod_id(&source_id).unwrap();
        assert_eq!(id.len(), 64);
        assert!(id.as_str() > source_id.as_str());
        validate_fabric_mod_id(&id).unwrap();
        assert!(carrier_mod_id(&"z".repeat(64)).is_err());
    }

    #[test]
    fn forge_mod_id_rules_are_strict() {
        assert!(validate_framework_mod_id("ab").is_ok());
        assert!(validate_framework_mod_id("my_mod_1").is_ok());
        assert!(validate_framework_mod_id("a").is_err());
        assert!(validate_framework_mod_id("MyMod").is_err());
        assert!(validate_framework_mod_id("my-mod").is_err());
        assert!(validate_framework_mod_id("my.mod").is_err());
        assert!(validate_framework_mod_id(&"a".repeat(65)).is_err());
    }

    #[test]
    fn adapter_kit_can_safely_replace_existing_output() {
        let root = temp_root();
        fs::create_dir_all(&root).unwrap();
        let source = root.join("sample-mod.jar");
        make_test_toml_mod(&source, "META-INF/mods.toml", "jtdl_testmod");
        let first = package_forge_adapter_kit(&source).unwrap();
        let second = package_forge_adapter_kit(&source).unwrap();
        assert_eq!(first, second);
        assert!(first.is_file());
        fs::remove_dir_all(root).unwrap();
    }
}
