use std::ffi::{c_char, CStr};

use crate::{hash::hash, hashtab::update_hashtab_from_tree, util::common_util::parse_qml, HASHTAB};

pub fn is_building_hashtab() -> bool {
    std::env::var_os("QMLDIFF_HASHTAB_CREATE").is_some()
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
