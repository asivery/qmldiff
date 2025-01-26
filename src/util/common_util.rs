use std::{fs::read_to_string, path::Path};

use anyhow::Result;

use crate::{
    hashtab::HashTab,
    parser::{
        common::IteratorPipeline,
        diff::{self, hash_processor::diff_hash_remapper, parser::Change},
        qml::{
            self,
            hash_extension::QMLHashRemapper,
            lexer::{Lexer, TokenType},
            parser::TreeElement,
            slot_extensions::QMLSlotRemapper,
        },
    },
    slots::Slots,
};

pub fn load_diff_file<P>(
    root_dir: Option<String>,
    file_path: P,
    hashtab: &HashTab,
) -> Result<Vec<Change>>
where
    P: AsRef<Path>,
{
    let contents = read_to_string(file_path)?;
    parse_diff(root_dir, contents, hashtab)
}

pub fn parse_diff(
    root_dir: Option<String>,
    contents: String,
    hashtab: &HashTab,
) -> Result<Vec<Change>> {
    let lexer = diff::lexer::Lexer::new(contents);
    let tokens: Vec<diff::lexer::TokenType> = lexer
        .map(|e| diff_hash_remapper(hashtab, e).unwrap())
        .collect();
    let mut parser = diff::parser::Parser::new(Box::new(tokens.into_iter()), root_dir, hashtab);

    parser.parse()
}

pub fn parse_qml(
    raw_qml: String,
    hashtab: Option<&HashTab>,
    slots: Option<&mut Slots>,
) -> Result<Vec<TreeElement>> {
    let mut iterator = IteratorPipeline::new(Box::from(Lexer::new(raw_qml)));
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
