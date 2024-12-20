#![allow(dead_code)]
use hashtab::{hashtab_to_toml_string, merge_toml_file, HashTab};
use lazy_static::lazy_static;
use lib_util::{
    extract_tree_node, include_if_building_hashtab, is_building_hashtab, is_extracting_tree,
};
use parser::diff::parser::{Change, ObjectToChange};
use parser::qml;
use parser::qml::emitter::emit_string;
use parser::qml::lexer::QMLDiffExtensions;
use processor::find_and_process;
use refcell_translation::{translate_from_root, untranslate_from_root};
use slots::Slots;
use std::time::Duration;
use std::{
    ffi::{c_char, CStr, CString},
    sync::Mutex,
};
use util::common_util::{load_diff_file, parse_diff};

mod hash;
mod hashtab;
mod parser;
mod processor;
mod refcell_translation;
mod slots;

#[path = "util/lib_util.rs"]
mod lib_util;
mod util;

lazy_static! {
    static ref HASHTAB: Mutex<HashTab> = Mutex::new(HashTab::new());
    static ref SLOTS: Mutex<Slots> = Mutex::new(Slots::new());
    static ref CHANGES: Mutex<Vec<Change>> = Mutex::new(Vec::new());
    static ref POST_INIT: Mutex<bool> = Mutex::new(false);
}

#[no_mangle]
extern "C" fn qmldiff_add_external_diff(
    change_file_contents: *const c_char,
    file_identifier: *const c_char,
) -> bool {
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
    match parse_diff(None, change_file_contents, &HASHTAB.lock().unwrap()) {
        Err(problem) => {
            eprintln!(
                "[qmldiff]: Failed to load external {}: {:?}",
                &file_identifier, problem
            );
            false
        }
        Ok(mut contents) => {
            SLOTS.lock().unwrap().update_slots(&mut contents);
            eprintln!("[qmldiff]: Loaded external {}", &file_identifier);
            CHANGES.lock().unwrap().extend(contents);
            true
        }
    }
}

fn load_hashtab(root_dir: &str) {
    let mut hashtab = HASHTAB.lock().unwrap();
    if let Err(x) = merge_toml_file(
        std::path::Path::new(&root_dir).join("hashtab"),
        &mut hashtab,
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

#[no_mangle]
extern "C" fn qmldiff_build_change_files(root_dir: *const c_char) -> i32 {
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

    if let Ok(dir) = std::fs::read_dir(&root_dir) {
        for file in dir.flatten() {
            let name: String = file.file_name().into_string().unwrap();
            if name.ends_with(".qmd") {
                eprintln!("[qmldiff]: Loading file {}", &name);
                match load_diff_file(
                    Some(root_dir.clone()),
                    file.path(),
                    &HASHTAB.lock().unwrap(),
                ) {
                    Err(problem) => {
                        eprintln!("[qmldiff]: Failed to load file {}: {:?}", &name, problem)
                    }
                    Ok(mut contents) => {
                        slots.update_slots(&mut contents);
                        all_changes.extend(contents);
                        loaded_files += 1;
                    }
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
        return file_name.to_lowercase().ends_with(".qml");
    }

    if is_extracting_tree() {
        return true;
    }

    CHANGES
        .lock()
        .unwrap()
        .iter()
        .any(|e| e.destination == ObjectToChange::File(file_name.clone()))
}

#[no_mangle]
/**
 * # Safety
 * no
 */
pub unsafe extern "C" fn qmldiff_process_file(
    file_name: *const c_char,
    raw_contents: *const c_char,
    contents_size: usize,
) -> *const c_char {
    let mut post_init = POST_INIT.lock().unwrap();
    if !*post_init {
        *post_init = true;
        SLOTS
            .lock()
            .unwrap()
            .process_slots(&mut CHANGES.lock().unwrap());
        eprintln!(
            "[qmldiff]: Was asked to process the first slot. Sealing slots, entering postinit..."
        );
    }
    let file_name: String = CStr::from_ptr(file_name).to_str().unwrap().into();

    if include_if_building_hashtab(&file_name, raw_contents) {
        return std::ptr::null();
    }

    if extract_tree_node(
        &file_name,
        std::slice::from_raw_parts(raw_contents as *const u8, contents_size),
    ) {
        return std::ptr::null();
    }

    {
        let changes = CHANGES.lock().unwrap();
        // It is modified.
        // Build the tree.
        let contents: String = CStr::from_ptr(raw_contents).to_str().unwrap().into();
        let lexer = crate::qml::lexer::Lexer::new(contents, None, None);
        let tokens: Vec<crate::qml::lexer::TokenType> = lexer.collect();
        let mut parser = crate::qml::parser::Parser::new(Box::new(tokens.into_iter()));
        eprintln!("[qmldiff]: Processing file {}...", &file_name);
        match parser.parse() {
            Ok(tree) => {
                let mut tree = translate_from_root(tree);
                let slots = &SLOTS.lock().unwrap();
                let hashtab = &HASHTAB.lock().unwrap();
                let extensions = QMLDiffExtensions::new(Some(hashtab), Some(slots));
                match find_and_process(&file_name, &mut tree, &changes, extensions, &mut Vec::new())
                {
                    Ok(()) => {
                        let raw_tree = untranslate_from_root(tree);
                        let emitted_string = CString::new(emit_string(&raw_tree).as_str()).unwrap();
                        let ret = emitted_string.as_ptr();
                        std::mem::forget(emitted_string);
                        return ret;
                    }
                    Err(e) => eprintln!("[qmldiff]: Error while processing file tree: {:?}", e),
                }
            }
            Err(e) => eprintln!("[qmldiff]: Error while parsing file tree: {:?}", e),
        }
    }

    std::ptr::null()
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
                    let string = hashtab_to_toml_string(&hashtab);
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
