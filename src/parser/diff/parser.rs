use std::{
    cell::RefCell,
    collections::HashMap,
    iter::Peekable,
    mem::take,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use crate::{
    error_received_expected,
    hashtab::HashTab,
    parser::{common::StringCharacterTokenizer, diff::hash_processor::diff_hash_remapper, qml},
};
use anyhow::{bail, Error, Result};

use super::lexer::{Keyword, Lexer, TokenType};

pub trait ExternalLoader {
    fn load_external(&mut self, file: &str);
}

pub struct Parser<'a> {
    source_name: Arc<String>,
    stream: Peekable<Box<dyn Iterator<Item = TokenType>>>,
    root_path: Option<String>,
    hashtab: Option<&'a HashTab>,
    external_loader: Option<Rc<RefCell<Box<dyn ExternalLoader>>>>,
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
pub struct RebuildAction {
    pub selector: NodeSelector,
    pub actions: Vec<RebuildInstruction>,
    pub redefine: bool,
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
    Rebuild(RebuildAction),
    Replicate(NodeTree),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectToChange {
    FileTokenStream(String),
    File(String),
    Template(String),
    Slot(String),
}

#[derive(Debug, Clone)]
pub struct Change {
    pub source: Arc<String>,
    pub destination: ObjectToChange,
    pub changes: Vec<FileChangeAction>,
    pub versions_allowed: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct RebuildArgumentReference {
    pub position: usize,
    pub name: String,
}

#[derive(Debug, Clone)]
pub enum LocateRebuildActionSelector {
    All,
    Stream(Vec<qml::lexer::TokenType>),
}

#[derive(Debug, Clone)]
pub struct LocateRebuildAction {
    pub location: Location,
    pub selector: LocateRebuildActionSelector,
}

#[derive(Debug, Clone)]
pub enum RemoveRebuildAction {
    Located,
    Stream(Vec<qml::lexer::TokenType>),
    UntilStream(Vec<qml::lexer::TokenType>),
    UntilEnd,
}

#[derive(Debug, Clone)]
pub enum ReplaceRebuildActionWhat {
    LiteralStream(Vec<qml::lexer::TokenType>),
    Located,
}

#[derive(Debug, Clone)]
pub struct ReplaceRebuildAction {
    pub what: ReplaceRebuildActionWhat,
    pub new_contents: Vec<qml::lexer::TokenType>,
    pub until_stream: Option<Vec<qml::lexer::TokenType>>,
}

#[derive(Debug, Clone)]
pub enum RebuildInstruction {
    InsertArgument(RebuildArgumentReference),
    RemoveArgument(RebuildArgumentReference),
    RenameArgument(RebuildArgumentReference, String),

    Locate(LocateRebuildAction),
    Insert(Vec<qml::lexer::TokenType>),
    Remove(RemoveRebuildAction),
    Replace(ReplaceRebuildAction),
}

fn trim_token_stream(token_stream: &mut Vec<qml::lexer::TokenType>) {
    while let Some(qml::lexer::TokenType::Whitespace(_)) = token_stream.first() {
        token_stream.remove(0);
    }
    while let Some(qml::lexer::TokenType::Whitespace(_)) = token_stream.last() {
        token_stream.pop();
    }
}

impl<'a> Parser<'a> {
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

    fn read_argument_reference(&mut self) -> Result<RebuildArgumentReference> {
        // <name> AT <pos>
        let first_tok = self.next_lex()?;
        let second_tok = self.next_lex()?;
        let third_tok = self.next_lex()?;
        match (first_tok, second_tok, third_tok) {
            (
                TokenType::Identifier(name),
                TokenType::Keyword(Keyword::At),
                TokenType::Identifier(pos),
            ) => {
                if let Ok(position) = pos.parse::<usize>() {
                    Ok(RebuildArgumentReference { name, position })
                } else {
                    error_received_expected!(pos, "Argument position (number)")
                }
            }
            _ => error_received_expected!("Invalid combination", "Argument reference"),
        }
    }

    fn read_rebuild_instructions(&mut self, is_redefine: bool) -> Result<Vec<RebuildInstruction>> {
        let mut instructions = Vec::new();
        loop {
            let next = self.next_lex()?;
            if let TokenType::Keyword(kw) = next {
                match kw {
                    Keyword::End => {
                        let next = self.next_lex()?;
                        match next {
                            TokenType::Keyword(Keyword::Redefine) if is_redefine => {
                                return Ok(instructions);
                            }
                            TokenType::Keyword(Keyword::Rebuild) if !is_redefine => {
                                return Ok(instructions);
                            }
                            _ => {
                                return error_received_expected!(next, "END REBUILD");
                            }
                        }
                    }

                    Keyword::Affect
                    | Keyword::Traverse
                    | Keyword::Assert
                    | Keyword::Template
                    | Keyword::Import
                    | Keyword::Multiple
                    | Keyword::Slot
                    | Keyword::Load
                    | Keyword::External
                    | Keyword::With
                    | Keyword::To
                    | Keyword::All
                    | Keyword::After
                    | Keyword::Before
                    | Keyword::Until
                    | Keyword::Argument
                    | Keyword::At
                    | Keyword::Located
                    | Keyword::Rebuild
                    | Keyword::Replicate
                    | Keyword::Version
                    | Keyword::Redefine => {
                        return error_received_expected!(kw, "Rebuild directive keyword");
                    }

                    Keyword::Insert => {
                        let next_token = self.next_lex()?;
                        match next_token {
                            TokenType::QMLCode {
                                qml_code: mut code,
                                stream_character: _,
                            } => {
                                trim_token_stream(&mut code);
                                instructions.push(RebuildInstruction::Insert(code));
                            }
                            TokenType::Keyword(Keyword::Argument) => {
                                instructions.push(RebuildInstruction::InsertArgument(
                                    self.read_argument_reference()?,
                                ));
                            }
                            tok => {
                                return error_received_expected!(tok, "Stream / Argument");
                            }
                        }
                    }

                    Keyword::Remove => {
                        let next_token = self.next_lex()?;
                        match next_token {
                            TokenType::QMLCode {
                                qml_code: mut code,
                                stream_character: _,
                            } => {
                                trim_token_stream(&mut code);
                                instructions.push(RebuildInstruction::Remove(
                                    RemoveRebuildAction::Stream(code),
                                ));
                            }
                            TokenType::Keyword(Keyword::Located) => {
                                instructions
                                    .push(RebuildInstruction::Remove(RemoveRebuildAction::Located));
                            }
                            TokenType::Keyword(Keyword::Until) => {
                                let next_token = self.next_lex()?;
                                match next_token {
                                    TokenType::Keyword(Keyword::End) => {
                                        instructions.push(RebuildInstruction::Remove(
                                            RemoveRebuildAction::UntilEnd,
                                        ));
                                    }
                                    TokenType::QMLCode {
                                        qml_code: mut code,
                                        stream_character: _,
                                    } => {
                                        trim_token_stream(&mut code);
                                        instructions.push(RebuildInstruction::Remove(
                                            RemoveRebuildAction::UntilStream(code),
                                        ));
                                    }
                                    _ => {
                                        return error_received_expected!(
                                            next_token,
                                            "End / Stream"
                                        );
                                    }
                                }
                            }
                            TokenType::Keyword(Keyword::Argument) => {
                                instructions.push(RebuildInstruction::RemoveArgument(
                                    self.read_argument_reference()?,
                                ));
                            }
                            tok => {
                                return error_received_expected!(
                                    tok,
                                    "Located / Stream / Until End / Until Stream / Argument"
                                );
                            }
                        }
                    }

                    Keyword::Rename => {
                        let next_token = self.next_lex()?;
                        match next_token {
                            TokenType::Keyword(Keyword::Argument) => {
                                let arg_ref = self.read_argument_reference()?;
                                let next_token = self.next_lex()?;
                                if let TokenType::Keyword(Keyword::To) = next_token {
                                    let name = self.next_id()?;
                                    instructions
                                        .push(RebuildInstruction::RenameArgument(arg_ref, name));
                                } else {
                                    return error_received_expected!(next_token, "TO - Name");
                                }
                            }
                            other => {
                                return error_received_expected!(other, "Argument - TO - Name");
                            }
                        }
                    }

                    Keyword::Replace => {
                        let what = match self.next_lex()? {
                            TokenType::QMLCode {
                                qml_code: mut replace_what,
                                stream_character: _,
                            } => {
                                trim_token_stream(&mut replace_what);
                                ReplaceRebuildActionWhat::LiteralStream(replace_what)
                            }
                            TokenType::Keyword(Keyword::Located) => {
                                ReplaceRebuildActionWhat::Located
                            }
                            other => {
                                return error_received_expected!(other, "Stream - With - Stream / Stream - With - Stream - Until - Stream");
                            }
                        };
                        let mut next_token = self.next_lex()?;
                        let until_stream = if let TokenType::Keyword(Keyword::Until) = next_token {
                            let next = self.next_lex()?;
                            if let TokenType::QMLCode {
                                mut qml_code,
                                stream_character: _,
                            } = next
                            {
                                next_token = self.next_lex()?;
                                trim_token_stream(&mut qml_code);
                                Some(qml_code)
                            } else {
                                return error_received_expected!(next, "Stream");
                            }
                        } else {
                            None
                        };

                        if let TokenType::Keyword(Keyword::With) = next_token {
                            let next = self.next_lex()?;
                            if let TokenType::QMLCode {
                                qml_code: mut new_contents,
                                stream_character: _,
                            } = next
                            {
                                trim_token_stream(&mut new_contents);
                                instructions.push(RebuildInstruction::Replace(
                                    ReplaceRebuildAction {
                                        what,
                                        until_stream,
                                        new_contents,
                                    },
                                ));
                            } else {
                                return error_received_expected!(next, "Stream");
                            }
                        } else {
                            return error_received_expected!(
                                next_token,
                                "With - Stream / Until - Stream - With - Stream"
                            );
                        }
                    }

                    Keyword::Locate => {
                        let location = match self.next_lex()? {
                            TokenType::Keyword(Keyword::After) => Location::After,
                            TokenType::Keyword(Keyword::Before) => Location::Before,
                            other => {
                                return error_received_expected!(other, "Before / After");
                            }
                        };
                        let selector = match self.next_lex()? {
                            TokenType::Keyword(Keyword::All) => LocateRebuildActionSelector::All,
                            TokenType::QMLCode {
                                qml_code: mut code,
                                stream_character: _,
                            } => {
                                trim_token_stream(&mut code);
                                LocateRebuildActionSelector::Stream(code)
                            }
                            other => {
                                return error_received_expected!(other, "All / Stream");
                            }
                        };
                        instructions.push(RebuildInstruction::Locate(LocateRebuildAction {
                            location,
                            selector,
                        }));
                    }
                }
            } else {
                return error_received_expected!(next, "Rebuild directive keyword");
            }
        }
    }

    pub fn read_next_instruction(&mut self, in_slot: bool) -> Result<FileChangeAction> {
        let next = self.next_lex()?;
        if let TokenType::Keyword(kw) = next {
            match kw {
                Keyword::Rebuild => {
                    let selector = self.read_node()?;
                    Ok(FileChangeAction::Rebuild(RebuildAction {
                        selector,
                        actions: self.read_rebuild_instructions(false)?,
                        redefine: false,
                    }))
                }
                Keyword::Redefine => {
                    let selector = self.read_node()?;
                    Ok(FileChangeAction::Rebuild(RebuildAction {
                        selector,
                        actions: self.read_rebuild_instructions(true)?,
                        redefine: true,
                    }))
                }
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
                                Ok(TokenType::QMLCode {
                                    qml_code: code,
                                    stream_character: _,
                                }) => code,
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
                        TokenType::QMLCode {
                            qml_code: code,
                            stream_character: _,
                        } => Ok(FileChangeAction::Insert(Insertable::Code(code))),
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
                | Keyword::External
                | Keyword::To
                | Keyword::Slot
                | Keyword::With
                | Keyword::Argument
                | Keyword::Until
                | Keyword::Located
                | Keyword::Version
                | Keyword::At => error_received_expected!(kw, "Directive keyword"),

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
                        TokenType::QMLCode {
                            qml_code,
                            stream_character: _,
                        } => Ok(FileChangeAction::Replace(ReplaceAction {
                            content: Insertable::Code(qml_code),
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
                Keyword::Replicate => Ok(FileChangeAction::Replicate(self.read_tree()?)),
            }
        } else {
            error_received_expected!(next, "Directive keyword")
        }
    }

    fn get_full_path_and_root_of(&'a self, file: &str) -> Result<(&'a str, PathBuf)> {
        if let Some(ref root) = self.root_path {
            let new_path = Path::new(file);
            if new_path.is_absolute() {
                return Err(Error::msg("Cannot load files using absolute paths!"));
            }
            let full_path = Path::new(root).join(new_path.strip_prefix("/").unwrap_or(new_path));
            Ok((root, full_path))
        } else {
            Err(Error::msg("Cannot load a file if no root path set!"))
        }
    }

    fn load_external(&mut self, file: &str) -> Result<()> {
        let (_, file) = self.get_full_path_and_root_of(file)?;
        if let Some(external_handler) = &self.external_loader {
            external_handler
                .borrow_mut()
                .load_external(&file.to_string_lossy());
            Ok(())
        } else {
            bail!(
                "Cannot load external {} - no external loader supported!",
                file.display()
            );
        }
    }

    fn load_from(
        &mut self,
        file: &str,
        output: &mut Vec<Change>,
        versions_allowed: Option<Vec<String>>,
    ) -> Result<()> {
        let (root, full_path) = self.get_full_path_and_root_of(file)?;
        let file_contents = match std::fs::read_to_string(&full_path) {
            Ok(e) => e,
            Err(_) => {
                return Err(Error::msg(format!(
                    "Cannot read file {}",
                    full_path.to_string_lossy()
                )))
            }
        };
        let moved_root = if let Some(e) = Path::new(file).parent() {
            String::from(Path::new(root).join(e).to_string_lossy())
        } else {
            root.to_string()
        };
        let mut parser = Self::new(
            Box::new({
                if let Some(hashtab) = self.hashtab {
                    Lexer::new(StringCharacterTokenizer::new(file_contents))
                        .map(|e| {
                            diff_hash_remapper(hashtab, e, &full_path.to_string_lossy()).unwrap()
                        })
                        .collect::<Vec<TokenType>>()
                } else {
                    Lexer::new(StringCharacterTokenizer::new(file_contents))
                        .collect::<Vec<TokenType>>()
                }
                .into_iter()
            }),
            Some(moved_root),
            Arc::from(full_path.to_string_lossy().to_string()),
            self.hashtab,
            self.external_loader.clone(),
        );
        output.extend(parser.parse(versions_allowed.clone())?);
        Ok(())
    }

    pub fn parse(&mut self, parent_versions_allowed: Option<Vec<String>>) -> Result<Vec<Change>> {
        let mut output = Vec::default();
        // If we're loading a file using the `LOAD` keyword, the allowed versions should be propagated from the
        // parent ONLY. Do not allow defining more allowed versions in non-root files.
        // (Unless the root file does not provide any supported versions' list. Then allow it.)
        let allow_new_version_definitions = parent_versions_allowed.is_none();
        let mut versions_allowed = parent_versions_allowed;

        let mut current_working_file: Option<ObjectToChange> = None;
        let mut current_instructions = Vec::new();
        let mut in_slot = false;
        let mut has_seen_non_version_statements = false;
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

                            TokenType::Keyword(Keyword::Replicate) => {
                                current_instructions
                                    .push(FileChangeAction::End(Keyword::Replicate));
                                continue;
                            }

                            _ => return error_received_expected!(next, "AFFECT / SLOT / Template"),
                        }
                        output.push(Change {
                            source: self.source_name.clone(),
                            changes: take(&mut current_instructions),
                            destination: current_working_file.take().unwrap(),
                            versions_allowed: versions_allowed.clone(),
                        });
                    }
                    _ => current_instructions.push(self.read_next_instruction(in_slot)?),
                }
            } else {
                // The affected file always needs to be set.
                let next = self.next_lex()?;
                match &next {
                    TokenType::Keyword(Keyword::Version) if !allow_new_version_definitions => {
                        return Err(Error::msg("Error while parsing: Files loaded using the LOAD keyword cannot define more supported versions!".to_string()))
                    }
                    TokenType::Keyword(Keyword::Version) if has_seen_non_version_statements => {
                        return error_received_expected!(next, "AFFECT / SLOT / TEMPLATE statement (VERSION statements only allowed at the beginning of file!)");
                    }
                    TokenType::Keyword(Keyword::Version) if !has_seen_non_version_statements => {
                        let version_allowed = match self.next_string_or_id() {
                            Err(x) => {
                                return error_received_expected!(x, "Version Identifier / String");
                            }
                            Ok(e) => e,
                        };
                        let version_allowed = version_allowed
                            .trim_matches(['"', '\'', '`', ' ', '\n'])
                            .into();
                        match versions_allowed {
                            None => {
                                versions_allowed = Some(vec![version_allowed]);
                            }
                            Some(ref mut x) => {
                                x.push(version_allowed);
                            }
                        };
                    }
                    TokenType::Keyword(Keyword::Affect) => {
                        has_seen_non_version_statements = true;
                        self.discard_whitespace();
                        if let Some(TokenType::Keyword(Keyword::Rebuild)) = self.stream.peek() {
                            let _ = self.next_lex();
                            let file_to_change = ObjectToChange::FileTokenStream(self.next_string_or_id()?);
                            output.push(Change {
                                source: self.source_name.clone(),
                                changes: vec![FileChangeAction::Rebuild(RebuildAction {
                                    selector: NodeSelector { object_name: "root".to_string(), named: None, props: Default::default() },
                                    redefine: false,
                                    actions: self.read_rebuild_instructions(false)?,
                                })],
                                destination: file_to_change,
                                versions_allowed: versions_allowed.clone()
                            });
                            continue;
                        } else {
                            current_working_file =
                                Some(ObjectToChange::File(self.next_string_or_id()?));
                        }
                        in_slot = false;
                    }
                    TokenType::Keyword(Keyword::Template) => {
                        has_seen_non_version_statements = true;
                        let name = self.next_id()?;
                        let data = match self.next_lex() {
                            Ok(TokenType::QMLCode {
                                qml_code,
                                stream_character: _,
                            }) => qml_code,
                            _ => panic!("Expected TEMPLATE <name> {{...}}"),
                        };
                        output.push(Change {
                            source: self.source_name.clone(),
                            destination: ObjectToChange::Template(name),
                            changes: vec![FileChangeAction::Insert(Insertable::Code(data))],
                            versions_allowed: versions_allowed.clone(),
                        });
                    }
                    TokenType::Keyword(Keyword::Slot) => {
                        has_seen_non_version_statements = true;
                        in_slot = true;
                        current_working_file = Some(match next {
                            TokenType::Keyword(Keyword::Slot) => {
                                ObjectToChange::Slot(self.next_id()?)
                            }
                            _ => panic!(),
                        });
                    }
                    TokenType::Keyword(Keyword::Load) => {
                        has_seen_non_version_statements = true;
                        self.discard_whitespace();
                        if let Some(TokenType::Keyword(Keyword::External)) = self.stream.peek() {
                            let _ = self.next_lex();
                            let path = self.read_path()?;
                            self.load_external(&path)?;
                        } else {
                            let path = self.read_path()?;
                            self.load_from(&path, &mut output, versions_allowed.clone())?;
                        }
                    }

                    _ => {
                        return error_received_expected!(
                            next,
                            "AFFECT / SLOT / VERSION / TEMPLATE statement"
                        )
                    }
                }
            }
        }

        if current_working_file.is_some() {
            output.push(Change {
                source: self.source_name.clone(),
                destination: current_working_file.take().unwrap(),
                changes: std::mem::take(&mut current_instructions),
                versions_allowed: versions_allowed.clone(),
            });
        }

        Ok(output)
    }

    pub fn new(
        token_stream: Box<dyn Iterator<Item = TokenType>>,
        root_path: Option<String>,
        source_name: Arc<String>,
        hashtab: Option<&'a HashTab>,
        external_loader: Option<Rc<RefCell<Box<dyn ExternalLoader>>>>,
    ) -> Parser<'a> {
        Parser {
            source_name,
            stream: token_stream.peekable(),
            root_path,
            hashtab,
            external_loader,
        }
    }
}
