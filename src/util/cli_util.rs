use anyhow::{Error, Result};
use std::{
    collections::HashSet,
    fs::{create_dir_all, read_dir, read_to_string, write},
    path::Path,
};

use crate::{
    hash::hash,
    hashtab::{hash_token_stream, HashTab, InvHashTab},
    parser::{
        common::StringCharacterTokenizer,
        diff::{
            self,
            emitter::emit_token_stream,
            hash_processor::diff_hash_remapper,
            lexer::{HashedValue, TokenType},
            parser::{Change, ExternalLoader, ObjectToChange},
        },
        qml::{self, hash_extension::qml_hash_remap},
    },
    processor::find_and_process,
    slots::Slots,
    util::common_util::{
        filter_out_non_matching_versions, load_diff_file, tokenize_qml,
    },
};

fn build_recursive_hashmap(directory: &String, dir_relative_name: &String, tab: &mut HashTab) {
    println!("Recursing {} (qrc:{}/)", directory, dir_relative_name);
    for file in read_dir(directory).unwrap().flatten() {
        let t = file.file_type().unwrap();
        let name = file.file_name().into_string().unwrap();
        let mut relative_name = dir_relative_name.clone();
        relative_name.push('/');
        relative_name.push_str(&name);
        tab.insert(hash(&name), name.clone());
        let hash = hash(&relative_name);
        tab.insert(hash, relative_name);
        if t.is_file() {
            if name.ends_with(".qml") {
                println!("Hashing {}", file.path().to_str().unwrap());
                let tree = tokenize_qml(
                    std::fs::read_to_string(file.path()).unwrap(),
                    &name,
                    None,
                    None,
                );
                hash_token_stream(&tree, tab);
            }
        } else {
            build_recursive_hashmap(
                &(directory.clone() + "/" + &name),
                &(dir_relative_name.clone() + "/" + &name),
                tab,
            );
        }
    }
}

pub fn start_hashmap_build(root: &String) -> HashTab {
    let mut hashtab = HashTab::new();
    build_recursive_hashmap(root, &String::new(), &mut hashtab);

    hashtab
}

pub fn process_diff_tree(
    diff_files: &Vec<String>,
    hashtab: &HashTab,
    inv_hashtab: &InvHashTab,
    into_hash: bool,
) {
    for file in diff_files {
        let path = std::path::Path::new(&file);
        if path.is_file() {
            process_single_diff(file, hashtab, inv_hashtab, into_hash);
        }
    }
}

fn process_single_diff(
    diff_file_path: &String,
    hashtab: &HashTab,
    inv_hashtab: &InvHashTab,
    into_hash: bool,
) {
    let string_contents = match std::fs::read_to_string(diff_file_path) {
        Err(error) => {
            println!("Error while reading file {}: {:?}", diff_file_path, error);
            return;
        }
        Ok(e) => e,
    };
    let mut token_stream: Vec<TokenType> =
        diff::lexer::Lexer::new(StringCharacterTokenizer::new(string_contents))
            .map(|e| diff_hash_remapper(hashtab, e, diff_file_path).unwrap())
            .collect();
    if into_hash {
        token_stream = token_stream
            .into_iter()
            .map(|e| match e {
                TokenType::Identifier(id) => {
                    let splits_values = id.split('.').map(|e| inv_hashtab.get(e).cloned().unwrap_or(0)).collect::<Vec<_>>();
                    if splits_values.iter().all(|e| e != &0) {
                        TokenType::HashedValue(diff::lexer::HashedValue::HashedIdentifier(splits_values))
                    } else {
                        if let Some(id) = inv_hashtab.get(&id) {
                            TokenType::HashedValue(HashedValue::HashedIdentifier(vec![*id]))
                        } else {
                            TokenType::Identifier(id)
                        }
                    }
                }
                TokenType::String(string) => {
                    if string.len() > 2 && inv_hashtab.contains_key(&string[1..string.len() - 1]) {
                        // Hashing force-converts into Identifiers.
                        // This is an intermediary form, so even if it
                        // goes against the spec, it's not an issue
                        TokenType::Identifier(format!(
                            "[[{}{}]]",
                            string.chars().next().unwrap(),
                            inv_hashtab.get(&string[1..string.len() - 1]).unwrap()
                        ))
                    } else {
                        // Do not translate
                        TokenType::String(string)
                    }
                }
                TokenType::QMLCode {
                    qml_code: qml,
                    stream_character,
                } => {
                    // Parse into tokens
                    let tokens = qml
                        .into_iter()
                        .map(|token| match token {
                            qml::lexer::TokenType::Identifier(id) => {
                                if inv_hashtab.contains_key(&id) {
                                    qml::lexer::TokenType::Extension(
                                        qml::lexer::QMLExtensionToken::HashedIdentifier(
                                            *inv_hashtab.get(&id).unwrap(),
                                        ),
                                    )
                                } else {
                                    qml::lexer::TokenType::Identifier(id)
                                }
                            }
                            qml::lexer::TokenType::String(string) => {
                                if string.len() > 2
                                    && inv_hashtab.contains_key(&string[1..string.len() - 1])
                                {
                                    // See comment above
                                    qml::lexer::TokenType::Extension(
                                        qml::lexer::QMLExtensionToken::HashedString(
                                            string.chars().next().unwrap(),
                                            *inv_hashtab.get(&string[1..string.len() - 1]).unwrap(),
                                        ),
                                    )
                                } else {
                                    // Do not translate
                                    qml::lexer::TokenType::String(string)
                                }
                            }
                            tok => tok,
                        })
                        .collect();
                    TokenType::QMLCode {
                        qml_code: tokens,
                        stream_character,
                    }
                }
                e => e,
            })
            .collect();
    } else {
        // Unhash the QMLCode
        let mut whitespace_indent = 0;
        token_stream = token_stream
            .into_iter()
            .map(|e| {
                if let TokenType::Whitespace(ref space) = e {
                    whitespace_indent = space.len() / 4;
                }
                match e {
                    TokenType::QMLCode {
                        qml_code,
                        stream_character,
                    } => TokenType::QMLCode {
                        qml_code: qml_code
                            .into_iter()
                            .map(|e| qml_hash_remap(hashtab, e, diff_file_path).unwrap())
                            .collect::<Vec<_>>(),
                        stream_character,
                    },
                    e => e,
                }
            })
            .collect();
    }
    let emitted = emit_token_stream(token_stream);
    if let Err(error) = std::fs::write(diff_file_path, emitted) {
        println!("Error while writing file {}: {:?}", diff_file_path, error);
    }
}

