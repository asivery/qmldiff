use anyhow::{Error, Result};
use std::{collections::HashMap, mem::take};

use crate::{
    parser::{
        diff::parser::{Change, FileChangeAction, Insertable, ObjectToChange, ReplaceAction},
        qml::{
            emitter::{emit_object, emit_simple_token_stream, flatten_lines},
            lexer::QMLDiffExtensions,
            parser::{AssignmentChildValue, ObjectChild, TreeElement},
        },
    },
    util::common_util::parse_qml,
};
pub struct Slot {
    contents: Vec<FileChangeAction>,
    pub template: bool,
    pub read_back: bool,
}

pub struct Slots(pub HashMap<String, Slot>);

impl Slots {
    pub fn new() -> Self {
        Slots(HashMap::new())
    }
    pub fn update_slots(&mut self, changes: &mut Vec<Change>) {
        changes.retain(|e| match &e.destination {
            ObjectToChange::File(_) => true,
            ObjectToChange::Template(slot_name) | ObjectToChange::Slot(slot_name) => {
                let mut created = false;
                if !self.0.contains_key(slot_name) {
                    let value = Slot {
                        contents: Vec::new(),
                        template: matches!(&e.destination, ObjectToChange::Template(_)), // Templates are locked slots
                        read_back: false,
                    };
                    self.0.insert(slot_name.clone(), value);
                    created = true;
                }
                let slot = self.0.get_mut(slot_name).unwrap();
                if slot.template && !created {
                    panic!("Cannot redefine template {}", slot_name);
                }
                slot.contents.extend_from_slice(&e.changes);
                false
            }
        })
    }

    fn build_template_code(&self, template_name: &String, invocation: &String) -> Result<String> {
        // Merge the template's QML code with the invocation template
        // Then emit the code raw
        // Slots are not supported in templates
        let invocation_tree = {
            let res = parse_qml(
                format!("Object {{ {} }}", invocation),
                Some(QMLDiffExtensions::new(
                    None,
                    None,
                    crate::parser::qml::lexer::ExtensionErrorHandling::ConvertToID,
                )),
                None,
            );
            res
        }?;
        let invocation_tree = match invocation_tree.get(0).unwrap() {
            TreeElement::Object(root) => root,
            _ => panic!(),
        };
        // Go through the entries defined in the invocation. Build slots out of that
        let mut temp_slots = Slots::new();
        macro_rules! insert_or_append {
            ($key: expr, $contents: expr) => {
                if temp_slots.0.contains_key(&$key) {
                    temp_slots
                        .0
                        .get_mut(&$key)
                        .unwrap()
                        .contents
                        .push(FileChangeAction::Insert(Insertable::Code($contents)));
                } else {
                    temp_slots.0.insert(
                        $key.clone(),
                        Slot {
                            contents: vec![FileChangeAction::Insert(Insertable::Code($contents))],
                            template: false,
                            read_back: false,
                        },
                    );
                }
            };
        }

        for child in &invocation_tree.children {
            match child {
                ObjectChild::Assignment(assignment) => {
                    insert_or_append!(assignment.name, match &assignment.value {
                        AssignmentChildValue::Object(_) => {
                            panic!("Only simple assignments are supported")
                        }
                        AssignmentChildValue::Other(stream) => {
                            emit_simple_token_stream(stream)
                        }
                    });
                }
                ObjectChild::ObjectAssignment(assignment) => {
                    insert_or_append!(assignment.name, flatten_lines(&emit_object(&assignment.value, 0)));
                }
                _ => return Err(Error::msg(
                    "Cannot process template invocation. Only simple / object assignments are supported.",
                )),
            }
        }

        let emited_template = {
            let mut slots_to_update = Vec::new();
            let slot_ref = self.0.get(template_name).unwrap();
            if !slot_ref.template {
                panic!("Cannot insert a slot as template!");
            }
            let string_contents = match &slot_ref.contents[0] {
                FileChangeAction::Insert(Insertable::Code(c)) => c,
                _ => panic!("Cannot happen."),
            };
            let res = parse_qml(
                string_contents.clone(),
                Some(QMLDiffExtensions::new(
                    None,
                    Some(&temp_slots),
                    crate::parser::qml::lexer::ExtensionErrorHandling::ConvertToID,
                )),
                Some(&mut slots_to_update),
            );
            if slots_to_update.len() != temp_slots.0.len() {
                panic!("Error inserting a template - not all values used!")
            }
            res
        }?;
        let contents_qml_object = match emited_template.get(0).unwrap() {
            TreeElement::Object(root) => root,
            _ => panic!(),
        };

        Ok(flatten_lines(&emit_object(contents_qml_object, 0)))
    }

