use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde_json::Value;
use zip::ZipArchive;

const MAX_ARCHIVE_ENTRIES: usize = 250_000;
const MAX_METADATA_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Default, Clone)]
pub struct JarAudit {
    pub class_count: usize,
    pub resource_count: usize,
    pub has_fabric_metadata: bool,
    pub has_quilt_metadata: bool,
    pub has_forge_metadata: bool,
    pub has_neoforge_metadata: bool,
    pub has_mixins: bool,
    pub has_nested_jars: bool,
    pub has_service_descriptors: bool,
    pub has_manifest_class_path: bool,
    pub has_main_class: bool,
    pub has_mixin_plugin: bool,
    pub has_access_widener: bool,
    pub has_access_transformer: bool,
    pub has_language_adapters: bool,
    pub has_legacy_coremod: bool,
    pub fabric_metadata_valid: Option<bool>,
    pub fabric_mod_id: Option<String>,
    pub fabric_mod_version: Option<String>,
    pub fabric_entrypoint_groups: Vec<String>,
    pub fabric_entrypoints: Vec<(String, String)>,
    pub mixin_configs: Vec<String>,
    pub declared_dependency_count: usize,
    pub declared_nested_jar_count: usize,
}

fn read_metadata(reader: impl Read) -> Result<String, String> {
    let mut limited = reader.take(MAX_METADATA_BYTES + 1);
    let mut bytes = Vec::new();
    limited
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read JAR metadata: {error}"))?;
    if bytes.len() as u64 > MAX_METADATA_BYTES {
        return Err("JAR metadata entry exceeds the 1 MiB safety limit".into());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn contains_key_recursive(value: &Value, key: &str) -> bool {
    match value {
        Value::Object(object) => object
            .iter()
            .any(|(candidate, child)| candidate == key || contains_key_recursive(child, key)),
        Value::Array(values) => values.iter().any(|child| contains_key_recursive(child, key)),
        _ => false,
    }
}

fn valid_fabric_id(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_lowercase())
        && id.len() <= 64
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
}

fn manifest_attribute(manifest: &str, attribute: &str) -> bool {
    let target = format!("{}:", attribute.to_ascii_lowercase());
    let mut joined = String::new();
    for line in manifest.lines() {
        if line.starts_with(' ') {
            joined.push_str(line.trim_start());
        } else {
            if joined.to_ascii_lowercase().starts_with(target.as_str()) {
                return true;
            }
            joined.clear();
            joined.push_str(line);
        }
    }
    joined.to_ascii_lowercase().starts_with(target.as_str())
}

fn entrypoint_classes(value: &Value) -> Vec<String> {
    let mut names = Vec::new();
    let mut push = |candidate: Option<&str>| {
        if let Some(name) = candidate.map(str::trim).filter(|name| !name.is_empty()) {
            names.push(name.to_string());
        }
    };
    match value {
        Value::String(name) => push(Some(name)),
        Value::Array(items) => {
            for item in items {
                match item {
                    Value::String(name) => push(Some(name)),
                    Value::Object(object) => push(object.get("value").and_then(Value::as_str)),
                    _ => {}
                }
            }
        }
        Value::Object(object) => push(object.get("value").and_then(Value::as_str)),
        _ => {}
    }
    names
}

fn mixin_config_names(value: &Value) -> Vec<String> {
    let mut names = Vec::new();
    let mut push = |candidate: Option<&str>| {
        if let Some(name) = candidate.map(str::trim).filter(|name| !name.is_empty()) {
            names.push(name.to_string());
        }
    };
    match value {
        Value::String(name) => push(Some(name)),
        Value::Array(items) => {
            for item in items {
                match item {
                    Value::String(name) => push(Some(name)),
                    Value::Object(object) => push(object.get("config").and_then(Value::as_str)),
                    _ => {}
                }
            }
        }
        Value::Object(object) => push(object.get("config").and_then(Value::as_str)),
        _ => {}
    }
    names
}

pub fn audit_jar(path: &Path) -> Result<JarAudit, String> {
    let file = File::open(path).map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    let mut archive = ZipArchive::new(file).map_err(|error| format!("invalid JAR/ZIP: {error}"))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(format!(
            "JAR has {} entries; maximum supported for audit is {MAX_ARCHIVE_ENTRIES}",
            archive.len()
        ));
    }

    let mut audit = JarAudit::default();
    let mut fabric_json: Option<Value> = None;
    let mut quilt_json: Option<Value> = None;
    let mut manifest = String::new();

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("cannot inspect JAR entry {index}: {error}"))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let lower = name.to_ascii_lowercase();

        if lower.ends_with(".class") {
            audit.class_count += 1;
        } else if lower != "meta-inf/manifest.mf" {
            audit.resource_count += 1;
        }

        if lower == "fabric.mod.json" {
            audit.has_fabric_metadata = true;
            let raw = read_metadata(&mut entry)?;
            match serde_json::from_str::<Value>(&raw) {
                Ok(value) => {
                    let id = value.get("id").and_then(Value::as_str).map(str::to_string);
                    let version = value.get("version").and_then(Value::as_str).map(str::to_string);
                    let valid = id.as_deref().is_some_and(valid_fabric_id)
                        && version
                            .as_deref()
                            .is_some_and(|version| !version.trim().is_empty());
                    audit.fabric_metadata_valid = Some(valid);
                    audit.fabric_mod_id = id;
                    audit.fabric_mod_version = version;
                    if let Some(entrypoints) = value.get("entrypoints").and_then(Value::as_object) {
                        audit.fabric_entrypoint_groups = entrypoints.keys().cloned().collect();
                        audit.fabric_entrypoint_groups.sort();
                        for (group, classes) in entrypoints {
                            for class in entrypoint_classes(classes) {
                                audit.fabric_entrypoints.push((group.clone(), class));
                            }
                        }
                        audit.fabric_entrypoints.sort();
                    }
                    for config in mixin_config_names(value.get("mixins").unwrap_or(&Value::Null)) {
                        if !audit.mixin_configs.contains(&config) {
                            audit.mixin_configs.push(config);
                        }
                    }
                    audit.declared_dependency_count = value
                        .get("depends")
                        .and_then(Value::as_object)
                        .map_or(0, |object| object.len());
                    audit.declared_nested_jar_count = value
                        .get("jars")
                        .and_then(Value::as_array)
                        .map_or(0, |array| array.len());
                    audit.has_mixins |= value.get("mixins").is_some();
                    audit.has_nested_jars |= audit.declared_nested_jar_count > 0;
                    audit.has_access_widener |= value.get("accessWidener").is_some();
                    audit.has_language_adapters |= value.get("languageAdapters").is_some();
                    fabric_json = Some(value);
                }
                Err(_) => audit.fabric_metadata_valid = Some(false),
            }
        } else if lower == "quilt.mod.json" {
            audit.has_quilt_metadata = true;
            let raw = read_metadata(&mut entry)?;
            quilt_json = serde_json::from_str::<Value>(&raw).ok();
        } else if lower == "meta-inf/mods.toml" || lower == "mcmod.info" {
            audit.has_forge_metadata = true;
        } else if lower == "meta-inf/neoforge.mods.toml" {
            audit.has_neoforge_metadata = true;
        } else if lower == "meta-inf/manifest.mf" {
            manifest = read_metadata(&mut entry)?;
        }

        if lower.ends_with(".mixins.json") || (lower.contains("mixin") && lower.ends_with(".json")) {
            audit.has_mixins = true;
            if lower.ends_with(".mixins.json") && !audit.mixin_configs.contains(&name) {
                audit.mixin_configs.push(name.clone());
            }
            if let Ok(raw) = read_metadata(&mut entry) {
                if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                    audit.has_mixin_plugin |= value.get("plugin").is_some();
                }
            }
        }
        if lower.ends_with(".accesswidener") || lower.ends_with(".classtweaker") {
            audit.has_access_widener = true;
        }
        if lower == "meta-inf/accesstransformer.cfg" {
            audit.has_access_transformer = true;
        }
        if lower.starts_with("meta-inf/jars/") || lower.ends_with(".jar") {
            audit.has_nested_jars = true;
        }
        if lower.starts_with("meta-inf/services/") {
            audit.has_service_descriptors = true;
        }
    }

    if let Some(value) = &fabric_json {
        audit.has_mixins |= contains_key_recursive(value, "mixins");
        audit.has_nested_jars |= contains_key_recursive(value, "jars");
    }
    if let Some(value) = &quilt_json {
        audit.has_mixins |= contains_key_recursive(value, "mixin") || contains_key_recursive(value, "mixins");
        audit.has_nested_jars |= contains_key_recursive(value, "jars");
        if let Some(entrypoints) = value
            .get("quilt_loader")
            .and_then(|loader| loader.get("entrypoints"))
            .and_then(Value::as_object)
        {
            for (group, classes) in entrypoints {
                for class in entrypoint_classes(classes) {
                    audit.fabric_entrypoints.push((group.clone(), class));
                }
            }
            audit.fabric_entrypoints.sort();
        }
        for config in mixin_config_names(value.get("mixin").unwrap_or(&Value::Null)) {
            if !audit.mixin_configs.contains(&config) {
                audit.mixin_configs.push(config);
            }
        }
        if audit.declared_dependency_count == 0 {
            audit.declared_dependency_count = value
                .get("quilt_loader")
                .and_then(|loader| loader.get("depends"))
                .and_then(Value::as_array)
                .map_or(0, |array| array.len());
        }
    }
    audit.has_manifest_class_path = manifest_attribute(&manifest, "Class-Path");
    audit.has_main_class = manifest_attribute(&manifest, "Main-Class");
    audit.has_legacy_coremod =
        manifest_attribute(&manifest, "FMLCorePlugin") || manifest_attribute(&manifest, "TweakClass");

    Ok(audit)
}

