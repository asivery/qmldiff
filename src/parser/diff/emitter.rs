use crate::parser::qml::{self, emitter::flatten_lines};

use super::lexer::{HashedValue, TokenType};

pub fn token_stream_into_vec(
    mut stream: impl Iterator<Item = TokenType>,
) -> Vec<super::lexer::TokenType> {
    let mut vec = vec![];
    loop {
        match stream.next() {
            Some(e) => vec.push(e),
            None => return vec,
        }
    }
}

pub fn emit_token_stream(stream: Vec<super::lexer::TokenType>) -> String {
    let mut output_string = String::new();
    for token in stream {
        let token_string = match token {
            TokenType::Comment(cmnt) => format!("; {}", cmnt),
            TokenType::EndOfStream => String::default(),
            TokenType::Identifier(id) => id,
            TokenType::Keyword(kw) => kw.to_string(),
            TokenType::NewLine(_) => String::from("\n"),
            TokenType::QMLCode {
                qml_code,
                stream_character,
            } => {
                let emitted = flatten_lines(&qml::emitter::emit_token_stream(&qml_code, 0));
                if let Some(token) = stream_character {
                    format!("STREAM {} {} {}", &token, emitted, &token)
                } else {
                    format!("{{{}}}", emitted)
                }
            }
            TokenType::String(str) => {
                if str.starts_with('\'') || str.starts_with('"') {
                    str
                } else {
                    format!("`{}`", str)
                }
            }
            TokenType::Symbol(chr) => String::from(chr),
            TokenType::Unknown(chr) => String::from(chr),
            TokenType::Whitespace(ws) => ws,
            TokenType::HashedValue(HashedValue::HashedString(q, hash)) => {
                format!("[[{}{}]]", q, hash)
            }
            TokenType::HashedValue(HashedValue::HashedIdentifier(hash)) => format!("[[{}]]", hash),
        };
        output_string += &token_string;
    }

    output_string
}
