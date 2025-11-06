#![allow(dead_code)]
use hashrules::HashRules;
use hashtab::{merge_hash_file, serialize_hashtab, HashTab};
use lazy_static::lazy_static;
use lib_util::{include_if_building_hashtab, is_building_hashtab};
use parser::diff::parser::{Change, ObjectToChange};
use processor::find_and_process;
use slots::Slots;
use std::ops::Deref;
use std::os::raw::c_void;
use std::time::Duration;
use std::{
    ffi::{c_char, CStr, CString},
    sync::Mutex,
};
use util::common_util::{load_diff_file, parse_diff};

use crate::error_collector::ErrorCollector;
use crate::parser::diff::parser::ExternalLoader;
use crate::util::common_util::{filter_out_non_matching_versions, tokenize_qml};

mod error_collector;
mod hash;
mod hashrules;
mod hashtab;
mod parser;
mod processor;
mod refcell_translation;
mod slots;

#[path = "util/lib_util.rs"]
mod lib_util;
mod util;

type CExternalLoaderFunc = unsafe extern "C" fn(file_name: *const c_char) -> c_void;

lazy_static! {
    static ref HASHTAB: Mutex<HashTab> = Mutex::new(HashTab::new());
    static ref SLOTS: Mutex<Slots> = Mutex::new(Slots::new());
    static ref CHANGES: Mutex<Vec<Change>> = Mutex::new(Vec::new());
    static ref POST_INIT: Mutex<bool> = Mutex::new(false);
    static ref HASHTAB_RULES: Mutex<Option<HashRules>> = Mutex::new(None);
    static ref CURRENT_VERSION: Mutex<Option<String>> = Mutex::new(None);
    static ref SLOTS_DISABLED: Mutex<bool> = Mutex::new(false);
    static ref EXTERNAL_LOADER: Mutex<Option<CExternalLoaderFunc>> = Mutex::new(None);
    static ref ERROR_COLLECTION_ENABLED: Mutex<bool> = Mutex::new(false);
    static ref ERROR_COLLECTOR: Mutex<ErrorCollector> = Mutex::new(ErrorCollector::new());
}

#[no_mangle]
unsafe extern "C" fn qmldiff_set_external_loader(external_loader: CExternalLoaderFunc) {
    *EXTERNAL_LOADER.lock().unwrap() = Some(external_loader);
}

#[no_mangle]
unsafe extern "C" fn qmldiff_set_version(version: *const c_char) {
    *CURRENT_VERSION.lock().unwrap() = Some(CStr::from_ptr(version).to_str().unwrap().into());
    eprintln!(
        "[qmldiff]: Set system version to {}",
        (*CURRENT_VERSION.lock().unwrap()).as_ref().unwrap()
    );
}

#[no_mangle]
extern "C" fn qmldiff_load_rules(rules: *const c_char) {
    let rules: String = unsafe { CStr::from_ptr(rules) }.to_str().unwrap().into();
    match HashRules::compile(&rules) {
        Ok(rules_ok) => {
            *HASHTAB_RULES.lock().unwrap() = Some(rules_ok);
            eprintln!("[qmldiff]: Configured hashtab rules.");
        }
        Err(error) => {
            eprintln!("[qmldiff]: Error loading rules: {}", error);
        }
    }
}

#[no_mangle]
extern "C" fn qmldiff_add_external_diff(
    change_file_contents: *const c_char,
    file_identifier: *const c_char,
) -> bool {
    if is_building_hashtab() {
        return false;
    }

    let file_identifier: String = unsafe { CStr::from_ptr(file_identifier) }
        .to_str()
        .unwrap()
        .into();

    if *POST_INIT.lock().unwrap() {
        eprintln!(
            "[qmldiff]: Cannot build changes from external {} after init has completed!",
            &file_identifier
        );
    }
    let change_file_contents: String = unsafe { CStr::from_ptr(change_file_contents) }
        .to_str()
        .unwrap()
        .into();

    let error_collection_enabled = *ERROR_COLLECTION_ENABLED.lock().unwrap();
    let mut error_collector_guard;
    let error_collector = if error_collection_enabled {
        error_collector_guard = ERROR_COLLECTOR.lock().unwrap();
        Some(&mut *error_collector_guard)
    } else {
        None
    };

    match parse_diff(
        None,
        change_file_contents,
        &file_identifier,
        &HASHTAB.lock().unwrap(),
        None,
        error_collector,
    ) {
        Err(problem) => {
            eprintln!(
                "[qmldiff]: Failed to load external {}: {:?}",
                &file_identifier, problem
            );
            false
        }
        Ok(mut contents) => {
            filter_out_non_matching_versions(
                &mut contents,
                CURRENT_VERSION.lock().unwrap().clone(),
                &file_identifier,
            );
            SLOTS.lock().unwrap().update_slots(&mut contents);
            eprintln!("[qmldiff]: Loaded external {}", &file_identifier);
            CHANGES.lock().unwrap().extend(contents);
            true
        }
    }
}