    pub fn expand_templates(
        &mut self,
        input: Vec<FileChangeAction>,
        into: &mut Vec<FileChangeAction>,
    ) {
        for e in input {
            match e {
                FileChangeAction::Replace(r_action)
                    if matches!(&r_action.content, Insertable::Template(_, _)) =>
                {
                    // HACK
                    let (template_name, invocation) = match &r_action.content {
                        Insertable::Template(a, b) => (a, b),
                        _ => panic!(),
                    };
                    if let Some(slot_contents) = self.0.get_mut(template_name) {
                        slot_contents.read_back = true;
                    }

                    into.push(FileChangeAction::Replace(ReplaceAction {
                        selector: r_action.selector,
                        content: Insertable::Code(
                            self.build_template_code(template_name, invocation).unwrap(),
                        ),
                    }));
                }
                FileChangeAction::Insert(Insertable::Template(template_name, invocation)) => {
                    if let Some(slot_contents) = self.0.get_mut(&template_name) {
                        slot_contents.read_back = true;
                    }
                    into.push(FileChangeAction::Insert(Insertable::Code(
                        self.build_template_code(&template_name, &invocation)
                            .unwrap(),
                    )))
                }
                e => into.push(e),
            }
        }
    }

    pub fn expand_slots(&mut self, input: Vec<FileChangeAction>, into: &mut Vec<FileChangeAction>) {
        for e in input {
            match e {
                FileChangeAction::Replace(r_action)
                    if matches!(&r_action.content, Insertable::Slot(_)) =>
                {
                    // HACK
                    let slot = match &r_action.content {
                        Insertable::Slot(s) => s,
                        _ => panic!(),
                    };
                    let mut all_insertions = vec![];
                    if let Some(slot_contents) = self.0.get_mut(slot) {
                        if slot_contents.template {
                            panic!("Cannot insert a template as a slot!");
                        }
                        slot_contents.read_back = true;
                    }
                    if let Some(slot_contents) = self.0.get(slot) {
                        self.expand_slots(slot_contents.contents.clone(), &mut all_insertions);
                    }
                    let qml_code_str = all_insertions
                        .into_iter()
                        .map(|e| match e {
                            FileChangeAction::Insert(Insertable::Code(raw_code)) => raw_code,
                            _ => panic!(),
                        })
                        .collect::<String>();
                    into.push(FileChangeAction::Replace(ReplaceAction {
                        selector: r_action.selector,
                        content: Insertable::Code(qml_code_str),
                    }));
                }
                FileChangeAction::Insert(Insertable::Slot(slot)) => {
                    if let Some(slot_contents) = self.0.get_mut(&slot) {
                        slot_contents.read_back = true;
                    }
                    if let Some(slot_contents) = self.0.get(&slot) {
                        self.expand_slots(slot_contents.contents.clone(), into);
                    }
                }
                FileChangeAction::Insert(Insertable::Template(name, invocation)) => {
                    into.push(FileChangeAction::Insert(Insertable::Code(
                        self.build_template_code(&name, &invocation).unwrap(),
                    )));
                }
                e => into.push(e),
            }
        }
    }

    pub fn process_slots(&mut self, changes: &mut Vec<Change>) {
        for change in changes {
            let old = take(&mut change.changes);
            let mut temp_holder = Vec::new();
            self.expand_templates(old, &mut temp_holder);
            self.expand_slots(temp_holder, &mut change.changes);
        }
    }

    pub fn all_read_back(&self) -> bool {
        !self.0.iter().any(|x| !x.1.read_back)
    }

    fn flatten_slot(
        &self,
        name: &str,
        into: &mut String,
        slots_used: &mut Vec<String>,
    ) -> Result<()> {
        let slot_contents = match self.0.get(name) {
            None => return Err(Error::msg(format!("Cannot find slot {}", name))),
            Some(e) => e,
        };

        slots_used.push(String::from(name));

        for content in &slot_contents.contents {
            if let FileChangeAction::Insert(x) = content {
                match x {
                    Insertable::Slot(slot_name) => {
                        self.flatten_slot(slot_name, into, slots_used)?
                    }
                    Insertable::Code(contents) => {
                        into.push_str(contents);
                        into.push('\n');
                    }
                    Insertable::Template(name, invocation) => {
                        into.push_str(&self.build_template_code(name, invocation).unwrap());
                        into.push('\n');
                    }
                }
            } else {
                panic!();
            };
        }

        Ok(())
    }

    pub fn resolve_slot_final_state(&self, name: &str) -> Result<(String, Vec<String>)> {
        let mut string = String::new();
        let mut slots_used = Vec::new();
        self.flatten_slot(name, &mut string, &mut slots_used)?;

        Ok((string, slots_used))
    }
}
