use std::fmt::Display;

use anyhow::Error;

use crate::{hashtab::HashTab, slots::Slots};

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
        })
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum TokenType {
    Keyword(Keyword),
    SymbolicKeyword(SymbolicKeyword),
    Identifier(String),
    Number(u64),
    String(String),
    Symbol(char),
    Comment(String),
    NewLine(usize),
    Whitespace(String),
    EndOfStream,
    Unknown(char),
}

#[derive(Clone)]
pub struct QMLDiffExtensions<'a> {
    hashtab: Option<&'a HashTab>,
    slots: Option<&'a Slots>,
}

impl<'a> QMLDiffExtensions<'a> {
    pub fn new(hashtab: Option<&'a HashTab>, slot_resolver: Option<&'a Slots>) -> Self {
        Self {
            hashtab,
            slots: slot_resolver,
        }
    }
}

pub struct Lexer<'a> {
    // HashTab is only required when reading the DIFF files.
    // Similarly to the DIFFs themselves, a hash can also repalce
    // any identifier within the QML tree.
    extensions: Option<QMLDiffExtensions<'a>>,
    input: String,                               // Raw input string
    position: usize,                             // current position in the input
    line_pos: usize,                             // Current position within a line [unused.]
    pub slots_used: Option<&'a mut Vec<String>>, // Slots used by this token stream
}

impl<'a> Lexer<'a> {
    pub fn new(
        input: String,
        extended_features: Option<QMLDiffExtensions<'a>>,
        slots_used: Option<&'a mut Vec<String>>,
    ) -> Self {
        Lexer {
            input,
            position: 0,
            line_pos: 0,
            extensions: extended_features,
            slots_used,
        }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.position..].chars().next()
    }

    fn peek_offset(&self, off: usize) -> Option<char> {
        self.input[self.position + off..].chars().next()
    }

    fn advance(&mut self) -> Option<char> {
        if let Some(c) = self.peek() {
            self.position += c.len_utf8();
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
        while let Some(c) = self.peek() {
            if condition(self, c) {
                result.push(c);
                self.advance();
            } else {
                break;
            }
        }
        result
    }
}

impl<'a> Lexer<'a> {
    pub fn next_token(&mut self) -> Result<TokenType, Error> {
        if let Some(c) = self.peek() {
            match c {
                // Cannot use [[hash]] - it's valid JS
                // For hashes here, ~&hash&~ will be used.
                // For hashed string: ~&[q]hash&~
                // where [q] is one of `, ', "
                // Example: ~&'1234&~
                '~' if self.peek_offset(1) == Some('&')
                    && self.extensions.is_some()
                    && self.extensions.as_ref().unwrap().hashtab.is_some() =>
                {
                    // HASH!
                    self.advance();
                    self.advance();
                    // If string_quote is None, that means we're not dealing
                    // with a string, and should proceed normally.
                    let string_quote: Option<char> = match self.peek() {
                        Some('\'') | Some('"') | Some('`') => self.advance(),
                        _ => None,
                    };
                    let hash_str =
                        self.collect_while(|this, c| c != '&' && this.peek_offset(1) != Some('~'));
                    self.advance(); // Remove &
                    self.advance(); // Remove ~
                    let hash = hash_str.parse::<u64>()?;
                    if let Some(resolved) = self
                        .extensions
                        .as_ref()
                        .unwrap()
                        .hashtab
                        .unwrap()
                        .get(&hash)
                    {
                        if let Some(quote) = string_quote {
                            Ok(TokenType::String(format!(
                                "{}{}{}",
                                quote,
                                resolved.clone(),
                                quote
                            )))
                        } else {
                            Ok(TokenType::Identifier(resolved.clone()))
                        }
                    } else {
                        Err(Error::msg(format!(
                            "Cannot dereference hash {} - not found in hashtab",
                            hash
                        )))
                    }
                }
                '~' if self.peek_offset(1) == Some('{')
                    && self.extensions.is_some()
                    && self.extensions.as_ref().unwrap().slots.is_some() =>
                {
                    // Slot
                    self.advance();
                    self.advance();
                    let slot_name =
                        self.collect_while(|this, c| c != '}' && this.peek_offset(1) != Some('~'));
                    self.advance(); // Remove }
                    self.advance(); // Remove ~
                    let (resolved, slots_used_extra) = self
                        .extensions
                        .as_ref()
                        .unwrap()
                        .slots
                        .unwrap()
                        .resolve_slot_final_state(&slot_name)?;
                    if let Some(slots) = &mut self.slots_used {
                        slots.extend(slots_used_extra);
                    }
                    self.input.insert_str(self.position, &resolved);

                    self.next_token()
                }
                '\n' => {
                    self.advance();
                    self.line_pos += 1;
                    Ok(TokenType::NewLine(self.line_pos))
                }

                c if c.is_whitespace() && c != '\n' => {
                    let str = self.collect_while(|_, c| c.is_whitespace());
                    Ok(TokenType::Whitespace(str))
                }

                '/' if self.input[self.position..].starts_with("//") => {
                    self.advance();
                    self.advance();
                    let comment = self.collect_while(|_, c| c != '\n');
                    Ok(TokenType::Comment(comment))
                }

                '/' if self.input[self.position..].starts_with("/*") => {
                    self.advance();
                    self.advance();
                    let comment =
                        self.collect_while(|s, _c| !s.input[s.position..].starts_with("*/"));
                    self.advance(); // Consume '*'
                    self.advance(); // Consume '/'
                    Ok(TokenType::Comment(comment))
                }

                '"' | '\'' | '`' => {
                    let quote = self.advance().unwrap();
                    let mut is_quoted = false;
                    let string = self.collect_while(move |_, c| {
                        if is_quoted {
                            is_quoted = false;
                            return true;
                        }
                        if c == quote {
                            return false;
                        }
                        if c == '\\' {
                            is_quoted = true;
                        }
                        true
                    });

                    self.advance(); // Consume closing quote
                    let s_quote = String::from(quote);
                    Ok(TokenType::String(s_quote.clone() + &string + &s_quote))
                }

                c if c.is_ascii_digit() => {
                    let num_str = self.collect_while(|_, c| c.is_ascii_digit());
                    let number = num_str.parse::<u64>().unwrap();
                    Ok(TokenType::Number(number))
                }

                c if c.is_alphabetic() || c == '_' => {
                    let ident = self.collect_while(|_, c| c.is_alphanumeric() || c == '_');
                    if let Ok(keyword) = Keyword::try_from(ident.as_str()) {
                        Ok(TokenType::Keyword(keyword))
                    } else if let Ok(symbolic) = SymbolicKeyword::try_from(ident.as_str()) {
                        Ok(TokenType::SymbolicKeyword(symbolic))
                    } else {
                        Ok(TokenType::Identifier(ident))
                    }
                }

                '{' | '}' | ':' | ';' | '.' | ',' | '(' | ')' | '[' | ']' | '|' | '&' | '%' => {
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
            if let Ok(token) = self.next_token() {
                return Some(token);
            }
        }
    }
}