impl JarAudit {
    pub fn injector_warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        if self.class_count == 0 {
            warnings.push("No .class files were found in this JAR.".into());
        }
        warnings.push(
            "Injector mode loads the JAR into the running JVM; it does NOT compile mod classes into \
             machine code."
                .into(),
        );
        if self.resource_count > 0 {
            warnings.push(format!(
                "{} non-class entries (assets, configs, lang files) are served from the archive by the \
                 injector's class loader.",
                self.resource_count
            ));
        }
        if self.fabric_metadata_valid == Some(false) {
            warnings.push("fabric.mod.json is invalid or lacks a valid id/version.".into());
        }
        if self.has_fabric_metadata {
            let details = match (&self.fabric_mod_id, &self.fabric_mod_version) {
                (Some(id), Some(version)) => format!(" (id={id}, version={version})"),
                (Some(id), None) => format!(" (id={id})"),
                _ => String::new(),
            };
            warnings.push(format!(
                "fabric.mod.json is present{details}: supported entrypoints are invoked, but preLaunch \
                 callbacks are skipped because this JVM is already running. Fabric Loader's own \
                 dependency resolution and lifecycle phases are not reproduced{extra}.",
                extra = if self.declared_dependency_count > 0 {
                    format!(
                        " ({} declared dependency assumption(s) are ignored)",
                        self.declared_dependency_count
                    )
                } else {
                    String::new()
                }
            ));
            if !self.fabric_entrypoint_groups.is_empty() {
                warnings.push(format!(
                    "Fabric entrypoint groups declared: {}.",
                    self.fabric_entrypoint_groups.join(", ")
                ));
            }
        }
        if self.has_quilt_metadata {
            warnings.push(
                "quilt.mod.json is present: its entrypoints are read, but Quilt Loader's own lifecycle \
                 and dependency checks are not emulated."
                    .into(),
            );
        }
        if self.has_mixins {
            warnings.push(
                "Mixin configuration detected. Every declared configuration is registered through \
                 Mixins.addConfiguration. The Java agent route also attempts to retransform loaded \
                 targets through the active Mixin transformer; the JVM can reject changes to loaded \
                 class structure, and the native route cannot retransform loaded classes. Restart the \
                 game for reliable compatibility."
                    .into(),
            );
        }
        if self.has_forge_metadata || self.has_neoforge_metadata {
            warnings.push(
                "Forge/NeoForge metadata detected. @Mod constructors are resolved reflectively \
                 (event bus, mod container), but Forge's discovery, dependency resolution and mod-bus \
                 phases do not run again for a mod attached after boot."
                    .into(),
            );
        }
        if self.has_nested_jars {
            warnings.push(format!(
                "{} bundled JAR(s) were declared or detected; they are extracted and added to the \
                 class path before the entrypoints run.",
                self.declared_nested_jar_count.max(1)
            ));
        }
        if self.has_service_descriptors {
            warnings.push(
                "META-INF/services descriptors are reachable through the injector's class loader; \
                 code that asks the *game* loader for services will not see them."
                    .into(),
            );
        }
        if self.has_manifest_class_path {
            warnings.push(
                "Manifest Class-Path entries are present; referenced external JARs are not resolved.".into(),
            );
        }
        if self.has_mixin_plugin {
            warnings.push(
                "A custom Mixin config plugin was detected; running it requires the real Mixin \
                 bootstrap and transformation lifecycle."
                    .into(),
            );
        }
        if self.has_access_widener {
            warnings.push(
                "An access widener/class tweaker was detected; it is not applied to Minecraft's \
                 classes, so code relying on the widened members will fail."
                    .into(),
            );
        }
        if self.has_access_transformer {
            warnings.push(
                "A Forge access transformer was detected; those members must be transformed by \
                 Forge/ModLauncher at startup and are not rewritten by the injector."
                    .into(),
            );
        }
        if self.has_language_adapters {
            warnings.push(
                "Fabric language adapters are declared; only plain Java initializers are invoked, \
                 the adapter registry itself is not consulted."
                    .into(),
            );
        }
        if self.has_legacy_coremod {
            warnings.push(
                "Legacy FML coremod/tweaker manifest attributes were detected; their early-launch \
                 transformation hooks cannot be reproduced by an agent attached after boot."
                    .into(),
            );
        }
        warnings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injector_warnings_identify_known_compatibility_gaps() {
        let warnings = JarAudit {
            has_mixins: true,
            has_nested_jars: true,
            has_fabric_metadata: true,
            ..JarAudit::default()
        }
        .injector_warnings();
        assert!(warnings
            .iter()
            .any(|line| line.contains("does NOT compile mod classes")));
        assert!(warnings
            .iter()
            .any(|line| line.contains("retransform loaded targets")));
        assert!(warnings
            .iter()
            .any(|line| line.contains("bundled JAR(s) were declared or detected")));
    }

