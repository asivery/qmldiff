use std::{cell::RefCell, fs::read_to_string, path::Path, rc::Rc, sync::Arc};

use anyhow::{Error, Result};

use crate::{
    error_collector::ErrorCollector,
    hashtab::HashTab,
    parser::{
        common::{IteratorPipeline, StringCharacterTokenizer},
        diff::{
            self,
            hash_processor::diff_hash_remapper,
            parser::{Change, ExternalLoader},
        },
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

pub fn add_error_source_if_needed<T>(result: Result<T>, source: &str) -> Result<T> {
    match result {
        Ok(e) => Ok(e),
        Err(error) => Err(Error::msg(format!(
            "(On behalf of '{}'): {:?}",
            source, error
        ))),
    }
}

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
    external_loader: Option<Box<dyn ExternalLoader>>,
    error_collector: Option<&mut ErrorCollector>,
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
        external_loader,
        error_collector,
    )
}

pub fn parse_diff(
    root_dir: Option<String>,
    contents: String,
    diff_name: &str,
    hashtab: &HashTab,
    external_loader: Option<Box<dyn ExternalLoader>>,
    error_collector: Option<&mut ErrorCollector>,
) -> Result<Vec<Change>> {
    let lexer = diff::lexer::Lexer::new(StringCharacterTokenizer::new(contents));
    let tokens: Vec<diff::lexer::TokenType> = if let Some(collector) = error_collector {
        lexer
            .map(|e| diff_hash_remapper(hashtab, e, diff_name, &mut Some(collector)).unwrap())
            .collect()
    } else {
        lexer
            .map(|e| diff_hash_remapper(hashtab, e, diff_name, &mut None).unwrap())
            .collect()
    };
    let mut parser = diff::parser::Parser::new(
        Box::new(tokens.into_iter()),
        root_dir,
        Arc::from(diff_name.to_string()),
        Some(hashtab),
        external_loader.map(|e| Rc::new(RefCell::new(e))),
    );

    parser.parse(None)
}

pub fn tokenize_qml(
    raw_qml: String,
    qml_name: &str,
    hashtab: Option<&HashTab>,
    slots: Option<&mut Slots>,
) -> Vec<TokenType> {
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

    iterator.collect::<Vec<_>>()
}

pub fn parse_qml(
    raw_qml: String,
    qml_name: &str,
    hashtab: Option<&HashTab>,
    slots: Option<&mut Slots>,
) -> Result<Vec<TreeElement>> {
    let mut parser: qml::parser::Parser = qml::parser::Parser::new(Box::new(
        tokenize_qml(raw_qml, qml_name, hashtab, slots).into_iter(),
    ));
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
