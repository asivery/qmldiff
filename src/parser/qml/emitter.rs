use std::fmt::Display;

use crate::parser::qml::lexer::Keyword;

use super::{
    lexer::TokenType,
    parser::{
        AssignmentChildValue, Import, Object, ObjectChild, Pragma, PropertyChild, TreeElement,
    },
};

#[derive(Debug, Clone)]
pub struct Line {
    pub text: String,
    pub indent: usize,
}

const INDENT_DEPTH: usize = 4;

impl Display for Line {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&String::from(' ').repeat(INDENT_DEPTH * self.indent))?;
        f.write_str(&self.text)
    }
}

impl Line {
    fn linearize(
        string: &str,
        indent: usize,
        prefix: Option<String>,
        sufix: Option<String>,
    ) -> Vec<Line> {
        let prefix = prefix.unwrap_or_default();
        let suffix = sufix.unwrap_or_default();
        string
            .split('\n')
            .map(|e: &str| Line {
                text: format!("{}{}{}", &prefix, e, &suffix),
                indent,
            })
            .collect()
    }

    fn empty() -> Self {
        Self {
            text: String::new(),
            indent: 0,
        }
    }
}

fn emit_import(import: &Import) -> Line {
    let mut string: String = String::from("import ");
    string += &import.object_name;
    if let Some(version) = &import.version {
        string += " ";
        string += version;
    }
    if let Some(alias) = &import.alias {
        string += " as ";
        string += alias;
    }

    Line {
        text: string,
        indent: 0,
    }
}

fn emit_pragma(pragma: &Pragma) -> Line {
    Line {
        text: format!("pragma {}", pragma.pragma),
        indent: 0,
    }
}

pub fn emit_simple_token_stream(stream: &Vec<TokenType>) -> String {
    let mut string = "".to_string();
    for entry in stream {
        string += &entry.to_string();
    }

    string
}

pub fn emit_token_stream(stream: &Vec<TokenType>, indent: usize) -> Vec<Line> {
    let mut lines = vec![Line {
        text: String::new(),
        indent,
    }];
    for token in stream {
        let last = &mut lines.last_mut().unwrap().text;
        let next = Line::linearize(&token.to_string(), indent, None, None);
        last.push_str(&next[0].text);
        lines.extend_from_slice(&next[1..]);
    }

    lines
}

fn emit_assignment_child_value(value: &AssignmentChildValue, indent: usize) -> Vec<Line> {
    match value {
        AssignmentChildValue::Other(stream) => emit_token_stream(stream, indent),
        AssignmentChildValue::Object(object) => emit_object(object, indent),
        // AssignmentChildValue::List(list) => {
        //     let mut temporary_lines = vec![Line {
        //         text: String::from("["),
        //         indent,
        //     }];
        //     for child in list {
        //         let mut emited_child = emit_assignment_child_value(child, indent + 1);
        //         emited_child.last_mut().unwrap().text.push(',');
        //         temporary_lines.extend(emited_child);
        //     }
        //     temporary_lines.push(Line {
        //         text: "]".into(),
        //         indent,
        //     });
        //     temporary_lines
        // }
    }
}

