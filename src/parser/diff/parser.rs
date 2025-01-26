use std::{collections::HashMap, iter::Peekable, mem::take, path::Path};

use crate::{error_received_expected, hashtab::HashTab};
use anyhow::{Error, Result};

use super::lexer::{Keyword, Lexer, TokenType};

pub struct Parser<'a> {
    stream: Peekable<Box<dyn Iterator<Item = TokenType>>>,
    root_path: Option<String>,
    hashtab: &'a HashTab,
}

#[derive(Debug, Clone)]
pub enum PropRequirement {
    Exists,
    Equals(String),
    Contains(String),
}

#[derive(Debug, Clone)]
pub struct NodeSelector {
    pub object_name: String,
    pub named: Option<String>,
    pub props: HashMap<String, PropRequirement>,
}

impl std::fmt::Display for NodeSelector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.object_name)?;
        if let Some(name) = &self.named {
            write!(f, ":{}", name)?;
        }
        if let Some(PropRequirement::Equals(id)) = self.props.get("id") {
            write!(f, "#{}", id)?;
        }
        for (name, replacement) in &self.props {
            if name != "id" || !matches!(replacement, PropRequirement::Equals(_)) {
                match replacement {
                    PropRequirement::Exists => {
                        write!(f, "[!{}]", name)?;
                    }
                    PropRequirement::Equals(val) => {
                        write!(f, "[.{}={}]", name, val)?;
                    }
                    PropRequirement::Contains(val) => {
                        write!(f, "[.{}~{}]", name, val)?;
                    }
                }
            }
        }
        Ok(())
    }
}

impl NodeSelector {
    pub fn new(name: String) -> Self {
        Self {
            object_name: name,
            named: None,
            props: HashMap::new(),
        }
    }

    pub fn is_simple(&self) -> bool {
        self.props.is_empty() && self.named.is_none()
    }
}

pub type NodeTree = Vec<NodeSelector>;

#[derive(Debug, Clone)]
pub enum Location {
    Before,
    After,
}

#[derive(Debug, Clone)]
pub enum LocationSelector {
    All,
    Tree(NodeTree),
}

#[derive(Debug, Clone)]
pub struct LocateAction {
    pub selector: LocationSelector,
    pub location: Location,
}

#[derive(Debug, Clone)]
pub struct ReplaceAction {
    pub selector: NodeTree,
    pub content: Insertable, // QML / SLOT / TEMPLATE
}

#[derive(Debug, Clone)]
pub enum Insertable {
    Code(Vec<crate::parser::qml::lexer::TokenType>),
    Slot(String),
    Template(String, Vec<crate::parser::qml::lexer::TokenType>),
}

#[derive(Debug, Clone)]
pub struct ImportAction {
    pub name: String,
    pub version: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RenameAction {
    pub selector: NodeTree,
    pub name_to: String,
}

#[derive(Debug, Clone)]
pub enum FileChangeAction {
    Traverse(NodeTree),
    Assert(NodeTree),
    Locate(LocateAction),
    Remove(NodeSelector),
    Rename(RenameAction),
    Insert(
        Insertable, /*The QML Code as a string, for the QML parser to work on, or a slot*/
    ),
    Replace(ReplaceAction),
    End(Keyword),
    AllowMultiple,
    AddImport(ImportAction),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectToChange {
    File(String),
    Template(String),
    Slot(String),
}

#[derive(Debug, Clone)]
pub struct Change {
    pub destination: ObjectToChange,
    pub changes: Vec<FileChangeAction>,
}

impl Parser<'_> {
    fn next_lex(&mut self) -> Result<TokenType> {
        self.discard_whitespace();

        match self.stream.next() {
            Some(token) => Ok(token),
            None => Err(Error::msg("Unexpected end of diff-stream")),
        }
    }

    fn next_id(&mut self) -> Result<String> {
        let next = self.next_lex()?;
        match next {
            TokenType::Identifier(id) => Ok(id),
            _ => error_received_expected!(next, "Identifier"),
        }
    }
    fn next_string_or_id(&mut self) -> Result<String> {
        let next = self.next_lex()?;
        match next {
            TokenType::Identifier(s) | TokenType::String(s) => Ok(s),
            next => {
                error_received_expected!(next, "String or identifier")
            }
        }
    }

