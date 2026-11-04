use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::classfile::ClassFile;

const MAX_ARCHIVE_ENTRIES: usize = 250_000;
const MAX_CLASSES: usize = 100_000;
const MAX_CLASS_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TOTAL_CLASS_BYTES: u64 = 512 * 1024 * 1024;

pub struct JarClasses {
    pub classes: Vec<ClassFile>,
    pub skipped: Vec<String>,
}

pub fn read_jar(path: &Path) -> Result<JarClasses, String> {
    let file = File::open(path).map_err(|e| format!("can't open {}: {e}", path.display()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("{} is not a valid jar/zip: {e}", path.display()))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(format!(
            "archive has {} entries; maximum allowed is {MAX_ARCHIVE_ENTRIES}",
            archive.len()
        ));
    }

    let mut by_name: HashMap<String, usize> = HashMap::new();
    let mut classes: Vec<ClassFile> = Vec::new();
    let mut skipped = Vec::new();
    let mut total_class_bytes: u64 = 0;

    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| format!("can't read entry {i}: {e}"))?;
        let entry_name = entry.name().to_string();
        if !entry_name.ends_with(".class") {
            continue;
        }
        if entry_name == "module-info.class" || entry_name.ends_with("/module-info.class") {
            skipped.push(format!("{entry_name} (module descriptor)"));
            continue;
        }
        if entry_name.starts_with("META-INF/versions/") {
            skipped.push(format!("{entry_name} (multi-release variant)"));
            continue;
        }

        let declared_size = entry.size();
        if declared_size > MAX_CLASS_BYTES {
            return Err(format!(
                "class {entry_name} is {declared_size} bytes; maximum allowed is {MAX_CLASS_BYTES}"
            ));
        }
        if classes.len() >= MAX_CLASSES {
            return Err(format!("archive contains more than {MAX_CLASSES} class files"));
        }
        let remaining = MAX_TOTAL_CLASS_BYTES.saturating_sub(total_class_bytes);
        if declared_size > remaining {
            return Err(format!(
                "class data exceeds the total {MAX_TOTAL_CLASS_BYTES}-byte safety limit"
            ));
        }
        let mut data = Vec::with_capacity(declared_size as usize);
        let mut limited = entry.take(MAX_CLASS_BYTES + 1);
        limited
            .read_to_end(&mut data)
            .map_err(|e| format!("can't read {entry_name}: {e}"))?;
        if data.len() as u64 > MAX_CLASS_BYTES {
            return Err(format!("class {entry_name} exceeds the decompressed size limit"));
        }
        total_class_bytes = total_class_bytes.saturating_add(data.len() as u64);
        if total_class_bytes > MAX_TOTAL_CLASS_BYTES {
            return Err(format!(
                "class data exceeds the total {MAX_TOTAL_CLASS_BYTES}-byte safety limit"
            ));
        }
        let class = ClassFile::parse(data).map_err(|e| format!("{entry_name}: {e}"))?;

        match by_name.get(&class.name) {
            Some(&existing) => classes[existing] = class,
            None => {
                by_name.insert(class.name.clone(), classes.len());
                classes.push(class);
            }
        }
    }

    if classes.is_empty() {
        return Err(
            "JAR contains no loadable .class files; refusing to build an injector that would do nothing"
                .into(),
        );
    }

    assign_priorities(&mut classes, &by_name)?;

    classes.sort_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.name.cmp(&b.name)));

    Ok(JarClasses { classes, skipped })
}

fn assign_priorities(classes: &mut [ClassFile], by_name: &HashMap<String, usize>) -> Result<(), String> {
    let parents: Vec<Vec<usize>> = classes
        .iter()
        .map(|class| {
            class
                .super_class
                .iter()
                .chain(class.interfaces.iter())
                .filter_map(|name| by_name.get(name).copied())
                .collect()
        })
        .collect();

    let mut children = vec![Vec::<usize>::new(); classes.len()];
    let mut remaining_parents = vec![0usize; classes.len()];
    for (child, parent_indices) in parents.iter().enumerate() {
        remaining_parents[child] = parent_indices.len();
        for &parent in parent_indices {
            children[parent].push(child);
        }
    }

    let mut ready = VecDeque::new();
    for (index, count) in remaining_parents.iter().enumerate() {
        if *count == 0 {
            ready.push_back(index);
        }
    }

    let mut processed = 0usize;
    while let Some(parent) = ready.pop_front() {
        processed += 1;
        let child_priority = classes[parent].priority.saturating_add(1);
        for &child in &children[parent] {
            classes[child].priority = classes[child].priority.max(child_priority);
            remaining_parents[child] -= 1;
            if remaining_parents[child] == 0 {
                ready.push_back(child);
            }
        }
    }

    if processed != classes.len() {
        return Err("class hierarchy contains a cycle; refusing to build an invalid injector".into());
    }
    Ok(())
}
