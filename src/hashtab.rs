use anyhow::Result;
use std::{collections::HashMap, fs::File, io::Read, path::Path};

use crate::{
    hash::hash,
    parser::qml::{
        lexer::TokenType,
    },
};

pub type HashTab = HashMap<u64, String>;
pub type InvHashTab = HashMap<String, u64>;

const INTERNAL_HASHTAB_VERSION_ALLOWED_KEY: u64 = 17607111715072197239u64; // Hash of "!*HashTab-Version"

pub struct HashTabFile {
    pub hashtab: HashTab,
    pub version: String,
}

pub fn hash_token_stream(tokens: &Vec<TokenType>, hashtab: &mut HashTab) {
    for token in tokens {
        match token {
            TokenType::Identifier(id) => {
                for id in id.split("."){
                    hashtab.insert(hash(id), id.to_string());
                }
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
                    println!("The file {} is only valid for QML environment version {}. Currently running {}. Loading skipped.", hashtab_file.as_ref().display(), this_file_version, allowed_version);
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