    #[test]
    fn fabric_id_validation_rejects_invalid_or_oversized_ids() {
        assert!(valid_fabric_id("example_mod-1"));
        assert!(!valid_fabric_id("ExampleMod"));
        assert!(!valid_fabric_id("1example"));
        assert!(!valid_fabric_id(&"a".repeat(65)));
    }

    #[test]
    fn manifest_attribute_supports_continuation_lines() {
        assert!(manifest_attribute(
            "Manifest-Version: 1.0\nClass-Path: lib/a.jar\n b.jar\n",
            "Class-Path"
        ));
        assert!(!manifest_attribute(
            "Manifest-Version: 1.0\nMain-Class: pkg.Main\n",
            "Class-Path"
        ));
    }

    #[test]
    fn entrypoint_classes_accepts_every_fabric_shape() {
        let plain: Value = serde_json::from_str("\"example.Mod\"").unwrap();
        assert_eq!(entrypoint_classes(&plain), ["example.Mod"]);

        let list: Value = serde_json::from_str("[\"example.A\",{\"value\":\"example.B\"}]").unwrap();
        assert_eq!(entrypoint_classes(&list), ["example.A", "example.B"]);

        let object: Value = serde_json::from_str("{\"value\":\"example.C\"}").unwrap();
        assert_eq!(entrypoint_classes(&object), ["example.C"]);

        let broken: Value = serde_json::from_str("[\"\", 7, null, {\"value\": 3}]").unwrap();
        assert!(entrypoint_classes(&broken).is_empty());
    }

    #[test]
    fn mixin_config_names_accepts_every_fabric_shape() {
        let plain: Value = serde_json::from_str("\"example.mod\"").unwrap();
        assert_eq!(mixin_config_names(&plain), ["example.mod"]);

        let list: Value = serde_json::from_str("[\"a.mixins.json\",{\"config\":\"b.mixins.json\"}]").unwrap();
        assert_eq!(mixin_config_names(&list), ["a.mixins.json", "b.mixins.json"]);

        let object: Value = serde_json::from_str("{\"config\":\"c.mixins.json\"}").unwrap();
        assert_eq!(mixin_config_names(&object), ["c.mixins.json"]);
    }
}
