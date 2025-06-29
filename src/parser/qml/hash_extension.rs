use anyhow::{Error, Result};

use crate::{
    hashtab::HashTab,
    parser::common::{ChainIteratorRemapper, IteratorRemapper},
};

use super::lexer::{QMLExtensionToken, TokenType};

pub struct QMLHashRemapper<'a> {
    hashtab: &'a HashTab,
}

impl<'a> QMLHashRemapper<'a> {
    pub fn new(hashtab: &'a HashTab) -> Self {
        Self { hashtab }
    }
}

pub fn qml_hash_remap(hashtab: &HashTab, token: TokenType, source_name: &str) -> Result<TokenType> {
    match token {
        TokenType::Extension(QMLExtensionToken::HashedIdentifier(id)) => {
            if let Some(resolved) = hashtab.get(&id) {
                Ok(TokenType::Identifier(resolved.clone()))
            } else {
                Err(Error::msg(format!(
                    "Cannot resolve hash {} required by {}!",
                    id, source_name
                )))
            }
        }
        TokenType::Extension(QMLExtensionToken::HashedString(q, id)) => {
            if let Some(resolved) = hashtab.get(&id) {
                Ok(TokenType::String(format!("{}{}{}", q, resolved, q)))
            } else {
                Err(Error::msg(format!(
                    "Cannot resolve hash {} required by {}!",
                    id, source_name
                )))
            }
        }
        other => Ok(other),
    }
}

impl IteratorRemapper<TokenType, &str> for QMLHashRemapper<'_> {
    fn remap(&mut self, value: TokenType, source_name: &&str) -> ChainIteratorRemapper<TokenType> {
        match qml_hash_remap(self.hashtab, value, source_name) {
            Ok(e) => ChainIteratorRemapper::Value(e),
            Err(e) => ChainIteratorRemapper::Error(e),
        }
    }
}
