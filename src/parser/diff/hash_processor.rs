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

fn resolve_hashed_ids(hashtab: &HashTab, source_name: &str, id: &Vec<u64>) -> Result<String> {
    let mut out_id = String::new();
    for id in id {
        if out_id != "" { out_id += "." }
        out_id += 
        hashtab
            .get(&id)
            .ok_or(Error::msg(format!(
                "Couldn't resolve the hashed identifier {} required by {}",
                id, source_name
            )))?;
    }

    Ok(out_id)
}


pub fn diff_hash_remapper(
    hashtab: &HashTab,
    value: TokenType,
    source_name: &str,
) -> Result<TokenType> {
    match value {
        TokenType::HashedValue(HashedValue::HashedIdentifier(id)) => Ok(TokenType::Identifier(resolve_hashed_ids(hashtab, source_name, &id)?)),
        TokenType::HashedValue(HashedValue::HashedString(q, id)) => {
            let unwrapped = resolve_hashed_ids(hashtab, source_name, &id)?;
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
