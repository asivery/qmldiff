use std::{fmt::Display, mem::take};

use anyhow::{bail, Error};

use crate::parser::{
    common::{CollectionType, StringCharacterTokenizer},
    qml,
};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Keyword {
    Affect,
    Traverse,
    Insert,
    Assert,
    Locate,
    Replace,
    Template,
    Remove,
    Import,
    Multiple,
    Replicate,
    Rename,
    End,
    Slot,
    Load,
    External,
    Version,

    With,
    To,
    All,
    After,
    Before,

    // Stream editing keywords:
    Until,
    Argument,
    At,
    Located,
    Rebuild,
    Redefine,

    // Conditionals
    If,
    Else,
    Elif,

    And,
    Or,
    Not,

    Exists,
    Matches,
    Diff,

    Info,
    Error_,
}

impl Display for Keyword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&String::from(match self {
            Self::Affect => "AFFECT",
            Self::After => "AFTER",
            Self::All => "ALL",
            Self::Assert => "ASSERT",
            Self::Before => "BEFORE",
            Self::Rename => "RENAME",
            Self::Load => "LOAD",
            Self::External => "EXTERNAL",
            Self::End => "END",
            Self::Import => "IMPORT",
            Self::Insert => "INSERT",
            Self::Locate => "LOCATE",
            Self::Multiple => "MULTIPLE",
            Self::Remove => "REMOVE",
            Self::Replace => "REPLACE",
            Self::Replicate => "REPLICATE",
            Self::Slot => "SLOT",
            Self::Template => "TEMPLATE",
            Self::Traverse => "TRAVERSE",
            Self::With => "WITH",
            Self::To => "TO",
            Self::Version => "VERSION",

            Self::Until => "UNTIL",
            Self::Argument => "ARGUMENT",
            Self::At => "AT",
            Self::Located => "LOCATED",
            Self::Rebuild => "REBUILD",
            Self::Redefine => "REDEFINE",

            Self::If => "IF",
            Self::Else => "ELSE",
            Self::Elif => "ELIF",

            Self::And => "AND",
            Self::Or => "OR",
            Self::Not => "NOT",

            Self::Exists => "EXISTS",
            Self::Matches => "MATCHES",
            Self::Diff => "DIFF",

            Self::Info => "INFO",
            Self::Error_ => "ERROR",
        }))
    }
}

impl TryFrom<&str> for Keyword {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "AFFECT" => Ok(Self::Affect),
            "TRAVERSE" => Ok(Self::Traverse),
            "ASSERT" => Ok(Self::Assert),
            "INSERT" => Ok(Self::Insert),
            "SLOT" => Ok(Self::Slot),
            "TEMPLATE" => Ok(Self::Template),
            "LOCATE" => Ok(Self::Locate),
            "IMPORT" => Ok(Self::Import),
            "RENAME" => Ok(Self::Rename),
            "LOAD" => Ok(Self::Load),
            "EXTERNAL" => Ok(Self::External),
            "ALL" => Ok(Self::All),
            "BEFORE" => Ok(Self::Before),
            "AFTER" => Ok(Self::After),
            "REMOVE" => Ok(Self::Remove),
            "REPLICATE" => Ok(Self::Replicate),
            "MULTIPLE" => Ok(Self::Multiple),
            "REPLACE" => Ok(Self::Replace),
            "WITH" => Ok(Self::With),
            "TO" => Ok(Self::To),
            "END" => Ok(Self::End),
            "VERSION" => Ok(Self::Version),

            "UNTIL" => Ok(Self::Until),
            "ARGUMENT" => Ok(Self::Argument),
            "AT" => Ok(Self::At),
            "LOCATED" => Ok(Self::Located),
            "REBUILD" => Ok(Self::Rebuild),
            "REDEFINE" => Ok(Self::Redefine),

            "IF" => Ok(Self::If),
            "ELSE" => Ok(Self::Else),
            "ELIF" => Ok(Self::Elif),

            "AND" => Ok(Self::And),
            "OR" => Ok(Self::Or),
            "NOT" => Ok(Self::Not),

            "EXISTS" => Ok(Self::Exists),
            "MATCHES" => Ok(Self::Matches),
            "DIFF" => Ok(Self::Diff),

            "INFO" => Ok(Self::Info),
            "ERROR" => Ok(Self::Error_),

