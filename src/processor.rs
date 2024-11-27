use std::cell::RefCell;
use std::rc::Rc;

use crate::parser::diff::lexer::Keyword;
use crate::parser::diff::parser::{
    FileChangeAction, Insertable, Location, LocationSelector, ObjectToChange,
};
use crate::parser::diff::parser::{NodeSelector, NodeTree, PropRequirement};
use crate::parser::qml::lexer::QMLDiffExtensions;
use crate::parser::qml::parser::{Import, ObjectChild, TreeElement};
use crate::refcell_translation::{
    translate_object_child, TranslatedEnumChild, TranslatedObject, TranslatedObjectAssignmentChild,
    TranslatedObjectChild, TranslatedObjectRef, TranslatedTree,
};

use anyhow::{Error, Result};

use crate::parser::diff::parser::Change;
use crate::parser::qml;

pub fn find_and_process(
    file_name: &str,
    qml: &mut TranslatedTree,
    diffs: &Vec<Change>,
    extended_features: QMLDiffExtensions,
    slots_used: &mut Vec<String>,
) -> Result<()> {
    for diff in diffs {
        match &diff.destination {
            ObjectToChange::File(f) if f == file_name => {
                process(qml, diff, extended_features.clone(), slots_used)?;
            }
            _ => {}
        }
    }

    Ok(())
}

fn does_match(
    object: &TranslatedObject,
    sel: &NodeSelector,
    object_named: Option<&String>,
) -> bool {
    if sel.object_name != object.name {
        return false;
    }
    if sel.named.is_some() && object_named != sel.named.as_ref() {
        return false;
    }
    let children_names: Vec<Option<&String>> =
        object.children.iter().map(|e| e.get_name()).collect();

    for (name, requirement) in &sel.props {
        if let Some(index) = children_names.iter().position(|e| e == &Some(name)) {
            match requirement {
                PropRequirement::Exists => {} // Checked already.
                PropRequirement::Equals(eq) => {
                    let child = object.children.get(index).unwrap();
                    if let Some(value) = child.get_str_value() {
                        if value != *eq {
                            return false;
                        }
                    }
                }
                PropRequirement::Contains(eq) => {
                    let child = object.children.get(index).unwrap();
                    if let Some(value) = child.get_str_value() {
                        if value.contains(eq) {
                            return false;
                        }
                    }
                }
            }
        } else {
            return false; // All conditions demand existence of the child.
        }
    }

    true
}

#[derive(Debug, Clone)]
enum TreeRoot {
    Object(TranslatedObjectRef),
    Enum(TranslatedEnumChild),
}

fn locate_in_tree(roots: Vec<TreeRoot>, tree: &NodeTree) -> Vec<TreeRoot> {
    let mut potential_roots = roots; // Start with the initial root
    for sel in tree {
        let mut swap_root = Vec::new();
        for r in &potential_roots {
            // Borrow each potential root mutably for children traversal
            if let TreeRoot::Object(r) = r {
                for child in &r.borrow().children {
                    let child_object = match child {
                        TranslatedObjectChild::Object(obj) => {
                            Some((None, TreeRoot::Object(obj.clone())))
                        }
                        TranslatedObjectChild::Component(asi)
                        | TranslatedObjectChild::ObjectAssignment(asi) => {
                            Some((Some(&asi.name), TreeRoot::Object(asi.value.clone())))
                        }
                        TranslatedObjectChild::Enum(enu) => {
                            Some((Some(&enu.name), TreeRoot::Enum(enu.clone())))
                        }
                        _ => None,
                    };

                    if let Some((name, object)) = child_object {
                        match &object {
                            TreeRoot::Object(obj) => {
                                if does_match(&obj.borrow(), sel, name) {
                                    swap_root.push(object); // Collect the matched child object
                                }
                            }
                            TreeRoot::Enum(r#enum) => {
                                if sel.is_simple() && sel.object_name == r#enum.name {
                                    swap_root.push(object);
                                }
                            }
                        }
                    }
                }
            }
        }
        potential_roots = swap_root; // Update the list of potential roots for the next iteration
    }

    potential_roots
}

#[derive(Clone, Debug)]
struct RootReference {
    pub root: Vec<TreeRoot>,
    pub cursor: Option<usize>,
}

