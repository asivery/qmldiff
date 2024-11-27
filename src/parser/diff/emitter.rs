use super::lexer::TokenType;

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
            TokenType::QMLCode(qml) => format!("{{{}}}", qml),
            TokenType::String(str) => str,
            TokenType::Symbol(chr) => String::from(chr),
            TokenType::Unknown(chr) => String::from(chr),
            TokenType::Whitespace(ws) => ws,
        };
        output_string += &token_string;
    }

    output_string
}