struct LoggingExternalLoader {}
impl ExternalLoader for LoggingExternalLoader {
    fn load_external(&mut self, file: &str) {
        println!("QMD tried to load external {file}")
    }
}

pub fn build_change_structures(
    files: &Vec<String>,
    hashtab: &HashTab,
    slots: &mut Slots,
    version: Option<String>,
) -> Result<Vec<Change>> {
    let mut all_changes = Vec::new();
    for path_str in files {
        let path = Path::new(path_str);
        if !path.exists() {
            return Err(Error::msg(format!("File {} does not exist!", path_str)));
        }
        if path.is_file() {
            let root_dir = String::from(path.parent().unwrap().to_string_lossy());
            println!("Reading diff {}...", path.to_string_lossy());
            let mut this_diff = load_diff_file(
                Some(root_dir),
                path,
                hashtab,
                Some(Box::new(LoggingExternalLoader {})),
            )?;
            filter_out_non_matching_versions(
                &mut this_diff,
                version.clone(),
                &path.to_string_lossy(),
            );
            slots.update_slots(&mut this_diff);
            all_changes.extend(this_diff);
        } else if path.is_dir() {
            for sub_file in (read_dir(path)?).flatten() {
                let sub_file_path = sub_file.path();
                if !sub_file_path.is_file() {
                    continue;
                }
                println!("Reading diff {}...", sub_file_path.to_string_lossy());
                let mut this_diff = load_diff_file(
                    Some(path_str.clone()),
                    &sub_file_path,
                    hashtab,
                    Some(Box::new(LoggingExternalLoader {})),
                )?;
                filter_out_non_matching_versions(
                    &mut this_diff,
                    version.clone(),
                    &sub_file_path.to_string_lossy(),
                );
                slots.update_slots(&mut this_diff);
                all_changes.extend(this_diff);
            }
        }
    }

    Ok(all_changes)
}

pub fn apply_changes(
    qml_root_path: &str,
    qml_destination_path: &str,
    flatten: bool,
    slots: &mut Slots,
    changes: &Vec<Change>,
) -> Result<()> {
    let file_set = changes
        .iter()
        .filter_map(|e| match &e.destination {
            ObjectToChange::File(f) | ObjectToChange::FileTokenStream(f) => Some(f.clone()),
            _ => None,
        })
        .collect::<HashSet<String>>();

    let mut file_iterator = 0u32;
    let absolute_root = Path::new(qml_destination_path);
    let source_root = Path::new(qml_root_path);

    for file_to_edit in file_set.iter() {
        // Open the file.
        let file_contents = match read_to_string(
            source_root.join(file_to_edit.strip_prefix('/').unwrap_or(file_to_edit)),
        ) {
            Ok(contents) => contents,
            Err(error) => {
                return Err(Error::msg(format!(
                    "Error: {} - file {} does not exist",
                    error, file_to_edit
                )))
            }
        };
        let tree = tokenize_qml(file_contents, &file_to_edit, None, None);
        let (emitted, count) = find_and_process(file_to_edit, tree, changes, slots)?;

        // Rewrite the file in destination
        let destination_path = if flatten {
            let next = format!(
                "{}_{}",
                file_iterator,
                Path::new(&file_to_edit)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
            );
            file_iterator += 1;
            absolute_root.join(next)
        } else {
            let next = Path::new(&file_to_edit);
            absolute_root.join(next.strip_prefix("/").unwrap_or(next))
        };
        create_dir_all(destination_path.parent().unwrap())?;
        write(&destination_path, emitted)?;
        println!(
            "Written file {} - {} diff(s) applied.",
            destination_path.to_string_lossy(),
            count
        );
    }

    Ok(())
}
