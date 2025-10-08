use std::ffi::{c_char, CStr};

use crate::{hash::hash, hashtab::hash_token_stream, util::common_util::tokenize_qml, HASHTAB};

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
            let qml = tokenize_qml(contents, file_name, None, None);
            hash_token_stream(&qml, &mut hashtab);
        }

        true
    } else {
        false
    }
}
