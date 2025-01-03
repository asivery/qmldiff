use std::{
    ffi::{c_char, CStr},
    fs::create_dir_all,
    path::Path,
};

use crate::{hash::hash, hashtab::update_hashtab_from_tree, util::common_util::parse_qml, HASHTAB};

pub fn is_building_hashtab() -> bool {
    std::env::var_os("QMLDIFF_HASHTAB_CREATE").is_some()
}

pub fn is_extracting_tree() -> bool {
    std::env::var_os("QMLDIFF_EXTRACT_TREE").is_some()
}

/**
 * # Safety
 * no
 */
pub unsafe fn include_if_building_hashtab(file_name: &str, raw_contents: *const c_char) -> bool {
    if std::env::var_os("QMLDIFF_HASHTAB_CREATE").is_some() {
        eprintln!("[qmldiff]: Hashing: {}", file_name);
        let mut hashtab = HASHTAB.lock().unwrap();
        for entry in file_name.split('/') {
            if !entry.is_empty() {
                let hashed = hash(entry);
                hashtab.entry(hashed).or_insert_with(|| entry.to_string());
            }
        }
        hashtab.insert(hash(file_name), String::from(file_name));
        if file_name.to_lowercase().ends_with(".qml") {
            let contents: String = CStr::from_ptr(raw_contents).to_str().unwrap().into();
            let tree = parse_qml(contents, None, None);
            if let Ok(tree) = tree {
                update_hashtab_from_tree(&tree, &mut hashtab);
            } else {
                eprintln!(
                    "[qmldiff]: Failed to build hashtab from file {}.",
                    &file_name
                );
            }
        }

        true
    } else {
        false
    }
}

pub fn extract_tree_node(tree_path: &str, data: &[u8]) -> bool {
    if let Some(root_path) = std::env::var_os("QMLDIFF_EXTRACT_TREE") {
        let root_path = Path::new(&root_path);
        let final_path = root_path.join(tree_path.strip_prefix('/').unwrap_or(tree_path));
        create_dir_all(final_path.parent().unwrap()).unwrap();
        if let Err(x) = std::fs::write(&final_path, data) {
            eprintln!(
                "[qmldiff]: Failed to write resource {} to {} - {}",
                tree_path,
                final_path.to_string_lossy(),
                x
            );
        } else {
            eprintln!(
                "[qmldiff]: Resource {} extracted",
                final_path.to_string_lossy()
            );
        }

        true
    } else {
        false
    }
}
