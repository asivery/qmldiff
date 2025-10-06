use std::fmt::Display;

use anyhow::Error;

use crate::parser::common::{CollectionType, StringCharacterTokenizer};

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Keyword {
    Import,
    Property,
    Pragma,
    Required,
    Signal,
    ReadOnly,
    As,
    Function,
    Enum,
    Default,
    Component,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum SymbolicKeyword {
    InstanceOf,
    New,
}

impl TryFrom<&str> for SymbolicKeyword {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "new" => Ok(Self::New),
            "instanceof" => Ok(Self::InstanceOf),
            _ => Err(anyhow::Error::msg(format!(
                "Invalid symbolic-keyword: {}",
                value
            ))),
        }
    }
}

impl From<SymbolicKeyword> for String {
    // These need to be space-terminated due to how parsing works.
    fn from(val: SymbolicKeyword) -> Self {
        String::from(match val {
            SymbolicKeyword::InstanceOf => " instanceof ",
            SymbolicKeyword::New => " new ",
        })
    }
}

impl TryFrom<&str> for Keyword {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "import" => Ok(Self::Import),
            "required" => Ok(Self::Required),
            "pragma" => Ok(Self::Pragma),
            "default" => Ok(Self::Default),
            "property" => Ok(Self::Property),
            "signal" => Ok(Self::Signal),
            "component" => Ok(Self::Component),
            "readonly" => Ok(Self::ReadOnly),
            "as" => Ok(Self::As),
            "function" => Ok(Self::Function),
            "enum" => Ok(Self::Enum),
            _ => Err(anyhow::Error::msg(format!("Invalid keyword: {}", value))),
        }
    }
}

impl From<Keyword> for String {
    fn from(val: Keyword) -> Self {
        String::from(match val {
            Keyword::As => "as",
            Keyword::Enum => "enum",
            Keyword::Import => "import",
            Keyword::Property => "property",
            Keyword::ReadOnly => "readonly",
            Keyword::Signal => "signal",
            Keyword::Pragma => "pragma",
            Keyword::Function => "function",
            Keyword::Default => "default",
            Keyword::Component => "component",
            Keyword::Required => "required",
        })
    }
}

impl Display for TokenType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            TokenType::String(k) => k.clone(),
            TokenType::Identifier(k) => k.clone(),
            TokenType::Keyword(k) => Into::<String>::into(k.clone()),
            TokenType::SymbolicKeyword(k) => Into::<String>::into(k.clone()),
            TokenType::Number(k) => k.to_string(),
            TokenType::Symbol(k) | TokenType::Unknown(k) => String::from(*k),
            TokenType::Whitespace(s) => s.clone(),
            TokenType::NewLine(_) => String::from("\n"),
            TokenType::Comment(comment) => format!("/*{}*/", comment),
            TokenType::EndOfStream => String::from("<<End of Stream>>"),
            TokenType::Extension(ext) => format!("{}", ext),
        })
    }
}

