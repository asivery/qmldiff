use std::sync::Arc;

use anyhow::{Error, Result};

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

pub fn diff_hash_remapper(
    hashtab: &HashTab,
    value: TokenType,
    source_name: &str,
) -> Result<TokenType> {
    match value {
        TokenType::HashedValue(HashedValue::HashedIdentifier(id)) => Ok(TokenType::Identifier(
            hashtab
                .get(&id)
                .ok_or(Error::msg(format!(
                    "Couldn't resolve the hashed identifier {} required by {}",
                    id, source_name
                )))?
                .clone(),
        )),
        TokenType::HashedValue(HashedValue::HashedString(q, id)) => {
            let unwrapped = hashtab.get(&id).ok_or(Error::msg(format!(
                "Couldn't resolve the hashed identifier {} required by {}",
                id, source_name
            )))?;
            Ok(TokenType::String(if q != '`' {
                format!("{}{}{}", q, unwrapped, q)
            } else {
                unwrapped.clone()
            }))
        }
        TokenType::QMLCode {
            qml_code,
            stream_character: is_stream,
        } => {
            Ok(TokenType::QMLCode {
                qml_code: qml_code
                    .into_iter()
                    .map(|e| match qml_hash_remap(hashtab, e, source_name) {
                        Ok(v) => v,
                        Err(e) => {
                            panic!("{:?}", e); // temporary solution.
                        }
                    })
                    .collect(),
                stream_character: is_stream,
            })
        }
        other => Ok(other),
    }
}

impl IteratorRemapper<TokenType, Arc<String>> for DiffHashRemapper<'_> {
    fn remap(
        &mut self,
        value: TokenType,
        souce_name: &Arc<String>,
    ) -> ChainIteratorRemapper<TokenType> {
        match diff_hash_remapper(self.hashtab, value, souce_name) {
            Ok(e) => ChainIteratorRemapper::Value(e),
            Err(e) => ChainIteratorRemapper::Error(e),
        }
    }
}
