use std::{fs::read_to_string, path::Path};

use anyhow::Result;

use crate::{
    hashtab::HashTab,
    parser::diff::{self, parser::Change},
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
    let lexer = diff::lexer::Lexer::new(contents, hashtab);
    let tokens: Vec<diff::lexer::TokenType> = lexer.collect();
    let mut parser = diff::parser::Parser::new(Box::new(tokens.into_iter()), root_dir, hashtab);

    parser.parse()
}