fn find_first_matching_child(root: &TreeRoot, tree: &Vec<NodeSelector>) -> Result<usize> {
    match root {
        TreeRoot::Object(root) => {
            for (i, child) in root.borrow().children.iter().enumerate() {
                if tree.len() == 1 {
                    let selector = &tree[0];
                    if selector.is_simple() {
                        // Might be a generic prop.
                        if child.get_name() == Some(&selector.object_name) {
                            return Ok(i);
                        }
                    }
                }

                match child {
                    TranslatedObjectChild::Object(obj) => {
                        if !locate_in_tree(
                            vec![TreeRoot::Object(Rc::new(RefCell::new(TranslatedObject {
                                name: String::default(),
                                full_name: String::default(),
                                children: vec![TranslatedObjectChild::Object(obj.clone())],
                            })))],
                            tree,
                        )
                        .is_empty()
                        {
                            return Ok(i);
                        }
                    }
                    TranslatedObjectChild::Component(obj)
                    | TranslatedObjectChild::ObjectAssignment(obj) => {
                        if !locate_in_tree(
                            vec![TreeRoot::Object(Rc::new(RefCell::new(TranslatedObject {
                                name: String::default(),
                                full_name: String::default(),
                                children: vec![TranslatedObjectChild::ObjectAssignment(
                                    TranslatedObjectAssignmentChild {
                                        name: obj.name.clone(),
                                        value: obj.value.clone(),
                                    },
                                )],
                            })))],
                            tree,
                        )
                        .is_empty()
                        {
                            return Ok(i);
                        }
                    }
                    _ => {}
                }
            }
        }
        TreeRoot::Enum(r#enum) if tree.len() == 1 && tree[0].is_simple() => {
            for (i, value) in r#enum.values.borrow().iter().enumerate() {
                if value.0 == tree[0].object_name {
                    return Ok(i);
                }
            }
        }
        _ => {}
    }

    Err(Error::msg(format!("Cannot LOCATE {:?} in root {:?}", tree, root)))
}

fn insert_into_root(
    root_cursor: &mut usize,
    root: &TreeRoot,
    code: &String,
    extended_features: QMLDiffExtensions,
    slots_used: &mut Vec<String>,
) -> Result<()> {
    let raw_qml = if matches!(root, TreeRoot::Object(_)) {
        format!("Object {{ {} }}", code)
    } else {
        format!("Object {{ enum Enum {{ {} }} }} ", code)
    };
    // Start the QML parser...
    let token_stream = qml::lexer::Lexer::new(raw_qml, Some(extended_features), Some(slots_used));
    let tokens: Vec<qml::lexer::TokenType> = token_stream.collect();
    let mut parser = qml::parser::Parser::new(Box::new(tokens.into_iter()));
    let mut qml_root = parser.parse()?;
    if let Some(TreeElement::Object(object)) = qml_root.pop() {
        match root {
            TreeRoot::Object(root) => {
                // Merge the children!
                for child in object.children {
                    root.borrow_mut()
                        .children
                        .insert(*root_cursor, translate_object_child(child));
                    *root_cursor += 1;
                }
            }
            TreeRoot::Enum(r#enum) => {
                if object.children.len() != 1 {
                    return Err(Error::msg("Internal error"));
                }
                if let ObjectChild::Enum(enum_child) = &object.children[0] {
                    r#enum
                        .values
                        .borrow_mut()
                        .extend_from_slice(&enum_child.values);
                }
            }
        }
    } else {
        return Err(Error::msg("Internal parse error"));
    }
    Ok(())
}

