use anyhow::Result;

use crate::{
    hashtab::HashTab,
    parser::{
        common::{ChainIteratorRemapper, IteratorRemapper},
        qml::hash_extension::qml_hash_remap,
    },
};

use super::lexer::{HashedValue, TokenType};

pub struct DiffHashRemapper<'a> {
    hashtab: &'a HashTab,
}

pub fn diff_hash_remapper(hashtab: &HashTab, value: TokenType) -> Result<TokenType> {
    match value {
        TokenType::HashedValue(HashedValue::HashedIdentifier(id)) => {
            Ok(TokenType::Identifier(hashtab.get(&id).unwrap().clone()))
        }
        TokenType::HashedValue(HashedValue::HashedString(q, id)) => {
            Ok(TokenType::String(if q != '`' {
                format!("{}{}{}", q, hashtab.get(&id).unwrap(), q)
            } else {
                hashtab.get(&id).unwrap().clone()
            }))
        }
        TokenType::QMLCode(code) => {
            Ok(TokenType::QMLCode(
                code.into_iter()
                    .map(|e| match qml_hash_remap(hashtab, e) {
                        Ok(v) => v,
                        Err(e) => {
                            panic!("{:?}", e); // temporary solution.
                        }
                    })
                    .collect(),
            ))
        }
        other => Ok(other),
    }
}

impl IteratorRemapper<TokenType> for DiffHashRemapper<'_> {
    fn remap(&mut self, value: TokenType) -> ChainIteratorRemapper<TokenType> {
        match diff_hash_remapper(self.hashtab, value) {
            Ok(e) => ChainIteratorRemapper::Value(e),
            Err(e) => ChainIteratorRemapper::Error(e),
        }
    }
}
