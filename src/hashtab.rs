use anyhow::{Error, Result};
use std::{collections::HashMap, path::Path};

use crate::{
    hash::hash,
    parser::qml::{
        lexer::TokenType,
        parser::{AssignmentChildValue, Object, ObjectChild, QMLTree, TreeElement},
    },
};

pub type HashTab = HashMap<u64, String>;
pub type InvHashTab = HashMap<String, u64>;

fn hash_token_stream(hashtab: &mut HashTab, tokens: &Vec<TokenType>) {
    for token in tokens {
        if let TokenType::Identifier(id) = token {
            hashtab.insert(hash(id), id.clone());
        }
    }
}

fn update_hashtab(hashtab: &mut HashTab, qml_obj: &Object) {
    macro_rules! include {
        ($value: expr) => {
            hashtab.insert(hash($value), $value.clone());
        };
    }
    include!(&qml_obj.name);
    for child in &qml_obj.children {
        let child_name = child.get_name();
        if let Some(child_name) = child_name {
            include!(child_name);
        }
        match child {
            ObjectChild::Object(obj) => update_hashtab(hashtab, obj),
            ObjectChild::Component(obj) => {
                update_hashtab(hashtab, &obj.object);
                include!(&obj.name);
            }
            ObjectChild::Assignment(asi) => {
                match &asi.value {
                    AssignmentChildValue::Object(obj) => update_hashtab(hashtab, obj),
                    AssignmentChildValue::Other(obj) => hash_token_stream(hashtab, obj),
                };
                include!(&asi.name);
            }
            ObjectChild::Signal(sig) => {
                include!(&sig.name);
            }
            ObjectChild::ObjectAssignment(asi) => {
                update_hashtab(hashtab, &asi.value);
                include!(&asi.name);
            }
            ObjectChild::Enum(enu) => {
                include!(&enu.name);
            }
            ObjectChild::Function(func) => {
                include!(&func.name);
            }
            ObjectChild::Property(prop) => {
                include!(&prop.name);
                match &prop.default_value {
                    Some(AssignmentChildValue::Object(obj)) => update_hashtab(hashtab, obj),
                    Some(AssignmentChildValue::Other(obj)) => hash_token_stream(hashtab, obj),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

pub fn update_hashtab_from_tree(qml: &QMLTree, hashtab: &mut HashTab) {
    for root_child in qml {
        if let TreeElement::Object(obj) = root_child {
            update_hashtab(hashtab, obj)
        }
    }
}

pub fn merge_toml_file<P>(
    toml_file: P,
    destination: &mut HashTab,
    mut inv_destination: Option<&mut InvHashTab>,
) -> Result<()>
where
    P: AsRef<Path>,
{
    let hashtab_raw: HashMap<String, String> =
        toml::from_str(&std::fs::read_to_string(toml_file)?)?;

    for (key, val) in hashtab_raw {
        match key.parse::<u64>() {
            Ok(u64key) => {
                if let Some(ref mut inv) = inv_destination {
                    inv.insert(val.clone(), u64key);
                }
                destination.insert(u64key, val);
            }
            Err(e) => {
                return Err(Error::from(e));
            }
        }
    }

    Ok(())
}

pub fn hashtab_to_toml_string(hashtab: &HashTab) -> String {
    toml::to_string(
        &hashtab
            .iter()
            .map(|(a, b)| (a.to_string(), b.clone()))
            .collect::<HashMap<String, String>>(),
    )
    .unwrap()
}
