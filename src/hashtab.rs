use anyhow::Result;
use std::{collections::HashMap, fs::File, io::Read, path::Path};

use crate::{
    hash::hash,
    parser::qml::{
        lexer::TokenType,
        parser::{AssignmentChildValue, Object, ObjectChild, QMLTree, TreeElement},
    },
};

pub type HashTab = HashMap<u64, String>;
pub type InvHashTab = HashMap<String, u64>;

const INTERNAL_HASHTAB_VERSION_ALLOWED_KEY: u64 = 17607111715072197239u64; // Hash of "!*HashTab-Version"

pub struct HashTabFile {
    pub hashtab: HashTab,
    pub version: String,
}

fn hash_token_stream(hashtab: &mut HashTab, tokens: &Vec<TokenType>) {
    for token in tokens {
        match token {
            TokenType::Identifier(id) => {
                hashtab.insert(hash(id), id.clone());
            }
            TokenType::String(str) => {
                // Remove the quotes around the string:
                let contents = &str[1..str.len() - 1];
                hashtab.insert(hash(contents), contents.to_string());
            }
            _ => {}
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
            ObjectChild::ObjectProperty(prop) => {
                include!(&prop.name);
                update_hashtab(hashtab, &prop.default_value);
            }
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

pub fn merge_hash_file<P>(
    hashtab_file: P,
    destination: &mut HashTab,
    current_version: Option<String>,
    mut inv_destination: Option<&mut InvHashTab>,
) -> Result<()>
where
    P: AsRef<Path>,
{
    let mut data_file = File::open(&hashtab_file)?;
    loop {
        let mut hash_value = [0u8; 8];
        let mut str_len = [0u8; 4];
        if data_file.read_exact(&mut hash_value).is_err() {
            break;
        }
        data_file.read_exact(&mut str_len)?;
        let str_len_int = u32::from_be_bytes(str_len) as usize;
        let hash_value_int = u64::from_be_bytes(hash_value);
        let mut str_content = vec![0u8; str_len_int];
        data_file.read_exact(&mut str_content)?;
        if hash_value_int == INTERNAL_HASHTAB_VERSION_ALLOWED_KEY {
            let this_file_version = String::from(String::from_utf8_lossy(&str_content));
            if let Some(ref allowed_version) = current_version {
                if this_file_version != *allowed_version {
                    println!("The file {} is only valid for QML environment version {}. Currently running {}. Loading skipped.", hashtab_file.as_ref().display(), allowed_version, allowed_version);
                    return Ok(());
                }
            }
        }
        if hash_value_int != 0 {
            let str: String = String::from_utf8_lossy(&str_content).into();
            if let Some(ref mut rev) = inv_destination {
                rev.insert(str.clone(), hash_value_int);
            }
            destination.insert(hash_value_int, str);
        }
    }
    Ok(())
}

pub fn serialize_hashtab(hashtab: &HashTab, current_version: Option<String>) -> Vec<u8> {
    let mut output = Vec::new();
    {
        let magic_string = "Hashtab file for QMLDIFF. Do not edit.".bytes();
        output.extend(0u64.to_be_bytes());
        output.extend((magic_string.len() as u32).to_be_bytes());
        output.extend(magic_string);
    }
    macro_rules! append_hash {
        ($id: expr, $val: expr) => {
            output.extend($id.to_be_bytes());
            let bytes = $val.bytes();
            output.extend((bytes.len() as u32).to_be_bytes());
            output.extend(bytes);
        };
    }
    if let Some(current_version) = current_version {
        append_hash!(INTERNAL_HASHTAB_VERSION_ALLOWED_KEY, current_version);
    }
    for (hash, str) in hashtab {
        append_hash!(hash, str);
    }
    output
}