impl Display for QMLExtensionToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HashedIdentifier(hash) => write!(f, "~&{}&~", hash),
            Self::HashedString(quote, hash) => write!(f, "~&{}{}&~", quote, hash),
            Self::Slot(slot) => write!(f, "~{{{}}}~", slot),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum QMLExtensionToken {
    HashedIdentifier(u64),
    HashedString(char, u64),
    Slot(String),
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum TokenType {
    Keyword(Keyword),
    SymbolicKeyword(SymbolicKeyword),
    Identifier(String),
    Number(String), // Numbers are stored as strings, so as to avoid any possible loss of precision when dealing with parsing / reemission.
    String(String),
    Symbol(char),
    Comment(String),
    NewLine(usize),
    Whitespace(String),
    EndOfStream,
    Unknown(char),
    Extension(QMLExtensionToken),
}

pub struct Lexer {
    pub stream: StringCharacterTokenizer,
    line_pos: usize, // Current position within a line [unused.]
}

impl Lexer {
    pub fn new(stream: StringCharacterTokenizer) -> Self {
        Self {
            stream,
            line_pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.stream.input[self.stream.position..].chars().next()
    }

    fn advance(&mut self) -> Option<char> {
        if let Some(c) = self.stream.peek() {
            self.stream.position += c.len_utf8();
            Some(c)
        } else {
            None
        }
    }

    fn collect_while<Z>(&mut self, mut condition: Z) -> String
    where
        Z: FnMut(&Self, char) -> bool,
    {
        let mut result = String::new();
        while let Some(c) = self.stream.peek() {
            if condition(self, c) {
                result.push(c);
                self.stream.advance();
            } else {
                break;
            }
        }
        result
    }
}

impl Lexer {
    pub fn next_token(&mut self) -> Result<TokenType, Error> {
        if let Some(c) = self.stream.peek() {
            match c {
                // Cannot use [[hash]] - it's valid JS
                // For hashes here, ~&hash&~ will be used.
                // For hashed string: ~&[q]hash&~
                // where [q] is one of `, ', "
                // Example: ~&'1234&~
                '~' if self.stream.peek_offset(1) == Some('&') => {
                    // HASH!
                    self.stream.advance();
                    self.stream.advance();
                    // If string_quote is None, that means we're not dealing
                    // with a string, and should proceed normally.
                    let string_quote: Option<char> = match self.stream.peek() {
                        Some('\'') | Some('"') | Some('`') => self.stream.advance(),
                        _ => None,
                    };
                    let hash_str = self.stream.collect_while(|this, c| {
                        (c != '&' && this.peek_offset(1) != Some('~')).into()
                    });
                    self.stream.advance(); // Remove &
                    self.stream.advance(); // Remove ~

                    let hashed_value = hash_str.parse()?;
                    Ok(TokenType::Extension(match string_quote {
                        Some(q) => QMLExtensionToken::HashedString(q, hashed_value),
                        None => QMLExtensionToken::HashedIdentifier(hashed_value),
                    }))
                }
                '~' if self.stream.peek_offset(1) == Some('{') => {
                    // Slot
                    self.stream.advance();
                    self.stream.advance();
                    let slot_name = self.stream.collect_while(|this, c| {
                        (c != '}' && this.peek_offset(1) != Some('~')).into()
                    });
                    self.stream.advance(); // Remove }
                    self.stream.advance(); // Remove ~

                    Ok(TokenType::Extension(QMLExtensionToken::Slot(slot_name)))
                }
                '\n' => {
                    self.stream.advance();
                    self.line_pos += 1;
                    Ok(TokenType::NewLine(self.line_pos))
                }

                c if c.is_whitespace() && c != '\n' => {
                    let str = self.stream.collect_while(|_, c| c.is_whitespace().into());
                    Ok(TokenType::Whitespace(str))
                }

                '/' if self.stream.input[self.stream.position..].starts_with("//") => {
                    self.stream.advance();
                    self.stream.advance();
                    let comment = self.stream.collect_while(|_, c| (c != '\n').into());
                    Ok(TokenType::Comment(comment))
                }

                '/' if self.stream.input[self.stream.position..].starts_with("/*") => {
                    self.stream.advance();
                    self.stream.advance();
                    let comment = self
                        .stream
                        .collect_while(|s, _c| (!s.input[s.position..].starts_with("*/")).into());
                    self.stream.advance(); // Consume '*'
                    self.stream.advance(); // Consume '/'
                    Ok(TokenType::Comment(comment))
                }

                '"' | '\'' | '`' => {
                    let quote = self.stream.advance().unwrap();
                    let mut is_quoted = false;
                    let string = self.stream.collect_while(move |_, c| {
                        if is_quoted {
                            is_quoted = false;
                            return CollectionType::Include;
                        }
                        if c == quote {
                            return CollectionType::Break;
                        }
                        if c == '\\' {
                            is_quoted = true;
                        }
                        CollectionType::Include
                    });

                    self.stream.advance(); // Consume closing quote
                    let s_quote = String::from(quote);
                    Ok(TokenType::String(s_quote.clone() + &string + &s_quote))
                }

                c if c.is_ascii_digit() => {
                    // Allow multiple dots in the number for simplicity's sake
                    let num_str = self
                        .stream
                        .collect_while(|_, c| (c.is_ascii_digit() || c == '.').into());
                    Ok(TokenType::Number(num_str))
                }

                c if c.is_alphabetic() || c == '_' => {
                    let ident = self
                        .stream
                        .collect_while(|_, c| (c.is_alphanumeric() || c == '_').into());
                    if let Ok(keyword) = Keyword::try_from(ident.as_str()) {
                        Ok(TokenType::Keyword(keyword))
                    } else if let Ok(symbolic) = SymbolicKeyword::try_from(ident.as_str()) {
                        Ok(TokenType::SymbolicKeyword(symbolic))
                    } else {
                        Ok(TokenType::Identifier(ident))
                    }
                }

                '{' | '}' | ':' | ';' | '.' | ',' | '(' | ')' | '[' | ']' | '|' | '&' | '%' => {
                    let symbol = self.stream.advance().unwrap();
                    Ok(TokenType::Symbol(symbol))
                }

                _ => {
                    let unknown = self.stream.advance().unwrap();
                    Ok(TokenType::Unknown(unknown))
                }
            }
        } else {
            Ok(TokenType::EndOfStream)
        }
    }
}

impl Iterator for Lexer {
    type Item = TokenType;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.stream.position >= self.stream.input.len() {
                return None;
            }
            if let Ok(token) = self.next_token() {
                return Some(token);
            }
        }
    }
}
