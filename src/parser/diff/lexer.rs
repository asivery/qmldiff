use std::fmt::Display;

use anyhow::Error;

use crate::hashtab::HashTab;

#[derive(Debug, PartialEq, Eq, Clone)]
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
    Rename,
    End,
    Slot,
    Load,

    With,
    To,
    All,
    After,
    Before,
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
            Self::End => "END",
            Self::Import => "IMPORT",
            Self::Insert => "INSERT",
            Self::Locate => "LOCATE",
            Self::Multiple => "MULTIPLE",
            Self::Remove => "REMOVE",
            Self::Replace => "REPLACE",
            Self::Slot => "SLOT",
            Self::Template => "TEMPLATE",
            Self::Traverse => "TRAVERSE",
            Self::With => "WITH",
            Self::To => "TO",
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
            "ALL" => Ok(Self::All),
            "BEFORE" => Ok(Self::Before),
            "AFTER" => Ok(Self::After),
            "REMOVE" => Ok(Self::Remove),
            "MULTIPLE" => Ok(Self::Multiple),
            "REPLACE" => Ok(Self::Replace),
            "WITH" => Ok(Self::With),
            "TO" => Ok(Self::To),
            "END" => Ok(Self::End),
            _ => Err(anyhow::Error::msg(format!("Invalid keyword: {}", value))),
        }
    }
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
    QMLCode(String),
    Unknown(char),
}

pub struct Lexer<'a> {
    hashtab: &'a HashTab,
    input: String,
    position: usize, // current position in the input
    line_pos: usize,
}

enum CollectionType {
    Break,
    Include,
    Drop,
}

impl From<bool> for CollectionType {
    fn from(value: bool) -> Self {
        if value {
            CollectionType::Include
        } else {
            CollectionType::Break
        }
    }
}

impl<'a> Lexer<'a> {
    pub fn new(input: String, hashtab: &'a HashTab) -> Self {
        Lexer {
            input,
            position: 0,
            line_pos: 0,
            hashtab,
        }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.position..].chars().next()
    }

    fn advance(&mut self) -> Option<char> {
        if let Some(c) = self.peek() {
            self.position += c.len_utf8();
            Some(c)
        } else {
            None
        }
    }

    fn collect_while<F>(&mut self, mut condition: F) -> String
    where
        F: FnMut(&Self, char) -> CollectionType,
    {
        let mut result = String::new();
        while let Some(c) = self.peek() {
            match condition(self, c) {
                CollectionType::Break => break,
                CollectionType::Drop => {
                    self.advance();
                }
                CollectionType::Include => {
                    result.push(c);
                    self.advance();
                }
            }
        }
        result
    }
}

impl<'a> Lexer<'a> {
    pub fn next_token(&mut self) -> Result<TokenType, Error> {
        if let Some(c) = self.peek() {
            match c {
                '\n' => {
                    self.advance();
                    self.line_pos += 1;
                    Ok(TokenType::NewLine(self.line_pos))
                }

                c if c.is_whitespace() && c != '\n' => {
                    let string = self.collect_while(|_, c| c.is_whitespace().into());
                    Ok(TokenType::Whitespace(string))
                }

                ';' => {
                    self.advance();
                    let comment = self.collect_while(|_, c| (c != '\n').into());
                    Ok(TokenType::Comment(comment))
                }

                '"' | '\'' | '`' => {
                    let quote = self.advance().unwrap();
                    let mut is_quoted = false;
                    let string = self.collect_while(move |_, c| {
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

                    self.advance(); // Consume closing quote
                    Ok(TokenType::String(if quote == '`' {
                        string
                    } else {
                        format!("{}{}{}", quote, string, quote)
                    }))
                }

                '[' if self.input[self.position+1..].starts_with('[') => {
                    // [[HASH]]
                    self.advance();
                    self.advance();
                    // String hashing:
                    let string_quote: Option<char> = match self.peek() {
                        Some('\'') | Some('"') | Some('`') => self.advance(),
                        _ => None
                    };
                    let hash = self.collect_while(|_, c| c.is_ascii_digit().into());
                    let a = self.peek();
                    self.advance();
                    let b = self.peek();
                    match (a, b) {
                        (Some(']'), Some(']')) => {}
                        _ => return Err(Error::msg("Invalid hash!")),
                    }
                    self.advance();
                    let hash = hash.parse::<u64>().unwrap();
                    let resolved_string = self.hashtab.get(&hash);
                    match resolved_string {
                        Some(string) => {
                            if let Some(string_quote) = string_quote {
                                Ok(TokenType::String(format!("{}{}{}", string_quote, string, string_quote)))
                            } else {
                                Ok(TokenType::Identifier(string.clone()))
                            }
                        },
                        None => Err(Error::msg(format!("Cannot resolve hash {}", hash))),
                    }
                }

                c if c.is_alphabetic() || c.is_ascii_digit() || c == '_' || c == '/' /*|| c == '.' */ => {
                    let ident =
                        self.collect_while(|_, c| (c.is_alphanumeric() || c == '_' || c == '.' || c == '/').into());
                    if let Ok(keyword) = Keyword::try_from(ident.as_str()) {
                        Ok(TokenType::Keyword(keyword))
                    } else {
                        Ok(TokenType::Identifier(ident))
                    }
                }

                '{' => {
                    // This is the start of QML code.
                    let mut depth = 1u32;
                    self.advance();
                    let contents = self.collect_while(move |_, chr| {
                        match chr {
                            '{' => depth += 1,
                            '}' => depth -= 1,
                            _ => {}
                        }
                        (depth != 0).into()
                    });
                    self.advance(); // past the final } character
                    Ok(TokenType::QMLCode(contents))
                }

                //       Child-of    Prop.EQ        ID      p.named | Others
                // Prop.v      Contains    Traversal     Name       |
                '[' | ']' | '>' | '~' | '=' | '/' | '#' | ':' | '!' | '.' => {
                    let symbol = self.advance().unwrap();
                    Ok(TokenType::Symbol(symbol))
                }

                _ => {
                    let unknown = self.advance().unwrap();
                    Ok(TokenType::Unknown(unknown))
                }
            }
        } else {
            Ok(TokenType::EndOfStream)
        }
    }
}

impl<'a> Iterator for Lexer<'a> {
    type Item = TokenType;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.position >= self.input.len() {
                return None;
            }
            match self.next_token() {
                Ok(token) => return Some(token),
                Err(_) => {
                    // TODO: handle this
                    continue;
                }
            }
        }
    }
}