fn _emit_object_to_token_stream(object: &Object, stream: &mut Vec<TokenType>, only_body: bool) {
    macro_rules! add {
        ($a: expr) => {
            stream.push($a);
            stream.push(TokenType::Whitespace(" ".into()));
        };
    }
    macro_rules! id {
        ($a: expr) => {
            add!(TokenType::Identifier($a));
        };
    }
    macro_rules! nl {
        () => {
            add!(TokenType::NewLine(0));
        };
    }
    macro_rules! emit_token_stream_property_prologue {
        ($prop: expr) => {
            for modifier in &$prop.modifiers {
                add!(TokenType::Keyword(modifier.clone()));
            }
            if let Some(r#type) = &$prop.r#type {
                add!(TokenType::Identifier(r#type.clone()));
            }
            add!(TokenType::Identifier($prop.name.clone()));
        };
    }

    if !only_body {
        id!(object.name.clone());
        add!(TokenType::Symbol('{'));
        nl!();
    }
    for child in &object.children {
        match child {
            ObjectChild::Abstract(_) => panic!("Cannot emit abstract to token stream!"),
            ObjectChild::ObjectAssignment(assignment) => {
                id!(assignment.name.clone());
                add!(TokenType::Symbol(':'));
                _emit_object_to_token_stream(&assignment.value, stream, false);
            }
            ObjectChild::Assignment(assignment) => {
                // HACK: See comment in parser:
                if assignment.name.contains(" ") {
                    for e in assignment.name.split(" ") {
                        id!(e.into());
                    }
                } else {
                    id!(assignment.name.clone());
                }
                add!(TokenType::Symbol(':'));
                match &assignment.value {
                    AssignmentChildValue::Object(obj) => {
                        _emit_object_to_token_stream(obj, stream, false);
                    }
                    AssignmentChildValue::Other(other) => stream.extend_from_slice(other),
                }
            }
            ObjectChild::Enum(r#enum) => {
                add!(TokenType::Keyword(Keyword::Enum));
                id!(r#enum.name.clone());
                add!(TokenType::Symbol('{'));
                nl!();
                for val in &r#enum.values {
                    id!(val.0.clone());
                    if let Some(value) = val.1 {
                        add!(TokenType::Symbol('='));
                        add!(TokenType::Number(value));
                    }
                    nl!();
                }
                add!(TokenType::Symbol('}'));
            }
            ObjectChild::Function(function) => {
                add!(TokenType::Keyword(Keyword::Function));
                id!(function.name.clone());
                stream.extend_from_slice(&function.arguments);
                stream.extend_from_slice(&function.body);
            }
            ObjectChild::Object(object) => {
                _emit_object_to_token_stream(object, stream, false);
            }
            ObjectChild::Property(prop) => {
                emit_token_stream_property_prologue!(prop);
                match &prop.default_value {
                    Some(AssignmentChildValue::Object(obj)) => {
                        add!(TokenType::Symbol(':'));
                        _emit_object_to_token_stream(obj, stream, false);
                    }
                    Some(AssignmentChildValue::Other(ts)) => {
                        add!(TokenType::Symbol(':'));
                        stream.extend_from_slice(ts);
                    }
                    None => {}
                }
            }
            ObjectChild::ObjectProperty(prop) => {
                emit_token_stream_property_prologue!(prop);
                add!(TokenType::Symbol(':'));
                _emit_object_to_token_stream(&prop.default_value, stream, false);
            }
            ObjectChild::Signal(sig) => {
                add!(TokenType::Keyword(Keyword::Signal));
                id!(sig.name.clone());
                if let Some(ts) = &sig.arguments {
                    stream.extend_from_slice(ts)
                }
            }
            ObjectChild::Component(comp) => {
                add!(TokenType::Keyword(Keyword::Component));
                add!(TokenType::Symbol(':'));
                _emit_object_to_token_stream(&comp.object, stream, false);
            }
        }
        nl!();
    }
    if !only_body {
        add!(TokenType::Symbol('}'));
    }
}

pub fn emit_object_to_token_stream(object: &Object, only_body: bool) -> Vec<TokenType> {
    let mut stream = vec![];
    _emit_object_to_token_stream(object, &mut stream, only_body);
    stream
}

fn emit_property_prologue<T>(prop: &PropertyChild<T>) -> String {
    let modifiers: String = prop
        .modifiers
        .iter()
        .map(|k| Into::<String>::into(k.clone()))
        .fold(String::new(), |a, b| a + &b + " ");
    if let Some(r#type) = &prop.r#type {
        format!("{} {} {}", modifiers, r#type, prop.name)
    } else {
        format!("{} {}", modifiers, prop.name)
    }
}

pub fn emit_object(object: &Object, indent: usize) -> Vec<Line> {
    let root_line = Line {
        text: format!("{} {{", object.name),
        indent,
    };
    let indent = indent + 1;
    let mut lines = vec![root_line];

    for child in &object.children {
        match child {
            ObjectChild::Abstract(r#abstract) => lines.extend(r#abstract.emit(indent)),
            ObjectChild::ObjectAssignment(assignment) => {
                let value_emited = emit_object(&assignment.value, indent);
                let new_first_line = Line {
                    text: format!(
                        "{}: {}",
                        &assignment.name,
                        value_emited.first().unwrap().text
                    ),
                    indent,
                };
                lines.push(new_first_line);
                lines.extend_from_slice(&value_emited[1..]);
            }
            ObjectChild::Assignment(assignment) => {
                let value_emited = emit_assignment_child_value(&assignment.value, indent);
                let new_first_line = Line {
                    text: format!(
                        "{}: {}",
                        &assignment.name,
                        value_emited.first().unwrap().text
                    ),
                    indent,
                };
                lines.push(new_first_line);
                lines.extend_from_slice(&value_emited[1..]);
            }
            ObjectChild::Enum(r#enum) => {
                lines.push(Line {
                    indent,
                    text: format!("enum {} {{", r#enum.name),
                });
                let length = r#enum.values.len();
                for (i, val) in r#enum.values.iter().enumerate() {
                    let mut text = if let Some(value) = val.1 {
                        format!("{} = {}", val.0, value)
                    } else {
                        val.0.to_string()
                    };

                    if i < length - 1 {
                        text.push(',');
                    }

                    lines.push(Line {
                        indent: indent + 1,
                        text,
                    });
                }
                lines.push(Line {
                    indent,
                    text: String::from("}"),
                });
            }
            ObjectChild::Function(function) => {
                let mut sub_lines = vec![Line {
                    text: format!("function {}", function.name),
                    indent,
                }];
                let arg_stream = emit_token_stream(&function.arguments, indent + 1);
                sub_lines.last_mut().unwrap().text += &arg_stream[0].text;
                sub_lines.extend_from_slice(&arg_stream[1..]);
                let func_stream = emit_token_stream(&function.body, 0);
                sub_lines.last_mut().unwrap().text += &func_stream[0].text;
                sub_lines.extend_from_slice(&func_stream[1..]);
                lines.extend(sub_lines);
            }
            ObjectChild::Object(object) => {
                lines.extend(emit_object(object, indent));
            }
            ObjectChild::Property(prop) => {
                let mut line = emit_property_prologue(prop);
                if let Some(default) = &prop.default_value {
                    let new_lines = emit_assignment_child_value(default, indent);
                    line += ": ";
                    line += &new_lines[0].text;
                    lines.push(Line { text: line, indent });
                    lines.extend_from_slice(&new_lines[1..]);
                } else {
                    lines.push(Line { text: line, indent });
                }
            }
            ObjectChild::ObjectProperty(prop) => {
                let mut line = emit_property_prologue(prop);
                let new_lines = emit_object(&prop.default_value, indent);
                line += ": ";
                line += &new_lines[0].text;
                lines.push(Line { text: line, indent });
                lines.extend_from_slice(&new_lines[1..]);
            }
            ObjectChild::Signal(sig) => {
                let mut line = format!("signal {}", sig.name);
                if let Some(args) = &sig.arguments {
                    let n = emit_token_stream(args, indent);
                    line += &n[0].text;
                    lines.push(Line { text: line, indent });
                    lines.extend_from_slice(&n[1..]);
                } else {
                    lines.push(Line { text: line, indent });
                }
            }
            ObjectChild::Component(comp) => {
                let mut sub_lines = vec![Line {
                    text: format!("component {}: ", comp.name),
                    indent,
                }];
                let arg_stream = emit_object(&comp.object, indent + 1);
                sub_lines.last_mut().unwrap().text += &arg_stream[0].text;
                sub_lines.extend_from_slice(&arg_stream[1..]);
                lines.extend(sub_lines);
            }
        }

        lines.push(Line::empty());
    }

    lines.push(Line {
        text: "}".into(),
        indent: indent - 1,
    });

    lines
}

pub fn emit(objects: &Vec<TreeElement>) -> Vec<Line> {
    let mut lines = Vec::default();
    for obj in objects {
        match obj {
            TreeElement::Import(import) => lines.push(emit_import(import)),
            TreeElement::Pragma(pragma) => lines.push(emit_pragma(pragma)),
            TreeElement::Object(obj) => lines.extend(emit_object(obj, 0)),
        }
    }

    lines
}

pub fn flatten_lines(lines: &[Line]) -> String {
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| (if i == 0 { "" } else { "\n" }).to_string() + &l.to_string())
        .collect()
}

pub fn emit_string(objects: &Vec<TreeElement>) -> String {
    flatten_lines(&emit(objects))
}
