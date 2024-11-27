use std::ffi::{c_char, CStr};

use crate::{
    hash::hash,
    hashtab::update_hashtab_from_tree,
    HASHTAB,
};

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
        let contents: String = CStr::from_ptr(raw_contents).to_str().unwrap().into();
        let lexer = crate::parser::qml::lexer::Lexer::new(contents, None, None);
        let tokens: Vec<crate::parser::qml::lexer::TokenType> = lexer.collect();

        let mut parser =
            crate::parser::qml::parser::Parser::new(Box::new(Box::new(tokens.into_iter())));
        if let Ok(tree) = parser.parse() {
            let mut hashtab = HASHTAB.lock().unwrap();
            hashtab.insert(hash(file_name), String::from(file_name));
            update_hashtab_from_tree(&tree, &mut hashtab);
        } else {
            eprintln!(
                "[qmldiff]: Failed to build hashtab from file {}.",
                &file_name
            );
        }

        true
    } else {
        false
    }
}