    fn discard_whitespace(&mut self) {
        loop {
            match self.stream.peek() {
                Some(TokenType::Whitespace(_))
                | Some(TokenType::NewLine(_))
                | Some(TokenType::Comment(_)) => {
                    self.stream.next();
                }
                _ => return,
            }
        }
    }

    fn read_path(&mut self) -> Result<String> {
        self.discard_whitespace();
        let next = match self.stream.next() {
            None => return error_received_expected!("EOD", "path token"),
            Some(e) => e,
        };
        match next {
            // TokenType::Symbol(s) => Ok(self.read_simple_path(&String::from(s))?),
            TokenType::String(str) => Ok(str),
            // TokenType::Identifier(id) => Ok(self.read_simple_path(&id)?),
            TokenType::Identifier(id) => Ok(id),
            _ => error_received_expected!(next, "path"),
        }
    }

    pub fn read_node(&mut self) -> Result<NodeSelector> {
        //                         /------------------------------\ /----------------------------------------------------\
        // ObjectName : named # id = property_name = property_value = property name ~ "property value contains this value"
        // [...] can be used for grouping.
        let name = self.next_id()?;
        let mut object = NodeSelector::new(name);
        while let Some(TokenType::Symbol(symbol)) = self.stream.peek() {
            match symbol {
                '[' | ']' => {
                    self.stream.next();
                    continue;
                } // Meaningless
                '!' => {
                    self.stream.next();
                    object
                        .props
                        .insert(self.next_id()?, PropRequirement::Exists);
                }
                ':' => {
                    self.stream.next();
                    object.named = Some(self.next_id()?);
                }
                '#' => {
                    self.stream.next();
                    object
                        .props
                        .insert("id".to_string(), PropRequirement::Equals(self.next_id()?));
                }
                '.' => {
                    self.stream.next();
                    // Next is the property name
                    let prop_name = self.next_id()?;
                    // Next should be a symbol - '=' or '~'
                    let next = self.next_lex()?;
                    match next {
                        TokenType::Symbol('~') => {
                            // Then string / identifier
                            // let next = self.next_lex()?;
                            let string_value = self.next_string_or_id()?;
                            object
                                .props
                                .insert(prop_name, PropRequirement::Contains(string_value));
                        }
                        TokenType::Symbol('=') => {
                            // Then ID
                            let id = self.next_string_or_id()?;
                            object.props.insert(prop_name, PropRequirement::Equals(id));
                        }
                        _ => return error_received_expected!(next, "Property value condition"),
                    }
                }
                '>' => break, // Tree.
                _ => return error_received_expected!(self.stream.peek(), "Property match symbol"),
            }
        }

        Ok(object)
    }

    pub fn read_tree(&mut self) -> Result<NodeTree> {
        // Node > Node
        let mut nodes = vec![self.read_node()?];
        self.discard_whitespace();
        while let Some(TokenType::Symbol('>')) = self.stream.peek() {
            self.stream.next();
            nodes.push(self.read_node()?);
            self.discard_whitespace();
        }

        Ok(nodes)
    }

