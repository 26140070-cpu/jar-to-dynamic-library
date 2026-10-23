use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::classfile::ClassFile;

pub struct JarClasses {
    pub classes: Vec<ClassFile>,
    pub skipped: Vec<String>,
}

pub fn read_jar(path: &Path) -> Result<JarClasses, String> {
    let file = File::open(path).map_err(|e| format!("can't open {}: {e}", path.display()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("{} is not a valid jar/zip: {e}", path.display()))?;

    let mut by_name: HashMap<String, usize> = HashMap::new();
    let mut classes: Vec<ClassFile> = Vec::new();
    let mut skipped = Vec::new();

    for i in 0..archive.len() {
        let mut entry = archive
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

        let mut data = Vec::with_capacity(entry.size() as usize);
        entry
            .read_to_end(&mut data)
            .map_err(|e| format!("can't read {entry_name}: {e}"))?;
        let class = ClassFile::parse(data).map_err(|e| format!("{entry_name}: {e}"))?;

        match by_name.get(&class.name) {
            Some(&existing) => classes[existing] = class,
            None => {
                by_name.insert(class.name.clone(), classes.len());
                classes.push(class);
            }
        }
    }

    assign_priorities(&mut classes, &by_name);

    classes.sort_by(|a, b| b.priority.cmp(&a.priority).then_with(|| a.name.cmp(&b.name)));

    Ok(JarClasses { classes, skipped })
}

fn assign_priorities(classes: &mut [ClassFile], by_name: &HashMap<String, usize>) {
    let parents: Vec<Vec<usize>> = classes
        .iter()
        .map(|c| {
            c.super_class
                .iter()
                .chain(c.interfaces.iter())
                .filter_map(|name| by_name.get(name).copied())
                .collect()
        })
        .collect();

    for _ in 0..=classes.len() {
        let mut changed = false;
        for child in 0..classes.len() {
            let wanted = classes[child].priority + 1;
            for &parent in &parents[child] {
                if classes[parent].priority < wanted {
                    classes[parent].priority = wanted;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}
