use anyhow::{Error, Result};

use crate::{
    error_collector::{ErrorCollector, HashLookupError},
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

pub fn qml_hash_remap(
    hashtab: &HashTab,
    token: TokenType,
    source_name: &str,
    error_collector: Option<&mut ErrorCollector>,
) -> Result<TokenType> {
    match token {
        TokenType::Extension(QMLExtensionToken::HashedIdentifier(id)) => {
            if let Some(resolved) = hashtab.get(&id) {
                Ok(TokenType::Identifier(resolved.clone()))
            } else {
                if let Some(collector) = error_collector {
                    collector.add_error(HashLookupError::new(
                        id,
                        source_name.to_string(),
                    ));
                    // Return a placeholder to continue processing
                    Ok(TokenType::Identifier(format!("__UNRESOLVED_HASH_{}", id)))
                } else {
                    Err(Error::msg(format!(
                        "Cannot resolve hash {} required by {}!",
                        id, source_name
                    )))
                }
            }
        }
        TokenType::Extension(QMLExtensionToken::HashedString(q, id)) => {
            if let Some(resolved) = hashtab.get(&id) {
                Ok(TokenType::String(format!("{}{}{}", q, resolved, q)))
            } else {
                if let Some(collector) = error_collector {
                    collector.add_error(HashLookupError::new(
                        id,
                        source_name.to_string(),
                    ));
                    // Return a placeholder to continue processing
                    Ok(TokenType::String(format!("{}__UNRESOLVED_HASH_{}{}", q, id, q)))
                } else {
                    Err(Error::msg(format!(
                        "Cannot resolve hash {} required by {}!",
                        id, source_name
                    )))
                }
            }
        }
        other => Ok(other),
    }
}

impl IteratorRemapper<TokenType, &str> for QMLHashRemapper<'_> {
    fn remap(&mut self, value: TokenType, source_name: &&str) -> ChainIteratorRemapper<TokenType> {
        match qml_hash_remap(self.hashtab, value, source_name, None) {
            Ok(e) => ChainIteratorRemapper::Value(e),
            Err(e) => ChainIteratorRemapper::Error(e),
        }
    }
}
