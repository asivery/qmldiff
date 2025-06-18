use anyhow::{Error, Result};
use std::{
    iter::Peekable,
    mem::{discriminant, Discriminant},
};

use super::{
    emitter::emit_simple_token_stream,
    lexer::{Keyword, TokenType},
};

pub type QMLTree = Vec<TreeElement>;

#[derive(Debug, PartialEq, Eq)]
pub struct Import {
    pub object_name: String,
    pub version: Option<String>,
    pub alias: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SignalChild {
    pub name: String,
    pub arguments: Option<Vec<TokenType>>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct PropertyChild<T: Clone> {
    pub name: String,
    pub default_value: T,
    pub modifiers: Vec<Keyword>,
    pub r#type: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum AssignmentChildValue {
    Object(Object),
    // List(Vec<AssignmentChildValue>),
    Other(Vec<TokenType>),
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct AssignmentChild {
    pub name: String,
    pub value: AssignmentChildValue,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ObjectAssignmentChild {
    pub name: String,
    pub value: Object,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct FunctionChild {
    pub name: String,
    pub arguments: Vec<TokenType>,
    pub body: Vec<TokenType>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct EnumChild {
    pub name: String,
    pub values: Vec<(String, Option<u64>)>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Pragma {
    pub pragma: String,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ComponentDefinition {
    pub name: String,
    pub object: Object,
}

#[derive(Debug, Clone)]
pub enum ObjectChild {
    Signal(SignalChild),
    Property(PropertyChild<Option<AssignmentChildValue>>),
    ObjectProperty(PropertyChild<Object>),
    Assignment(AssignmentChild),
    ObjectAssignment(ObjectAssignmentChild),
    Function(FunctionChild),
    Object(Object),
    Enum(EnumChild),
    Component(ComponentDefinition),
}

impl<'a> ObjectChild {
    pub fn get_name(&'a self) -> Option<&'a String> {
        match self {
            ObjectChild::Assignment(assi) => Some(&assi.name),
            ObjectChild::ObjectAssignment(assi) => Some(&assi.name),
            ObjectChild::Component(cmp) => Some(&cmp.name),
            ObjectChild::Enum(e) => Some(&e.name),
            ObjectChild::Function(fnc) => Some(&fnc.name),
            ObjectChild::Object(_) => None,
            ObjectChild::Property(prop) => Some(&prop.name),
            ObjectChild::ObjectProperty(prop) => Some(&prop.name),
            ObjectChild::Signal(signal) => Some(&signal.name),
        }
    }

    pub fn get_str_value(&'a self) -> Option<String> {
        match self {
            ObjectChild::Assignment(assigned) => match &assigned.value {
                AssignmentChildValue::Other(generic_value) => {
                    Some(emit_simple_token_stream(generic_value))
                }
                _ => None,
            },
            ObjectChild::ObjectAssignment(_) => None,
            ObjectChild::Component(_) => None,
            ObjectChild::Enum(_) => None,
            ObjectChild::Function(_) => None,
            ObjectChild::Object(_) => None,
            ObjectChild::Property(prop) => match &prop.default_value {
                Some(AssignmentChildValue::Other(generic_value)) => {
                    Some(emit_simple_token_stream(generic_value))
                }
                _ => None,
            },
            ObjectChild::ObjectProperty(_) => None,
            ObjectChild::Signal(_) => None,
        }
    }
}

impl PartialEq for ObjectChild {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (ObjectChild::Signal(a), ObjectChild::Signal(b)) => a == b,
            (ObjectChild::Property(a), ObjectChild::Property(b)) => a == b,
            (ObjectChild::ObjectAssignment(a), ObjectChild::ObjectAssignment(b)) => a == b,
            (ObjectChild::Assignment(a), ObjectChild::Assignment(b)) => a == b,
            (ObjectChild::Function(a), ObjectChild::Function(b)) => a == b,
            (ObjectChild::Object(a), ObjectChild::Object(b)) => a == b,
            (ObjectChild::Enum(a), ObjectChild::Enum(b)) => a == b,
            (ObjectChild::Component(a), ObjectChild::Component(b)) => a == b,
            _ => false,
        }
    }
}

// Implement Eq for ObjectChild as well, since PartialEq is implemented
impl Eq for ObjectChild {}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Object {
    pub name: String,
    pub children: Vec<ObjectChild>,
    pub full_name: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TreeElement {
    Import(Import),
    Object(Object),
    Pragma(Pragma),
}

pub struct Parser {
    stream: Peekable<Box<dyn Iterator<Item = TokenType>>>,
}

macro_rules! error_received_expected {
    ($recvd: expr, $expected: expr) => {
        Err(Error::msg(format!(
            "Error while parsing: expected {}, got {:?}",
            $expected, $recvd
        )))
    };
}

impl Parser {
    pub fn new(token_stream: Box<dyn Iterator<Item = TokenType>>) -> Parser {
        Parser {
            stream: token_stream.peekable(),
        }
    }

    fn build_delimeted_name(
        &mut self,
        delim: char,
        type_allowed: Discriminant<TokenType>,
        next_delim: bool,
    ) -> Result<String> {
        let mut final_string = String::default();
        let mut next_delim = next_delim;
        loop {
            let token = self.stream.peek();
            match token {
                Some(TokenType::Symbol(chr)) |
                Some(TokenType::Unknown(chr)) => {
                    if *chr == delim {
                        if next_delim {
                            final_string.push(*chr);
                            next_delim = false;
                        } else {
                            // Two delims one after another - this is bad
                            return error_received_expected!("<ident>", delim);
                        }
                    } else {
                        // Some other symbol.
                        return Ok(final_string);
                    }
                }

                Some(TokenType::Whitespace(_))
                | Some(TokenType::NewLine(_))
                | Some(TokenType::EndOfStream)
                | None => {
                    return Ok(final_string);
                }

                Some(token) if type_allowed != discriminant(token) => {
                    return error_received_expected!(token, "valid token");
                }

                Some(TokenType::Identifier(ident)) => {
                    if next_delim {
                        return error_received_expected!(ident, format!("Delimeter {}", delim));
                    }
                    next_delim = true;
                    final_string.push_str(ident);
                }

                Some(TokenType::Number(n)) => {
                    if next_delim {
                        return error_received_expected!(n, format!("Delimeter {}", delim));
                    }
                    next_delim = true;
                    final_string.push_str(&n.to_string());
                }

                Some(token) => return error_received_expected!(token, "Symbol or delimeter"),
            }
            self.stream.next();
        }
    }

    fn next_lex(&mut self) -> Result<TokenType> {
        self.discard_whitespace();

        match self.stream.next() {
            Some(token) => Ok(token),
            None => Err(Error::msg("Unexpected end of QML-stream")),
        }
    }

    fn next_typed_id(&mut self) -> Result<String> {
        let mut base_id = self.next_id(true)?;
        self.discard_whitespace();
        if let Some(TokenType::Unknown('<')) = self.stream.peek() {
            self.stream.next();
            let type_id = self.next_typed_id()?;
            base_id.push('<');
            base_id.push_str(&type_id);
            base_id.push('>');
            let next = self.next_lex()?;
            if let TokenType::Unknown('>') = next {
            } else {
                return error_received_expected!(next, ">");
            }
        }

        Ok(base_id)
    }

    fn next_id(&mut self, allow_compound: bool) -> Result<String> {
        let tok = self.next_lex()?;
        let root = match tok {
            TokenType::Identifier(id) => id,
            TokenType::Keyword(k) => k.into(),
            _ => return error_received_expected!(tok, "identifier"),
        };

        if allow_compound {
            return self.reread_as_compound_name(root);
        }

        Ok(root)
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

    fn parse_pragma_statement(&mut self) -> Result<Pragma> {
        self.discard_whitespace();
        let id = self.next_id(false)?;
        let val = Pragma { pragma: id };
        self.discard_whitespace();
        if let Some(TokenType::Symbol(';')) = self.stream.peek() {
            self.stream.next();
        }

        Ok(val)
    }

    fn parse_import_statement(&mut self) -> Result<Import> {
        self.discard_whitespace();
        let name = match self.stream.peek() {
            Some(TokenType::Identifier(_)) => self.build_delimeted_name(
                '.',
                discriminant(&TokenType::Identifier(String::new())),
                false,
            )?,
            Some(TokenType::String(str)) => {
                let value = str.clone();
                self.stream.next();
                value
            }
            _ => return error_received_expected!(self.stream.peek(), "Valid import source"),
        };
        self.discard_whitespace();
        let version = if let Some(TokenType::Number(_)) = self.stream.peek() {
            Some(self.build_delimeted_name('.', discriminant(&TokenType::Number(0)), false)?)
        } else {
            None
        };
        self.discard_whitespace();
        let alias = if let Some(TokenType::Keyword(Keyword::As)) = self.stream.peek() {
            self.stream.next();
            let token = self.next_lex()?;
            if let TokenType::Identifier(ident) = token {
                Some(ident)
            } else {
                return error_received_expected!(token, "as-identifier for import");
            }
        } else {
            None
        };

        Ok(Import {
            object_name: name,
            version,
            alias,
        })
    }

    fn parse_global_scope(&mut self) -> Result<Vec<TreeElement>> {
        let mut elements = Vec::new();

        loop {
            self.discard_whitespace();
            let token = match self.stream.next() {
                None => break,
                Some(token) => token,
            };
            match token {
                TokenType::Keyword(Keyword::Import) => {
                    elements.push(TreeElement::Import(self.parse_import_statement()?));
                }
                TokenType::Keyword(Keyword::Pragma) => {
                    elements.push(TreeElement::Pragma(self.parse_pragma_statement()?));
                }

                TokenType::Identifier(object) => {
                    let name = self.reread_as_compound_name(object)?;
                    elements.push(TreeElement::Object(self.parse_object(
                        name,
                        false,
                        String::from("<root>"),
                    )?))
                }

                _ => return Err(Error::msg(format!("Unexpected token: {:?}!", token))),
            }
        }

        Ok(elements)
    }

    pub fn read_until_depth_runs_out(&mut self, start: char, end: char) -> Result<Vec<TokenType>> {
        let mut list = Vec::default();

        let mut depth = 0;
        {
            // Do initial check
            match self.stream.peek() {
                Some(TokenType::Symbol(s)) if *s == start => {}
                _ => {
                    return Err(Error::msg(format!(
                        "Cannot glob depth-calc expression - doesn't start with required {}",
                        start
                    )))
                }
            }
        }
        loop {
            let token = self.stream.next();
            if let Some(token) = token {
                if let TokenType::Symbol(symbol) = token {
                    if symbol == start {
                        depth += 1;
                    } else if symbol == end {
                        depth -= 1;
                        if depth == 0 {
                            list.push(token);
                            return Ok(list);
                        }
                    }
                }
                list.push(token);
            } else {
                return Err(Error::msg("Unexpected end of QML-stream"));
            }
        }
    }

    pub fn reread_as_compound_name(&mut self, root: String) -> Result<String> {
        let mut root = root.clone();
        if let Some(TokenType::Symbol('.')) = self.stream.peek() {
            root.push_str(&self.build_delimeted_name(
                '.',
                discriminant(&TokenType::Identifier(String::default())),
                true,
            )?);
        }
        Ok(root)
    }

    fn read_value(&mut self, parent_name: String) -> Result<AssignmentChildValue> {
        // Read until two identifiers / identifier and keyword is detected
        let mut value = Vec::default();

        self.discard_whitespace();
        match self.stream.peek() {
            Some(TokenType::Symbol('[')) => {
                value.extend_from_slice(&self.read_until_depth_runs_out('[', ']')?);
            }
            Some(TokenType::Identifier(name)) => {
                let name = name.clone();
                let next = self.next_lex().unwrap();
                self.discard_whitespace();
                // Read next to check if it's an object
                if let Some(TokenType::Symbol('{')) = self.stream.peek() {
                    // It is
                    return Ok(AssignmentChildValue::Object(self.parse_object(
                        name.clone(),
                        false,
                        parent_name + ">" + &name,
                    )?));
                }
                // Is not. Push both to the value stack...
                value.push(next);
            }
            Some(TokenType::Symbol('(')) => {
                value.extend_from_slice(&self.read_until_depth_runs_out('(', ')')?);
                self.discard_whitespace();
                if let Some(TokenType::Unknown('=')) = self.stream.peek() {
                    value.push(self.stream.next().unwrap());
                    let next_lex = self.next_lex()?;
                    if let TokenType::Unknown('>') = next_lex {
                        value.push(next_lex);
                        self.discard_whitespace();
                        //value.extend_from_slice(&self.read_until_depth_runs_out('{', '}')?);
                        let read_value = self.read_value(parent_name)?;
                        if let AssignmentChildValue::Other(tokens) = read_value {
                            value.extend_from_slice(&tokens);
                            return Ok(AssignmentChildValue::Other(value));
                        } else {
                            return error_received_expected!(read_value, "Invalid lambda function");
                        }
                    } else {
                        return error_received_expected!(next_lex, "Lambda function");
                    }
                }
            }
            Some(TokenType::Symbol('{')) => {
                value.extend_from_slice(&self.read_until_depth_runs_out('{', '}')?);
            }

            _ => {
                // value.push(next); // Just start reading until either:
                // [<number> / <ident>] [<ident> / <kw>]
            }
        };

        let mut last_important = value.last().cloned();

        loop {
            self.discard_whitespace();
            // println!("Next is {:?}", self.stream.peek());
            match self.stream.peek() {
                Some(TokenType::Keyword(_))
                | Some(TokenType::Identifier(_))
                | Some(TokenType::Symbol('}'))
                | Some(TokenType::Symbol(';'))
                | Some(TokenType::Symbol(',')) => {
                    // Next is a kw or id.
                    // Was last one of non-terminal symbols?
                    'terminal: {
                        // println!("Last important is {:?}", &last_important);
                        match last_important {
                            None => break 'terminal,
                            Some(TokenType::Symbol(sym)) | Some(TokenType::Unknown(sym)) => {
                                match sym {
                                    // Terminal symbols:
                                    '}' | ')' | ']' | ';' => {} // Terminate
                                    _ => break 'terminal,
                                }
                            }
                            Some(TokenType::SymbolicKeyword(_)) => break 'terminal, // NEVER terminate.
                            _ => {}                                                 // Terminate.
                        }
                        // println!("Break! Value retrieved: {:?}", value);
                        return Ok(AssignmentChildValue::Other(value));
                    }
                    // println!("Prevented.");
                }
                Some(TokenType::Symbol('[')) => {
                    value.extend_from_slice(&self.read_until_depth_runs_out('[', ']')?);
                    last_important = Some(value.last().unwrap().clone());
                    continue;
                }
                Some(TokenType::Symbol('(')) => {
                    value.extend_from_slice(&self.read_until_depth_runs_out('(', ')')?);
                    last_important = Some(value.last().unwrap().clone());
                    continue;
                }
                Some(TokenType::Symbol('{')) => {
                    value.extend_from_slice(&self.read_until_depth_runs_out('{', '}')?);
                    last_important = Some(value.last().unwrap().clone());
                    continue;
                }
                _ => {}
            }
            // Continue on.
            let token = self.next_lex()?;
            match token {
                TokenType::Whitespace(_) | TokenType::NewLine(_) | TokenType::Comment(_) => {}
                _ => last_important = Some(token.clone()),
            }
            // println!("Token recvd for value: {:?}", &token);
            value.push(token);
        }
    }

    pub fn parse_object(
        &mut self,
        name: String,
        skip_brace: bool,
        full_tree_name: String,
    ) -> Result<Object> {
        let mut object = Object {
            name,
            children: Vec::new(),
            full_name: full_tree_name.clone(),
        };

        if !skip_brace {
            let paren_start = self.next_lex()?;
            match paren_start {
                TokenType::Symbol('{') => {}
                _ => return error_received_expected!(paren_start, "{"),
            };
        }

        loop {
            let token = self.next_lex();
            match token {
                Ok(token) => match token {
                    TokenType::Symbol(';') => {
                        continue;
                    }
                    TokenType::Symbol('}') => {
                        return Ok(object);
                    }
                    TokenType::Keyword(kw) => {
                        match kw {
                            Keyword::Signal => {
                                // Signals are constrained to:
                                // `signal name` or `signal name (...)`
                                let name = self.next_id(true)?;
                                self.discard_whitespace();
                                let arguments =
                                    if let Some(TokenType::Symbol('(')) = self.stream.peek() {
                                        Some(self.read_until_depth_runs_out('(', ')')?)
                                    } else {
                                        None
                                    };
                                object
                                    .children
                                    .push(ObjectChild::Signal(SignalChild { arguments, name }));
                            }
                            Keyword::Function => {
                                let name = self.next_id(true)?;
                                self.discard_whitespace();
                                let arguments = self.read_until_depth_runs_out('(', ')')?;
                                self.discard_whitespace();
                                let body = self.read_until_depth_runs_out('{', '}')?;
                                object.children.push(ObjectChild::Function(FunctionChild {
                                    arguments,
                                    name,
                                    body,
                                }));
                            }
                            Keyword::Enum => {
                                let name = self.next_id(true)?;
                                let mut values = Vec::new();
                                let n_lex = self.next_lex()?;
                                match n_lex {
                                    TokenType::Symbol('{') => {}
                                    _ => return error_received_expected!(n_lex, "{"),
                                }

                                loop {
                                    let token = self.next_lex()?;
                                    match token {
                                        TokenType::Symbol('}') => break,
                                        TokenType::Identifier(id) => {
                                            self.discard_whitespace();
                                            if let Some(TokenType::Unknown('=')) =
                                                self.stream.peek()
                                            {
                                                self.stream.next();
                                                let next = self.next_lex()?;
                                                if let TokenType::Number(num) = next {
                                                    values.push((id, Some(num)))
                                                } else {
                                                    return error_received_expected!(
                                                        next, "Number"
                                                    );
                                                }
                                            } else {
                                                values.push((id, None))
                                            }
                                        }
                                        TokenType::Symbol(',') => {}
                                        _ => {
                                            return error_received_expected!(
                                                token,
                                                "Valid enum token"
                                            )
                                        }
                                    }
                                }
                                object
                                    .children
                                    .push(ObjectChild::Enum(EnumChild { name, values }))
                            }
                            Keyword::Component => {
                                let name = self.next_id(true)?;
                                self.discard_whitespace();
                                let next_token = self.next_lex()?;
                                if let TokenType::Symbol(':') = next_token {
                                    let comp_name = self.next_id(true)?;
                                    let obj = self.parse_object(
                                        comp_name,
                                        false,
                                        full_tree_name.clone() + " > " + &name,
                                    )?;
                                    object.children.push(ObjectChild::Component(
                                        ComponentDefinition { name, object: obj },
                                    ));
                                } else {
                                    return error_received_expected!(next_token, ":");
                                }
                            }
                            Keyword::ReadOnly
                            | Keyword::Property
                            | Keyword::Default
                            | Keyword::Required => {
                                // In QML, keywords aren't hard-defined
                                // there can be a field called 'property', which can be assigned
                                self.discard_whitespace();
                                if let Some(TokenType::Symbol(':')) = self.stream.peek() {
                                    object.children.push(self.parse_simple_assignment(
                                        kw.into(),
                                        full_tree_name.clone(),
                                    )?);
                                    continue;
                                }
                                let mut modifiers = Vec::default();
                                modifiers.push(kw);
                                self.discard_whitespace();
                                while let Some(TokenType::Keyword(kw)) = self.stream.peek() {
                                    modifiers.push(kw.clone());
                                    self.stream.next();
                                    self.discard_whitespace();
                                }
                                // Next come the type and name
                                let mut name = self.next_typed_id()?;
                                self.discard_whitespace();
                                let r#type =
                                    if let Some(TokenType::Identifier(_)) = self.stream.peek() {
                                        let r#type = name;
                                        name = self.next_id(true)?;
                                        self.discard_whitespace();
                                        Some(r#type)
                                    } else {
                                        None
                                    };
                                let default_value = match self.stream.peek() {
                                    Some(TokenType::Symbol(':')) => {
                                        self.stream.next(); // Advance past the symbol
                                        Some(self.read_value(full_tree_name.clone())?)
                                    }
                                    _ => None,
                                };
                                match default_value {
                                    Some(AssignmentChildValue::Object(default_object)) => {
                                        object.children.push(ObjectChild::ObjectProperty(
                                            PropertyChild {
                                                name,
                                                default_value: default_object,
                                                modifiers,
                                                r#type,
                                            },
                                        ));
                                    }
                                    _ => {
                                        object.children.push(ObjectChild::Property(
                                            PropertyChild {
                                                name,
                                                default_value,
                                                modifiers,
                                                r#type,
                                            },
                                        ));
                                    }
                                }
                            }
                            _ => {
                                return error_received_expected!(
                                    kw,
                                    "readonly / property / function / signal keywords"
                                )
                            }
                        }
                    }
                    TokenType::Identifier(id) => {
                        object.children.push(self.parse_simple_assignment(
                            id.clone(),
                            full_tree_name.clone() + " > " + &id,
                        )?);
                    }
                    _ => {
                        return error_received_expected!(token, "Valid property starter token");
                    }
                },
                Err(err) => return Err(err),
            }
        }
    }

    fn parse_simple_assignment(&mut self, id: String, parent_name: String) -> Result<ObjectChild> {
        self.discard_whitespace();
        let mut id = self.reread_as_compound_name(id)?;
        self.discard_whitespace();
        // HACK:
        if let Some(TokenType::Identifier(potential_on)) = self.stream.peek() {
            if potential_on == "on" {
                // This is a conditional binding / animation.
                // Swap ids
                self.stream.next();
                id = format!("{} on ", id) + &self.next_id(true)?;
            }
        }
        self.discard_whitespace();
        let next = self.stream.peek();
        match next {
            Some(TokenType::Symbol(':')) => {
                // Simple property assignment
                self.stream.next();
                let value = self.read_value(parent_name)?;
                match value {
                    AssignmentChildValue::Object(obj) => {
                        Ok(ObjectChild::ObjectAssignment(ObjectAssignmentChild {
                            name: id,
                            value: obj,
                        }))
                    }
                    val => Ok(ObjectChild::Assignment(AssignmentChild {
                        name: id,
                        value: val,
                    })),
                }
            }
            Some(TokenType::Symbol('{')) => {
                // Object child
                Ok(ObjectChild::Object(self.parse_object(
                    id,
                    false,
                    parent_name,
                )?))
            }
            _ => error_received_expected!(self.stream.peek(), "item assignment value token"),
        }
    }

    pub fn parse(&mut self) -> Result<QMLTree> {
        self.parse_global_scope()
    }
}
