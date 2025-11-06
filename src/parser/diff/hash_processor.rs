use std::sync::Arc;

use anyhow::{Error, Result};

use crate::{
    error_collector::{ErrorCollector, HashLookupError},
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

fn resolve_hashed_ids(
    hashtab: &HashTab,
    source_name: &str,
    id: &Vec<u64>,
    error_collector: &mut Option<&mut ErrorCollector>,
) -> Result<String> {
    let mut out_id = String::new();

    for id in id {
        if out_id != "" { out_id += "." }

        if let Some(resolved) = hashtab.get(&id) {
            out_id += resolved;
        } else {
            if let Some(ref mut collector) = error_collector {
                collector.add_error(HashLookupError::new(
                    *id,
                    source_name.to_string(),
                ));
                out_id += &format!("__UNRESOLVED_HASH_{}", id);
            } else {
                return Err(Error::msg(format!(
                    "Couldn't resolve the hashed identifier {} required by {}",
                    id, source_name
                )));
            }
        }
    }

    Ok(out_id)
}


pub fn diff_hash_remapper(
    hashtab: &HashTab,
    value: TokenType,
    source_name: &str,
    error_collector: &mut Option<&mut ErrorCollector>,
) -> Result<TokenType> {
    match value {
        TokenType::HashedValue(HashedValue::HashedIdentifier(id)) => {
            Ok(TokenType::Identifier(resolve_hashed_ids(
                hashtab,
                source_name,
                &id,
                error_collector,
            )?))
        }
        TokenType::HashedValue(HashedValue::HashedString(q, id)) => {
            let unwrapped = resolve_hashed_ids(hashtab, source_name, &id, error_collector)?;
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
                    .map(|e| match qml_hash_remap(hashtab, e, source_name, error_collector.as_deref_mut()) {
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
        match diff_hash_remapper(self.hashtab, value, souce_name, &mut None) {
            Ok(e) => ChainIteratorRemapper::Value(e),
            Err(e) => ChainIteratorRemapper::Error(e),
        }
    }
}