    pub fn read_next_instruction(&mut self, in_slot: bool) -> Result<FileChangeAction> {
        let next = self.next_lex()?;
        if let TokenType::Keyword(kw) = next {
            match kw {
                Keyword::Import => {
                    let name = self.next_id()?;
                    let version = self.next_id()?;
                    self.discard_whitespace();
                    let alias = match self.stream.peek() {
                        Some(TokenType::Identifier(id)) => Some(id.clone()),
                        _ => None,
                    };
                    if alias.is_some() {
                        self.stream.next();
                    }
                    Ok(FileChangeAction::AddImport(ImportAction {
                        name,
                        version,
                        alias,
                    }))
                }
                Keyword::Rename => {
                    let node = self.read_tree()?;
                    self.discard_whitespace();
                    let next = self.next_lex()?;
                    match next {
                        TokenType::Keyword(Keyword::To) => {}
                        _ => return error_received_expected!(next, "TO"),
                    }
                    let name = self.next_string_or_id()?;
                    Ok(FileChangeAction::Rename(RenameAction {
                        name_to: name,
                        selector: node,
                    }))
                }
                Keyword::Insert => {
                    let next = self.next_lex()?;
                    match next {
                        TokenType::Keyword(Keyword::Template) => {
                            self.discard_whitespace();
                            let template_name = self.next_id()?;
                            self.discard_whitespace();
                            let next_token = match self.next_lex() {
                                Ok(TokenType::QMLCode(code)) => code,
                                _ => {
                                    return Err(Error::msg("Expected 'INSERT TEMPLATE <name> {}"));
                                }
                            };

                            Ok(FileChangeAction::Insert(Insertable::Template(
                                template_name,
                                next_token,
                            )))
                        }
                        TokenType::Keyword(Keyword::Slot) => {
                            Ok(FileChangeAction::Insert(Insertable::Slot(self.next_id()?)))
                        }
                        TokenType::QMLCode(code) => {
                            Ok(FileChangeAction::Insert(Insertable::Code(code)))
                        }
                        _ => error_received_expected!(next, "QML code"),
                    }
                }
                _ if in_slot => error_received_expected!(kw, "INSERT"),

                Keyword::Affect
                | Keyword::After
                | Keyword::All
                | Keyword::Template
                | Keyword::Before
                | Keyword::Load
                | Keyword::To
                | Keyword::Slot
                | Keyword::With => error_received_expected!(kw, "Directive keyword"),

                Keyword::Assert => Ok(FileChangeAction::Assert(self.read_tree()?)),
                Keyword::End => {
                    let next = self.next_lex()?;
                    match next {
                        TokenType::Keyword(Keyword::Traverse)
                        | TokenType::Keyword(Keyword::Affect)
                        | TokenType::Keyword(Keyword::Slot)
                        | TokenType::Keyword(Keyword::Template) => {
                            Ok(FileChangeAction::End(Keyword::Traverse))
                        }
                        _ => error_received_expected!(next, "End-able keyword"),
                    }
                }
                Keyword::Locate => {
                    // LOCATE AFTER <Selector>
                    // LOCATE AFTER ALL
                    // LOCATE BEFORE ALL
                    // LOCATE BEFORE <Selector>
                    let next = self.next_lex()?;
                    let location = match next {
                        TokenType::Keyword(Keyword::After) => Location::After,
                        TokenType::Keyword(Keyword::Before) => Location::Before,
                        _ => return error_received_expected!(next, "Before / After"),
                    };
                    self.discard_whitespace();
                    let peek = self.stream.peek();
                    let selector = match peek {
                        Some(TokenType::Identifier(_)) => LocationSelector::Tree(self.read_tree()?),
                        Some(TokenType::Keyword(Keyword::All)) => {
                            self.stream.next();
                            LocationSelector::All
                        }
                        _ => return error_received_expected!(peek, "ALL / tree"),
                    };
                    Ok(FileChangeAction::Locate(LocateAction {
                        location,
                        selector,
                    }))
                }
                Keyword::Remove => Ok(FileChangeAction::Remove(self.read_node()?)),
                Keyword::Multiple => Ok(FileChangeAction::AllowMultiple),
                Keyword::Replace => {
                    let node = self.read_tree()?;
                    self.discard_whitespace();
                    let next = self.next_lex()?;
                    match next {
                        TokenType::Keyword(Keyword::With) => {}
                        _ => return error_received_expected!(next, "WITH"),
                    }
                    let next = self.next_lex()?;
                    match next {
                        TokenType::QMLCode(code) => Ok(FileChangeAction::Replace(ReplaceAction {
                            content: Insertable::Code(code),
                            selector: node,
                        })),
                        TokenType::Keyword(Keyword::Slot) => {
                            Ok(FileChangeAction::Replace(ReplaceAction {
                                content: Insertable::Slot(self.next_id()?),
                                selector: node,
                            }))
                        }
                        _ => error_received_expected!(next, "QML code / SLOT <slot>"),
                    }
                }
                Keyword::Traverse => Ok(FileChangeAction::Traverse(self.read_tree()?)),
            }
        } else {
            error_received_expected!(next, "Directive keyword")
        }
    }