            _ => Err(anyhow::Error::msg(format!("Invalid keyword: {}", value))),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum HashedValue {
    HashedString(char, Vec<u64>),
    HashedIdentifier(Vec<u64>),
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum TokenType {
    Keyword(Keyword),
    Identifier(String),
    String(String),
    Symbol(char),
    Comment(String),
    NewLine(usize),
    Whitespace(String),
    EndOfStream,
    QMLCode {
        qml_code: Vec<qml::lexer::TokenType>,
        stream_character: Option<qml::lexer::TokenType>,
    },
    Unknown(char),
    HashedValue(HashedValue),
}

pub struct Lexer {
    pub stream: StringCharacterTokenizer,
    pub line_pos: usize, // Current position within a line [unused.]
}

impl Lexer {
    pub fn new(input: StringCharacterTokenizer) -> Self {
        Self {
            stream: input,
            line_pos: 0,
        }
    }
    pub fn next_token(&mut self) -> Result<TokenType, Error> {
        if let Some(c) = self.stream.peek() {
            match c {
                '\n' => {
                    self.stream.advance();
                    self.line_pos += 1;
                    Ok(TokenType::NewLine(self.line_pos))
                }

                c if c.is_whitespace() && c != '\n' => {
                    let string = self.stream.collect_while(|_, c| c.is_whitespace().into());
                    Ok(TokenType::Whitespace(string))
                }

                ';' => {
                    self.stream.advance();
                    let comment = self.stream.collect_while(|_, c| (c != '\n').into());
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
                            return CollectionType::Drop;
                        }
                        CollectionType::Include
                    });

                    self.stream.advance(); // Consume closing quote
                    Ok(TokenType::String(if quote == '`' {
                        string
                    } else {
                        format!("{}{}{}", quote, string, quote)
                    }))
                }

                '[' if self.stream.input[self.stream.position+1..].starts_with('[') => {
                    // [[HASH]]
                    self.stream.advance();
                    self.stream.advance();
                    // String hashing:
                    let string_quote: Option<char> = match self.stream.peek() {
                        Some('\'') | Some('"') | Some('`') => self.stream.advance(),
                        _ => None
                    };
                    let hash = self.stream.collect_while(|_, c| (c.is_ascii_digit() || c == '.').into());
                    let a = self.stream.peek();
                    self.stream.advance();
                    let b = self.stream.peek();
                    match (a, b) {
                        (Some(']'), Some(']')) => {}
                        _ => return Err(Error::msg("Invalid hash!")),
                    }
                    self.stream.advance();
                    let hash = hash.split('.').map(|x| x.parse::<u64>().unwrap()).collect();
                    Ok(TokenType::HashedValue(match string_quote {
                        None => HashedValue::HashedIdentifier(hash),
                        Some(q) => HashedValue::HashedString(q, hash)
                    }))
                }

                c if c.is_alphabetic() || c.is_ascii_digit() || c == '_' || c == '-' || c == '/' /*|| c == '.' */ => {
                    let ident =
                        self.stream.collect_while(|_, c| (c.is_alphanumeric() || c.is_ascii_digit() || c == '-' || c == '_' || c == '.' || c == '/').into());
                    if let Ok(keyword) = Keyword::try_from(ident.as_str()) {
                        Ok(TokenType::Keyword(keyword))
                    } else if ident == "STREAM" {
                        self.stream.collect_while(|_, c| c.is_whitespace().into());
                        // Start processing as a QML token stream, until met with the same token as the one that follows
                        // this keyword
                        let mut qml_lexer = qml::lexer::Lexer::new(take(&mut self.stream));
                        let mut qml_code = Vec::new();
                        let initial_token = qml_lexer.next_token()?;
                        loop {
                            let token = qml_lexer.next_token()?;
                            if token == initial_token {
                                break;
                            }
                            qml_code.push(token);
                        }
                        self.stream = take(&mut qml_lexer.stream);
                        Ok(TokenType::QMLCode {
                            qml_code,
                            stream_character: Some(initial_token),
                        })
                    } else {
                        Ok(TokenType::Identifier(ident))
                    }
                }

                '{' => {
                    // This is the start of QML code.
                    self.stream.advance();
                    let mut qml_lexer = qml::lexer::Lexer::new(take(&mut self.stream));
                    let mut qml_code = Vec::new();
                    let mut depth = 1u32;
                    loop {
                        let token = qml_lexer.next_token()?;
                        match token {
                            qml::lexer::TokenType::Symbol('{') => depth += 1,
                            qml::lexer::TokenType::Symbol('}') => depth -= 1,
                            qml::lexer::TokenType::EndOfStream => bail!("Unexpected End-Of-Stream reached!"),
                            _ => {}
                        }
                        if depth == 0 {
                            break;
                        } else {
                            qml_code.push(token);
                        }
                    }
                    self.stream = take(&mut qml_lexer.stream);
                    Ok(TokenType::QMLCode {
                        qml_code,
                        stream_character: None,
                    })
                }

                //       Child-of    Prop.EQ        ID      p.named | Others
                // Prop.v      Contains    Traversal     Name       |      Wildcard   Order of parsing of logic expr.
                '[' | ']' | '>' | '~' | '=' | '/' | '#' | ':' | '!' | '.' | '?'      | '(' | ')' => {
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
            match self.next_token() {
                Ok(token) => return Some(token),
                Err(e) => {
                    // TODO: handle this
                    panic!("Error while reading token: {e:?}");
                }
            }
        }
    }
}