fn load_hashtab(root_dir: &str) {
    let mut hashtab = HASHTAB.lock().unwrap();
    if let Err(x) = merge_hash_file(
        std::path::Path::new(&root_dir).join("hashtab"),
        &mut hashtab,
        CURRENT_VERSION.lock().unwrap().clone(),
        None,
    ) {
        eprintln!("[qmldiff]: Failed to load hashtab: {}", x);
    } else {
        println!(
            "[qmldiff]: Hashtab loaded! Cached {} entries",
            hashtab.len()
        );
    }
}

impl ExternalLoader for CExternalLoaderFunc {
    fn load_external(&mut self, file: &str) {
        let c_string = CString::new(file).unwrap();
        unsafe {
            self(c_string.as_ptr());
        }
    }
}

#[no_mangle]
extern "C" fn qmldiff_build_change_files(root_dir: *const c_char) -> i32 {
    if is_building_hashtab() {
        return 0;
    }

    let root_dir: String = unsafe { CStr::from_ptr(root_dir) }.to_str().unwrap().into();

    if *POST_INIT.lock().unwrap() {
        eprintln!(
            "[qmldiff]: Cannot build changes from {} after init has completed!",
            &root_dir
        );
    }
    let mut loaded_files = 0i32;
    let mut all_changes = Vec::new();
    let mut slots = Slots::new();

    eprintln!("[qmldiff]: Iterating over directory {}", &root_dir);

    load_hashtab(&root_dir);

    let error_collection_enabled = *ERROR_COLLECTION_ENABLED.lock().unwrap();
    let mut error_collector_guard;
    let mut error_collector = if error_collection_enabled {
        error_collector_guard = ERROR_COLLECTOR.lock().unwrap();
        Some(&mut *error_collector_guard)
    } else {
        None
    };

    if let Ok(dir) = std::fs::read_dir(&root_dir) {
        let mut files = vec![];
        for file in dir.flatten() {
            let path: String = file.path().to_string_lossy().to_string();
            if path.ends_with(".qmd") {
                files.push(path);
            }
        }
        files.sort();
        for file in &files {
            let fname_start = match file.rfind("/") {
                Some(e) => e + 1,
                None => 0,
            };
            eprintln!("[qmldiff]: Loading file {}", &file[fname_start..]);
            match load_diff_file(
                Some(root_dir.clone()),
                file,
                &HASHTAB.lock().unwrap(),
                EXTERNAL_LOADER
                    .lock()
                    .unwrap()
                    .map(|e| Box::new(e) as Box<dyn ExternalLoader>),
                error_collector.as_deref_mut(),
            ) {
                Err(problem) => {
                    eprintln!("[qmldiff]: Failed to load file {}: {:?}", file, problem)
                }
                Ok(mut contents) => {
                    filter_out_non_matching_versions(
                        &mut contents,
                        CURRENT_VERSION.lock().unwrap().clone(),
                        file,
                    );
                    slots.update_slots(&mut contents);
                    all_changes.extend(contents);
                    loaded_files += 1;
                }
            }
        }
    }

    SLOTS.lock().unwrap().0.extend(slots.0);
    CHANGES.lock().unwrap().extend(all_changes);
    loaded_files
}

#[no_mangle]
/**
 * # Safety
 * no
 */
pub unsafe extern "C" fn qmldiff_is_modified(file_name: *const c_char) -> bool {
    let file_name: String = CStr::from_ptr(file_name).to_str().unwrap().into();

    if is_building_hashtab() {
        return true;
    }

    CHANGES
        .lock()
        .unwrap()
        .iter()
        .any(|e| match &e.destination {
            ObjectToChange::File(z) | ObjectToChange::FileTokenStream(z) => z == &file_name,
            _ => false,
        })
}

#[no_mangle]
/**
 * # Safety
 * no
 */
pub unsafe extern "C" fn qmldiff_disable_slots_while_processing() {
    *(SLOTS_DISABLED.lock().unwrap()) = true;
}

#[no_mangle]
/**
 * # Safety
 * no
 */
