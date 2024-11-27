use anyhow::{Error, Result};
use std::{collections::HashMap, mem::take};

use crate::parser::diff::parser::{
    Change, FileChangeAction, Insertable, ObjectToChange, ReplaceAction,
};
pub struct Slot {
    contents: Vec<FileChangeAction>,
    locked: bool,
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
                        locked: matches!(&e.destination, ObjectToChange::Template(_)), // Templates are locked slots
                        read_back: false,
                    };
                    self.0.insert(slot_name.clone(), value);
                    created = true;
                }
                let slot = self.0.get_mut(slot_name).unwrap();
                if slot.locked && !created {
                    panic!("Cannot redefine template {}", slot_name);
                }
                slot.contents.extend_from_slice(&e.changes);
                false
            }
        })
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
                e => into.push(e),
            }
        }
    }

    pub fn process_slots(&mut self, changes: &mut Vec<Change>) {
        for change in changes {
            let old = take(&mut change.changes);
            self.expand_slots(old, &mut change.changes);
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
            match content {
                FileChangeAction::Insert(Insertable::Slot(slot_name)) => {
                    self.flatten_slot(slot_name, into, slots_used)?
                }
                FileChangeAction::Insert(Insertable::Code(contents)) => {
                    into.push_str(contents);
                    into.push('\n');
                }
                _ => panic!(),
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
