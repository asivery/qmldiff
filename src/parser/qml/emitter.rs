use std::fmt::Display;

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
                let mut line = emit_property_prologue(&prop);
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
                let mut line = emit_property_prologue(&prop);
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