pub unsafe extern "C" fn qmldiff_enable_slots_while_processing() {
    *(SLOTS_DISABLED.lock().unwrap()) = false;
}

#[no_mangle]
extern "C" fn qmldiff_enable_error_collection() {
    *(ERROR_COLLECTION_ENABLED.lock().unwrap()) = true;
}

#[no_mangle]
extern "C" fn qmldiff_disable_error_collection() {
    *(ERROR_COLLECTION_ENABLED.lock().unwrap()) = false;
}

#[no_mangle]
extern "C" fn qmldiff_has_collection_errors() -> bool {
    ERROR_COLLECTOR.lock().unwrap().has_errors()
}

#[no_mangle]
extern "C" fn qmldiff_print_and_clear_collection_errors() {
    let mut collector = ERROR_COLLECTOR.lock().unwrap();
    if collector.has_errors() {
        eprintln!("\nHash lookup errors found:");
        collector.print_errors();
        eprintln!("\nTotal errors: {}", collector.error_count());
    }
    *collector = ErrorCollector::new();
}

#[no_mangle]
/**
 * # Safety
 * no
 */
pub unsafe extern "C" fn qmldiff_process_file(
    file_name: *const c_char,
    raw_contents: *const c_char,
    _contents_size: usize,
) -> *const c_char {
    let mut post_init = POST_INIT.lock().unwrap();
    let are_slots_disabled = SLOTS_DISABLED.lock().unwrap().clone();
    if !*post_init && !are_slots_disabled {
        eprintln!(
            "[qmldiff]: Was asked to process the first slot. Sealing slots, entering postinit..."
        );
        *post_init = true;
        SLOTS
            .lock()
            .unwrap()
            .process_slots(&mut CHANGES.lock().unwrap());
    }
    let file_name: String = CStr::from_ptr(file_name).to_str().unwrap().into();

    if include_if_building_hashtab(&file_name, raw_contents) {
        return std::ptr::null();
    }

    let changes = CHANGES.lock().unwrap();
    // It is modified.
    // Build the tree.
    let contents: String = CStr::from_ptr(raw_contents).to_str().unwrap().into();
    let tree = tokenize_qml(contents, &file_name, None, None);
    eprintln!("[qmldiff]: Processing file {}...", &file_name);
    // Fake slots - when slots are disabled, use the always-empty set of slots in their stead.
    let mut fake_slots = Slots::new();
    let slots = if are_slots_disabled {
        &mut fake_slots
    } else {
        &mut SLOTS.lock().unwrap()
    };
    match find_and_process(&file_name, tree, &changes, slots) {
        Ok((emitted, _count)) => {
            let emitted_string = CString::new(emitted).unwrap();
            let ret = emitted_string.as_ptr();
            std::mem::forget(emitted_string);
            ret
        }
        Err(e) => {
            eprintln!("[qmldiff]: Error while processing file tree: {:?}", e);
            std::ptr::null()
        }
    }
}

#[no_mangle]
pub extern "C" fn qmldiff_start_saving_thread() {
    if std::env::var_os("QMLDIFF_HASHTAB_CREATE").is_some() {
        std::thread::spawn(|| {
            eprintln!("[qmldiff]: Hashtab saver started!");
            loop {
                std::thread::sleep(Duration::from_secs(60));
                if let Some(dist_hashmap_path) = std::env::var_os("QMLDIFF_HASHTAB_CREATE") {
                    let hashtab = match HASHTAB.try_lock() {
                        Ok(ht) => ht,
                        Err(_) => {
                            eprintln!("[qmldiff]: Cannot save hashtab right now. Waiting...");
                            continue;
                        }
                    };
                    let mut to_process_rules = hashtab.clone();
                    if let Some(rules) = HASHTAB_RULES.lock().unwrap().deref() {
                        eprintln!("[qmldiff]: Processing rules.");
                        rules.process(&mut to_process_rules);
                    } else {
                        eprintln!("[qmldiff]: No rules to process.");
                    }
                    let string = serialize_hashtab(
                        &to_process_rules,
                        CURRENT_VERSION.lock().unwrap().clone(),
                    );
                    if let Err(e) = std::fs::write(&dist_hashmap_path, string) {
                        eprintln!(
                            "[qmldiff]: Cannot write to {}: {}",
                            &dist_hashmap_path.to_string_lossy(),
                            e
                        );
                    } else {
                        eprintln!(
                            "[qmldiff]: Hashtab saved to {}",
                            &dist_hashmap_path.to_string_lossy()
                        );
                    }
                }
            }
        });
    }
}