    fn load_from(&mut self, file: &str, output: &mut Vec<Change>) -> Result<()> {
        if let Some(ref root) = self.root_path {
            let new_path = Path::new(file);
            if new_path.is_absolute() {
                return Err(Error::msg("Cannot load files using absolute paths!"));
            }
            let full_path = Path::new(root).join(new_path.strip_prefix("/").unwrap_or(new_path));
            let file_contents = match std::fs::read_to_string(&full_path) {
                Ok(e) => e,
                Err(_) => {
                    return Err(Error::msg(format!(
                        "Cannot read file {}",
                        full_path.to_string_lossy()
                    )))
                }
            };
            let mut parser = Self::new(
                Box::new(
                    Lexer::new(file_contents)
                        .collect::<Vec<TokenType>>()
                        .into_iter(),
                ),
                self.root_path.clone(),
                self.hashtab,
            );
            output.extend(parser.parse()?);
            Ok(())
        } else {
            Err(Error::msg("Cannot load a file if no root path set!"))
        }
    }

    pub fn parse(&mut self) -> Result<Vec<Change>> {
        let mut output = Vec::default();

        let mut current_working_file: Option<ObjectToChange> = None;
        let mut current_instructions = Vec::new();
        let mut in_slot = false;
        loop {
            // End of file condition:
            self.discard_whitespace();
            match self.stream.peek() {
                None | Some(TokenType::EndOfStream) if current_working_file.is_some() => {
                    return error_received_expected!("EoF", "END directive")
                }
                None | Some(TokenType::EndOfStream) => break,
                _ => {}
            }

            if current_working_file.is_some() {
                match self.stream.peek() {
                    Some(TokenType::Keyword(Keyword::End)) => {
                        self.stream.next();
                        let next = self.next_lex()?;
                        match next {
                            TokenType::Keyword(Keyword::Affect)
                            | TokenType::Keyword(Keyword::Slot)
                            | TokenType::Keyword(Keyword::Template) => {}

                            TokenType::Keyword(Keyword::Traverse) => {
                                current_instructions.push(FileChangeAction::End(Keyword::Traverse));
                                continue;
                            }

                            _ => return error_received_expected!(next, "AFFECT / SLOT / Template"),
                        }
                        output.push(Change {
                            changes: take(&mut current_instructions),
                            destination: current_working_file.take().unwrap(),
                        });
                    }
                    _ => current_instructions.push(self.read_next_instruction(in_slot)?),
                }
            } else {
                // The affected file always needs to be set.
                let next = self.next_lex()?;
                match &next {
                    TokenType::Keyword(Keyword::Affect) => {
                        current_working_file =
                            Some(ObjectToChange::File(self.next_string_or_id()?));
                        in_slot = false;
                    }
                    TokenType::Keyword(Keyword::Template) => {
                        let name = self.next_id()?;
                        let data = match self.next_lex() {
                            Ok(TokenType::QMLCode(c)) => c,
                            _ => panic!("Expected TEMPLATE <name> {{...}}"),
                        };
                        output.push(Change {
                            destination: ObjectToChange::Template(name),
                            changes: vec![FileChangeAction::Insert(Insertable::Code(data))],
                        });
                    }
                    TokenType::Keyword(Keyword::Slot) => {
                        in_slot = true;
                        current_working_file = Some(match next {
                            TokenType::Keyword(Keyword::Slot) => {
                                ObjectToChange::Slot(self.next_id()?)
                            }
                            _ => panic!(),
                        });
                    }
                    TokenType::Keyword(Keyword::Load) => {
                        let path = self.read_path()?;
                        self.load_from(&path, &mut output)?;
                    }

                    _ => {
                        return error_received_expected!(next, "AFFECT / SLOT / TEMPLATE statement")
                    }
                }
            }
        }

        if current_working_file.is_some() {
            output.push(Change {
                destination: current_working_file.take().unwrap(),
                changes: std::mem::take(&mut current_instructions),
            });
        }

        Ok(output)
    }

    pub fn new(
        token_stream: Box<dyn Iterator<Item = TokenType>>,
        root_path: Option<String>,
        hashtab: &HashTab,
    ) -> Parser {
        Parser {
            stream: token_stream.peekable(),
            root_path,
            hashtab,
        }
    }
}