pub fn process(
    absolute_root: &mut TranslatedTree,
    diff: &Change,
    extended_features: QMLDiffExtensions,
    slots_used: &mut Vec<String>,
) -> Result<()> {
    let mut root_stack: Vec<RootReference> = Vec::new();
    let mut current_root = RootReference {
        root: vec![TreeRoot::Object(absolute_root.root.clone())],
        cursor: None,
    }; // Start with root as the current root

    macro_rules! unambiguous_root {
        () => {{
            if current_root.root.len() != 1 {
                return Err(Error::msg(format!(
                    "Root must be unambiguous! (Right now {} elements matched)",
                    current_root.root.len()
                )));
            }
            &current_root.root[0]
        }};
    }

    macro_rules! unambiguous_root_cursor_set {
        () => {{
            let reference = unambiguous_root!();
            if let Some(cursor) = current_root.cursor {
                (reference, cursor)
            } else {
                return Err(Error::msg(
                    "Cursor not set! Use the LOCATE or REPLACE directive first.",
                ));
            }
        }};
    }

    for change in &diff.changes {
        match change {
            FileChangeAction::End(Keyword::Traverse) => {
                // Pop the last object from the stack to return to the previous root
                if let Some(root) = root_stack.pop() {
                    current_root = root;
                } else {
                    return Err(Error::msg("Cannot END TRAVERSE - end of scope!"));
                }
            }
            FileChangeAction::Traverse(tree) => {
                // Attempt to locate the child object in the current root
                let object = locate_in_tree(current_root.root.clone(), tree);
                if object.is_empty() {
                    return Err(Error::msg(format!(
                        "Cannot locate element in tree: {}",
                        tree.iter()
                            .map(|e| e.to_string())
                            .collect::<Vec<String>>()
                            .join(" > ")
                    )));
                }

                // Push the current root onto the stack and set the new current root
                root_stack.push(current_root);
                current_root = RootReference {
                    root: object,
                    cursor: None,
                };
            }
            FileChangeAction::Assert(tree_selector) => {
                current_root.root.retain(|e| {
                    // Is the tree selector simple
                    if tree_selector.len() == 1 && tree_selector[0].is_simple() {
                        match &e {
                            TreeRoot::Object(e) => {
                                for child_object in &e.borrow().children {
                                    // Yes, and it matches
                                    if child_object.get_name()
                                        == Some(&tree_selector[0].object_name)
                                    {
                                        return true;
                                    }
                                }
                            }
                            TreeRoot::Enum(e) => {
                                for value in e.values.borrow().iter() {
                                    if value.0 == tree_selector[0].object_name {
                                        return true;
                                    }
                                }
                            }
                        }
                    }
                    !locate_in_tree(vec![e.clone()], tree_selector).is_empty()
                });
                if current_root.root.is_empty() {
                    return Err(Error::msg("ASSERTed all objects out of existence"));
                }
            }
            FileChangeAction::Insert(insertable) => {
                // Object starts with { -> To convert into Object, concat with "Object"
                if let Some(code) = match insertable {
                    Insertable::Code(code) => Some(code),
                    Insertable::Slot(_) => {
                        panic!("Cannot insert slot! Use `process_slots()` first!")
                    }
                } {
                    let (root, mut cursor) = unambiguous_root_cursor_set!();
                    insert_into_root(
                        &mut cursor,
                        root,
                        code,
                        extended_features.clone(),
                        slots_used,
                    )?;
                    current_root.cursor = Some(cursor);
                }
            }
            FileChangeAction::Locate(location) => {
                let root = unambiguous_root!();
                current_root.cursor = Some(match &location.selector {
                    LocationSelector::All => match location.location {
                        Location::Before => 0,
                        Location::After => match root {
                            TreeRoot::Enum(r#enum) => r#enum.values.borrow().len(),
                            TreeRoot::Object(root) => root.borrow().children.len(),
                        },
                    },
                    LocationSelector::Tree(tree) => {
                        let element_idx = find_first_matching_child(root, tree)?;

                        match location.location {
                            Location::After => element_idx + 1,
                            Location::Before => element_idx,
                        }
                    }
                });
            }
            FileChangeAction::Replace(replacer) => {
                let root = unambiguous_root!();
                let mut element_idx = find_first_matching_child(root, &replacer.selector)?;
                match root {
                    TreeRoot::Object(obj) => {
                        obj.borrow_mut().children.remove(element_idx);
                    }
                    TreeRoot::Enum(r#enum) => {
                        r#enum.values.borrow_mut().remove(element_idx);
                    }
                };
                insert_into_root(
                    &mut element_idx,
                    root,
                    match &replacer.content {
                        Insertable::Code(code) => code,
                        Insertable::Slot(_) => {
                            panic!("Cannot insert slot! Use `process_slots()` first!")
                        }
                    },
                    extended_features.clone(),
                    slots_used,
                )?;
                current_root.cursor = Some(element_idx);
            }
            FileChangeAction::Rename(rename) => {
                let root = unambiguous_root!();
                let element_idx = find_first_matching_child(root, &rename.selector)?;
                match root {
                    TreeRoot::Enum(_) => return Err(Error::msg("Cannot RENAME a value within an enum!")),
                    TreeRoot::Object(obj) => {
                        obj.borrow_mut().children[element_idx].set_name(rename.name_to.clone())?;
                    }
                }
                current_root.cursor = Some(element_idx+1);
            }
            FileChangeAction::Remove(selector) => {
                // Root must be unambiguous
                match unambiguous_root!() {
                    TreeRoot::Object(obj) => {
                        obj.borrow_mut().children.retain(|e| {
                            if selector.is_simple() {
                                // Might be a generic prop.
                                if e.get_name() == Some(&selector.object_name) {
                                    return false;
                                }
                            }

                            // Complex object. Delve deeper.
                            match e {
                                TranslatedObjectChild::Object(e) => {
                                    !does_match(&e.borrow(), selector, None)
                                }
                                TranslatedObjectChild::ObjectAssignment(e) => {
                                    !does_match(&e.value.borrow(), selector, Some(&e.name))
                                }
                                _ => true, // Retain all else!
                            }
                        });
                    }
                    TreeRoot::Enum(r#enum) => {
                        if !selector.is_simple() {
                            return Err(Error::msg("Cannot do precision removal in enum."));
                        }
                        r#enum
                            .values
                            .borrow_mut()
                            .retain(|e| e.0 != selector.object_name);
                    }
                }
            }
            FileChangeAction::AddImport(import) => {
                if !root_stack.is_empty() {
                    return Err(Error::msg(
                        "Cannot use import within TRAVERSE / SLOT statements!",
                    ));
                }
                absolute_root.leftovers.push(TreeElement::Import(Import {
                    alias: import.alias.clone(),
                    object_name: import.name.clone(),
                    version: Some(import.version.clone()),
                }));
            }
            _ => return Err(Error::msg("Not supported yet")),
        }
    }

    Ok(())
}
