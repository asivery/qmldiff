use std::{fs::read_to_string, path::Path, sync::Arc};

use anyhow::{Error, Result};

use crate::{
    hashtab::HashTab,
    parser::{
        common::{IteratorPipeline, StringCharacterTokenizer},
        diff::{self, hash_processor::diff_hash_remapper, parser::Change},
        qml::{
            self,
            hash_extension::QMLHashRemapper,
            lexer::{Lexer, TokenType},
            parser::{Object, TreeElement},
            slot_extensions::QMLSlotRemapper,
        },
    },
    slots::Slots,
};

pub fn filter_out_non_matching_versions(
    changes: &mut Vec<Change>,
    ver: Option<String>,
    from: &str,
) {
    // If no env. version provided, allow all.
    if changes.is_empty() {
        return;
    }

    if let Some(ver) = &ver {
        changes.retain(|x| {
            match x.versions_allowed {
                None => true, // If no version whitelist defined, allow all.
                Some(ref vers) => {
                    let retain = vers.contains(ver);
                    if !retain {
                        eprintln!("[qmldiff]: Warning: A change to {:?} (defined by '{}') has been removed! Compatible with versions {:?}, currently running {}", x.destination, from, vers, ver);
                    }

                    retain
                }
            }
        });
        if changes.is_empty() {
            eprintln!("[qmldiff]: Warning: All changes from '{}' have been blocked due to version mismatch!", from);
        }
    }
}

pub fn load_diff_file<P>(
    root_dir: Option<String>,
    file_path: P,
    hashtab: &HashTab,
) -> Result<Vec<Change>>
where
    P: AsRef<Path>,
{
    let contents = read_to_string(&file_path)?;
    parse_diff(
        root_dir,
        contents,
        &file_path.as_ref().to_string_lossy(),
        hashtab,
    )
}

pub fn parse_diff(
    root_dir: Option<String>,
    contents: String,
    diff_name: &str,
    hashtab: &HashTab,
) -> Result<Vec<Change>> {
    let lexer = diff::lexer::Lexer::new(StringCharacterTokenizer::new(contents));
    let tokens: Vec<diff::lexer::TokenType> = lexer
        .map(|e| diff_hash_remapper(hashtab, e, diff_name).unwrap())
        .collect();
    let mut parser = diff::parser::Parser::new(
        Box::new(tokens.into_iter()),
        root_dir,
        Arc::from(diff_name.to_string()),
        Some(hashtab),
    );

    parser.parse(None)
}

pub fn parse_qml(
    raw_qml: String,
    qml_name: &str,
    hashtab: Option<&HashTab>,
    slots: Option<&mut Slots>,
) -> Result<Vec<TreeElement>> {
    let mut iterator = IteratorPipeline::new(
        Box::from(Lexer::new(StringCharacterTokenizer::new(raw_qml))),
        qml_name,
    );
    let mut hash_mapper;
    if hashtab.is_some() {
        hash_mapper = QMLHashRemapper::new(hashtab.unwrap());
        iterator.add_remapper(&mut hash_mapper);
    }

    let mut slot_mapper;
    if let Some(slots) = slots {
        slot_mapper = QMLSlotRemapper::new(slots);
        iterator.add_remapper(&mut slot_mapper);
    }

    let mut parser: qml::parser::Parser =
        qml::parser::Parser::new(Box::new(iterator.collect::<Vec<_>>().into_iter()));
    parser.parse()
}

pub fn parse_qml_from_chain(tokens: Vec<TokenType>) -> Result<Vec<TreeElement>> {
    let mut parser = qml::parser::Parser::new(Box::new(tokens.into_iter()));
    parser.parse()
}

pub fn parse_qml_into_simple_object(tokens: Vec<TokenType>) -> Result<Object> {
    let data = parse_qml_from_chain(tokens)?.pop().unwrap();
    match data {
        TreeElement::Object(o) => Ok(o),
        _ => Err(Error::msg("Invalid token stream for object recreation!")),
    }
}
