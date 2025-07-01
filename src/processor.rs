use std::cell::RefCell;
use std::mem::take;
use std::rc::Rc;
use std::usize;

use crate::parser::common::IteratorPipeline;
use crate::parser::diff::lexer::Keyword;
use crate::parser::diff::parser::{
    FileChangeAction, Insertable, LocateRebuildActionSelector, Location, LocationSelector,
    ObjectToChange, RebuildAction, RebuildInstruction, RemoveRebuildAction,
    ReplaceRebuildActionWhat,
};
use crate::parser::diff::parser::{NodeSelector, NodeTree, PropRequirement};
use crate::parser::qml::emitter::emit_object_to_token_stream;
use crate::parser::qml::lexer::TokenType;
use crate::parser::qml::parser::{AssignmentChildValue, Import, Object, ObjectChild, TreeElement};
use crate::parser::qml::slot_extensions::QMLSlotRemapper;
use crate::refcell_translation::{
    translate, translate_object_child, untranslate, untranslate_object_child, TranslatedEnumChild,
    TranslatedObject, TranslatedObjectAssignmentChild, TranslatedObjectChild, TranslatedObjectRef,
    TranslatedTree,
};
use crate::slots::Slots;
use crate::util::common_util::{parse_qml_from_chain, parse_qml_into_simple_object};

use anyhow::{Error, Result};

use crate::parser::diff::parser::Change;

macro_rules! traverse_no_raw_children {
    () => {
        unreachable!("TRAVERSE does not allow raw children as roots!")
    };
}

pub fn find_and_process(
    file_name: &str,
    qml: &mut TranslatedTree,
    diffs: &Vec<Change>,
    slots: &mut Slots,
) -> Result<()> {
    for diff in diffs {
        match &diff.destination {
            ObjectToChange::File(f) if f == file_name => match process(qml, diff, slots) {
                Ok(_) => {}
                Err(error) => {
                    return Err(Error::msg(format!(
                        "(On behalf of '{}'): {:?}",
                        diff.source, error
                    )))
                }
            },
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
                        if !value.contains(eq) {
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
    Child {
        parent: TranslatedObjectRef,
        child_index: usize,
    },
}

fn locate_in_tree(
    roots: Vec<TreeRoot>,
    tree: &NodeTree,
    force_raw_children: bool,
) -> Vec<TreeRoot> {
    let mut potential_roots = roots; // Start with the initial root
    for sel in tree {
        let mut swap_root = Vec::new();
        for (i, r) in potential_roots.iter().enumerate() {
            let is_last = i == potential_roots.len() - 1;
            // Borrow each potential root mutably for children traversal
            if let TreeRoot::Object(r) = r {
                for (i, child) in r.borrow().children.iter().enumerate() {
                    let child_object = match child {
                        TranslatedObjectChild::Object(obj) => {
                            Some((None, TreeRoot::Object(obj.clone())))
                        }
                        TranslatedObjectChild::ObjectProperty(obj) => {
                            Some((Some(&obj.name), TreeRoot::Object(obj.default_value.clone())))
                        }
                        TranslatedObjectChild::Component(asi)
                        | TranslatedObjectChild::ObjectAssignment(asi) => {
                            Some((Some(&asi.name), TreeRoot::Object(asi.value.clone())))
                        }
                        TranslatedObjectChild::Enum(enu) => {
                            Some((Some(&enu.name), TreeRoot::Enum(enu.clone())))
                        }
                        _ if force_raw_children => Some((
                            child.get_name(),
                            TreeRoot::Child {
                                child_index: i,
                                parent: r.clone(),
                            },
                        )),
                        _ => None,
                    };

                    if let Some((name, object)) = child_object {
                        match &object {
                            TreeRoot::Object(obj) => {
                                if does_match(&obj.borrow(), sel, name) {
                                    // Collect the matched child object
                                    if force_raw_children && is_last {
                                        swap_root.push(TreeRoot::Child {
                                            parent: r.clone(),
                                            child_index: i,
                                        });
                                    } else {
                                        swap_root.push(object);
                                    }
                                }
                            }
                            TreeRoot::Enum(r#enum) => {
                                if sel.is_simple() && sel.object_name == r#enum.name {
                                    if force_raw_children && is_last {
                                        swap_root.push(TreeRoot::Child {
                                            parent: r.clone(),
                                            child_index: i,
                                        });
                                    } else {
                                        swap_root.push(object);
                                    }
                                }
                            }
                            TreeRoot::Child {
                                parent: _,
                                child_index: _,
                            } => {
                                if let Some(name) = name {
                                    if sel.is_simple() && sel.object_name == *name {
                                        swap_root.push(object);
                                    }
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
    pub is_replicating: bool,
}

fn tree_to_string(tree: &NodeTree) -> String {
    tree.iter()
        .map(|e| e.to_string())
        .collect::<Vec<String>>()
        .join(" > ")
}

fn find_first_matching_child(root: &TreeRoot, tree: &Vec<NodeSelector>) -> Result<usize> {
    macro_rules! make_tree_return_i {
        ($i: expr, $obj: expr, $name: expr) => {
            if !locate_in_tree(
                vec![TreeRoot::Object(Rc::new(RefCell::new(TranslatedObject {
                    name: String::default(),
                    full_name: String::default(),
                    children: vec![TranslatedObjectChild::ObjectAssignment(
                        TranslatedObjectAssignmentChild {
                            name: $name.clone(),
                            value: $obj.clone(),
                        },
                    )],
                })))],
                tree,
                false,
            )
            .is_empty()
            {
                return Ok($i);
            }
        };
    }
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
                            false,
                        )
                        .is_empty()
                        {
                            return Ok(i);
                        }
                    }
                    TranslatedObjectChild::Component(obj)
                    | TranslatedObjectChild::ObjectAssignment(obj) => {
                        make_tree_return_i!(i, obj.value, obj.name);
                    }
                    TranslatedObjectChild::ObjectProperty(obj) => {
                        make_tree_return_i!(i, obj.default_value, obj.name);
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

    Err(Error::msg(format!("Cannot LOCATE {:?}", tree)))
}

fn insert_into_root(
    root_cursor: &mut usize,
    root: &TreeRoot,
    code: &[TokenType],
    slots: &mut Slots,
) -> Result<()> {
    let mut raw_qml = IteratorPipeline::new(
        Box::new(
            if matches!(root, TreeRoot::Object(_)) {
                let mut new_data = vec![
                    TokenType::Identifier("Object".to_string()),
                    TokenType::Symbol('{'),
                ];
                new_data.extend_from_slice(code);
                new_data.push(TokenType::Symbol('}'));

                new_data
            } else {
                let mut new_data = vec![
                    TokenType::Identifier("Object".to_string()),
                    TokenType::Symbol('{'),
                    TokenType::Keyword(crate::parser::qml::lexer::Keyword::Enum),
                    TokenType::Identifier("Enum".to_string()),
                    TokenType::Symbol('{'),
                ];
                new_data.extend_from_slice(code);
                new_data.push(TokenType::Symbol('}'));
                new_data.push(TokenType::Symbol('}'));

                new_data
            }
            .into_iter(),
        ),
        "<AST regeneration>",
    );
    let mut slot_resolver = QMLSlotRemapper::new(slots);
    raw_qml.add_remapper(&mut slot_resolver);
    // Start the QML parser...
    let tokens = raw_qml.collect();
    let mut qml_root = parse_qml_from_chain(tokens)?;
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
            TreeRoot::Child {
                parent: _,
                child_index: _,
            } => {
                return Err(Error::msg(
                    "Cannot assign object to a non-object object child!",
                ));
            }
        }
    } else {
        return Err(Error::msg("Internal parse error"));
    }
    Ok(())
}

fn parse_argument_stream(stream: &Vec<TokenType>) -> Result<(Vec<String>, usize)> {
    let mut pos = 0;
    let mut args = Vec::new();
    let mut requires_close = false;
    let mut last_ident = false;
    while pos < stream.len() {
        let token = &stream[pos];
        pos += 1;
        match token {
            TokenType::Whitespace(_) => {}
            TokenType::Symbol('(') => {
                requires_close = true;
            }
            TokenType::Symbol(')') => {
                requires_close = false;
            }
            TokenType::Unknown('=') => {
                if stream.get(pos) != Some(&TokenType::Unknown('>')) {
                    return Err(Error::msg(
                        "Cannot parse QML stream - invalid argument stream!",
                    ));
                }
                pos += 1; // Skip the '>'
                break;
            }
            TokenType::Symbol(',') if last_ident => {
                last_ident = false;
            }

            TokenType::Identifier(ident) if !last_ident => {
                args.push(ident.clone());
                last_ident = true;
            }
            _ => {
                return Err(Error::msg(
                    "Cannot parse QML stream - invalid argument stream!",
                ));
            }
        }
    }

    if requires_close {
        return Err(Error::msg(
            "Cannot parse QML stream - non-closed function arguments!",
        ));
    }

    Ok((args, pos))
}

fn find_beginning_of_function(stream: &Vec<TokenType>, mut start: usize) -> usize {
    while start < stream.len() {
        match stream[start] {
            TokenType::Symbol('{') => {
                break;
            }
            TokenType::Whitespace(_) | TokenType::Comment(_) | TokenType::NewLine(_) => {
                start += 1;
            }
            _ => {
                break;
            }
        }
    }
    start
}

fn build_arguments_token_stream(args: Vec<String>) -> Vec<TokenType> {
    let mut tokens = vec![TokenType::Symbol('(')];
    let len = args.len();
    for (i, arg) in args.into_iter().enumerate() {
        tokens.push(TokenType::Identifier(arg));
        if i != len - 1 {
            tokens.push(TokenType::Symbol(','));
        }
    }
    tokens.push(TokenType::Symbol(')'));

    tokens
}

fn build_arrow_func(
    arguments: Vec<String>,
    body: Vec<TokenType>,
    enclosed: bool,
) -> Vec<TokenType> {
    let mut base = build_arguments_token_stream(arguments);
    base.push(TokenType::Unknown('='));
    base.push(TokenType::Unknown('>'));
    if enclosed {
        base.push(TokenType::Symbol('{'));
    }
    base.extend(body);
    if enclosed {
        base.push(TokenType::Symbol('}'));
    }
    base
}

fn is_whitespace(x: &TokenType) -> bool {
    match x {
        TokenType::NewLine(_) | TokenType::Whitespace(_) => true,
        _ => false,
    }
}

fn find_substream_in_stream(
    haystack: &Vec<TokenType>,
    needle: &Vec<TokenType>,
    mut start: usize,
    glob_whitespace_before: bool,
) -> Option<(usize, usize)> {
    let haystack_len = haystack.len();
    let needle = needle
        .iter()
        .filter(|e| !is_whitespace(e))
        .collect::<Vec<_>>();
    let needle_len = needle.len();
    'main: while start < haystack_len {
        let mut haystack_offset = 0usize;
        // Greedily extend our haystack offset to glob as much as possible
        if glob_whitespace_before {
            while (start + haystack_offset) < haystack_len
                && is_whitespace(&haystack[start + haystack_offset])
            {
                haystack_offset += 1;
            }
        }
        if start + haystack_offset >= haystack_len {
            return None;
        }
        if haystack[start + haystack_offset] != *needle[0] {
            start += haystack_offset + 1;
            continue;
        }

        let mut total_len = haystack_offset;
        let mut needle_i = 0;
        while needle_i < needle_len && (haystack_offset + needle_i + start) < haystack_len {
            while (needle_i + start + haystack_offset < haystack_len)
                && is_whitespace(&haystack[needle_i + start + haystack_offset])
            {
                haystack_offset += 1;
                total_len += 1;
            }
            if haystack[needle_i + start + haystack_offset] != *needle[needle_i] {
                start += needle_i + haystack_offset + 1;
                continue 'main;
            }
            total_len += 1;
            needle_i += 1;
        }
        return Some((start, total_len));
    }
    None
}

fn execute_rebuild_steps(
    rebuild_instructions: &RebuildAction,
    arguments: &mut Option<Vec<String>>,
    main_body_stream: &mut Vec<TokenType>,
) -> Result<()> {
    let mut position = usize::MAX;

    macro_rules! not_functional_error {
        () => {
            return Err(Error::msg("Cannot edit the arguments of a non-function!"))
        };
    }
    let mut located = None;
    for instr in &rebuild_instructions.actions {
        macro_rules! unambiguous_position {
            () => {
                if position == usize::MAX {
                    return Err(Error::msg(format!(
                        "In order to apply {:?}, the position must be unambiguous! Please use LOCATE first.",
                        instr
                    )));
                }
            };
        }

        match instr {
            RebuildInstruction::InsertArgument(arg) => match arguments {
                Some(ref mut arguments) => {
                    if arg.position > arguments.len() {
                        return Err(Error::msg(format!("Cannot insert the argument {} at position {} - there are only {} elements", arg.name, arg.position, arguments.len())));
                    }
                    arguments.insert(arg.position, arg.name.clone());
                }
                None => not_functional_error!(),
            },
            RebuildInstruction::RemoveArgument(arg) => match arguments {
                Some(ref mut arguments) => {
                    if arg.position >= arguments.len()
                        || arguments.get(arg.position) != Some(&arg.name)
                    {
                        return Err(Error::msg(format!(
                            "Cannot remove the argument {} at position {}",
                            arg.name, arg.position
                        )));
                    }
                    arguments.remove(arg.position);
                }
                None => not_functional_error!(),
            },
            RebuildInstruction::RenameArgument(arg, new_name) => match arguments {
                Some(ref mut arguments) => {
                    if arg.position >= arguments.len()
                        || arguments.get(arg.position) != Some(&arg.name)
                    {
                        return Err(Error::msg(format!(
                            "Cannot rename the argument {} at position {}",
                            arg.name, arg.position
                        )));
                    }
                    arguments[arg.position] = new_name.clone();
                }
                None => not_functional_error!(),
            },
            RebuildInstruction::Insert(insert) => {
                unambiguous_position!();
                main_body_stream.splice(position..position, insert.clone());
                position += insert.len();
            }
            RebuildInstruction::Locate(locate) => match &locate.selector {
                LocateRebuildActionSelector::All => {
                    match locate.location {
                        Location::After => position = main_body_stream.len(),
                        Location::Before => position = 0,
                    }
                    located = None;
                }
                LocateRebuildActionSelector::Stream(stream) => {
                    let current_position = if position == usize::MAX { 0 } else { position };
                    let (new_base_pos, length) = match find_substream_in_stream(
                        main_body_stream,
                        stream,
                        current_position,
                        true,
                    ) {
                        Some(n) => n,
                        None => {
                            return Err(Error::msg(format!(
                                "Cannot locate the substream [{:?}] in [{:?}]",
                                stream, &main_body_stream
                            )));
                        }
                    };
                    located = Some(stream.clone());
                    match locate.location {
                        Location::After => position = new_base_pos + length,
                        Location::Before => position = new_base_pos,
                    }
                }
            },
            RebuildInstruction::Remove(remove) => {
                match remove {
                    RemoveRebuildAction::Located => {
                        // Make sure we're located at the position the offending stream starts at:
                        unambiguous_position!();
                        if let Some(ref located) = located {
                            if let Some((position_located, length_located)) =
                                find_substream_in_stream(main_body_stream, located, position, true)
                            {
                                if position_located == position {
                                    // We're OK - remove
                                    main_body_stream
                                        .splice(position..position + length_located, vec![]);
                                } else {
                                    return Err(Error::msg(
                                        "LOCATED substream not at current cursor position!",
                                    ));
                                }
                            } else {
                                return Err(Error::msg("LOCATED substream not at found!"));
                            }
                        } else {
                            return Err(Error::msg(
                                "In order to use LOCATED, LOCATE to a substream first!",
                            ));
                        }
                    }
                    RemoveRebuildAction::Stream(literal) => {
                        // Make sure the cursor is located where 'literal' starts at
                        unambiguous_position!();
                        if let Some((found_position, length)) =
                            find_substream_in_stream(main_body_stream, literal, position, true)
                        {
                            if found_position == position {
                                // We're OK - remove
                                main_body_stream.splice(position..position + length, vec![]);
                            } else {
                                return Err(Error::msg(
                                    "Requested substream to REMOVE could not be found",
                                ));
                            }
                        } else {
                            return Err(Error::msg(
                                "Requested substream to REMOVE not at current index",
                            ));
                        }
                    }
                    RemoveRebuildAction::UntilEnd => {
                        unambiguous_position!();
                        main_body_stream.splice(position.., vec![]);
                    }
                    RemoveRebuildAction::UntilStream(until_stream) => {
                        located = Some(until_stream.clone());
                        if let Some((until_stream_location, _)) =
                            find_substream_in_stream(main_body_stream, until_stream, position, true)
                        {
                            main_body_stream.splice(position..until_stream_location, vec![]);
                        } else {
                            return Err(Error::msg(
                                "Requested substream to REMOVE UNTIL not found in stream",
                            ));
                        }
                    }
                }
            }
            RebuildInstruction::Replace(replace) => {
                unambiguous_position!();
                // NOTE / TODO: Interpolated strings still break
                let source_stream = match &replace.what {
                    ReplaceRebuildActionWhat::Located => {
                        if let Some(ref located) = located {
                            located.clone()
                        } else {
                            return Err(Error::msg(
                                "In order to use LOCATED, LOCATE to a substream first!",
                            ));
                        }
                    }
                    ReplaceRebuildActionWhat::LiteralStream(stream) => stream.clone(),
                };
                let mut until_position = match &replace.until_stream {
                    Some(stream) => {
                        match find_substream_in_stream(main_body_stream, stream, position, false) {
                            Some((pos, _len)) => pos,
                            None => {
                                return Err(Error::msg(format!(
                                    "Could not locate substream [{:?}] in stream!",
                                    stream
                                )));
                            }
                        }
                    }
                    None => main_body_stream.len(),
                };
                let mut counter = 0;
                let mut position = position;
                while position < until_position {
                    let (found_index, source_length) = match find_substream_in_stream(
                        main_body_stream,
                        &source_stream,
                        position,
                        false,
                    ) {
                        None => break,
                        Some(n) => n,
                    };
                    until_position -= source_length;
                    position = found_index;
                    main_body_stream.splice(
                        position..position + source_length,
                        replace.new_contents.clone(),
                    );
                    position += replace.new_contents.len() - source_length + 1;
                    counter += 1;
                }
                if counter == 0 {
                    return Err(Error::msg(format!(
                        "Cannot replace substream {:?} - not found!",
                        source_stream
                    )));
                }
            }
        }
    }
    Ok(())
}

fn redefine_child(
    rebuild_instructions: &RebuildAction,
    child: TranslatedObjectChild,
) -> Result<Vec<TranslatedObjectChild>> {
    let reified_child = untranslate_object_child(child);
    let mut child_token_stream = emit_object_to_token_stream(
        &Object {
            name: String::new(),
            full_name: String::new(),
            children: vec![reified_child],
        },
        true,
    );
    execute_rebuild_steps(rebuild_instructions, &mut None, &mut child_token_stream)?;
    // Wrap in a temporary object:
    child_token_stream.insert(0, TokenType::Identifier("Temporary".into()));
    child_token_stream.insert(1, TokenType::Symbol('{'));
    child_token_stream.push(TokenType::Symbol('}'));
    let parsed_object = parse_qml_into_simple_object(child_token_stream)?;

    Ok(parsed_object
        .children
        .into_iter()
        .map(translate_object_child)
        .collect::<Vec<_>>())
}

fn rebuild_child(
    rebuild_instructions: &RebuildAction,
    child: &mut TranslatedObjectChild,
) -> Result<()> {
    let mut arguments_token_length = 0;
    let mut arguments = None;
    match child {
        TranslatedObjectChild::Assignment(_) => {}
        TranslatedObjectChild::Function(func) => {
            let (a, b) = parse_argument_stream(&func.arguments)?;
            arguments = Some(a);
            arguments_token_length = b;
        }
        TranslatedObjectChild::Property(prop) => match &prop.default_value {
            Some(AssignmentChildValue::Object(_)) => {}
            Some(AssignmentChildValue::Other(stream)) => {
                if let Ok((a, b)) = parse_argument_stream(stream) {
                    arguments = Some(a);
                    arguments_token_length = b;
                }
            }
            None => {}
        },
        TranslatedObjectChild::Object(_) => {}
        TranslatedObjectChild::ObjectAssignment(_) => {}
        TranslatedObjectChild::ObjectProperty(_) => {}
        _ => {
            return Err(Error::msg(
                "Can only rebuild functions / assignments / objects!",
            ));
        }
    }

    let (mut main_body_stream, is_enclosed, is_object) = if arguments.is_some() {
        match child {
            TranslatedObjectChild::Function(func) => {
                func.body.remove(0);
                func.body.pop();
                (take(&mut func.body), true, false)
            }
            TranslatedObjectChild::Assignment(assign) => match assign.value {
                AssignmentChildValue::Other(ref mut stream) => {
                    let mut begin = find_beginning_of_function(stream, arguments_token_length);
                    let mut end = stream.len();
                    let enclosed = stream[begin] == TokenType::Symbol('{');
                    if enclosed {
                        begin += 1;
                        end -= 1;
                    }
                    (Vec::from(&stream[begin..end]), enclosed, false)
                }
                _ => unreachable!(),
            },
            TranslatedObjectChild::Property(prop) => match prop.default_value {
                Some(AssignmentChildValue::Other(ref mut stream)) => {
                    let mut begin = find_beginning_of_function(stream, arguments_token_length);
                    let mut end = stream.len();
                    let enclosed = stream[begin] == TokenType::Symbol('{');
                    if enclosed {
                        begin += 1;
                        end -= 1;
                    }
                    (Vec::from(&stream[begin..end]), enclosed, false)
                }
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
    } else {
        // Not a function!
        match child {
            TranslatedObjectChild::Assignment(assign) => match assign.value {
                AssignmentChildValue::Other(ref mut stream) => (take(stream), false, false),
                AssignmentChildValue::Object(ref obj) => {
                    (emit_object_to_token_stream(obj, false), false, true)
                }
            },
            TranslatedObjectChild::Property(prop) => match prop.default_value {
                Some(AssignmentChildValue::Other(ref mut stream)) => (take(stream), false, false),
                Some(AssignmentChildValue::Object(ref obj)) => {
                    (emit_object_to_token_stream(obj, false), false, true)
                }

                None => panic!("Cannot rebuild a nonexistent value of property!"),
            },
            TranslatedObjectChild::Object(obj) => (
                emit_object_to_token_stream(&untranslate(take(obj)), false),
                false,
                true,
            ),
            TranslatedObjectChild::ObjectAssignment(obj) => (
                emit_object_to_token_stream(&untranslate(take(&mut obj.value)), false),
                false,
                true,
            ),
            TranslatedObjectChild::ObjectProperty(obj) => (
                emit_object_to_token_stream(&untranslate(take(&mut obj.default_value)), false),
                false,
                true,
            ),
            _ => unreachable!(),
        }
    };
    execute_rebuild_steps(rebuild_instructions, &mut arguments, &mut main_body_stream)?;
    // Rebuild the original object.
    // Did we deal with a function?
    match child {
        TranslatedObjectChild::Function(func) => {
            // Yes - reserialize args, rebuild body
            if is_enclosed {
                main_body_stream.insert(0, TokenType::Symbol('{'));
                main_body_stream.push(TokenType::Symbol('}'));
            }
            func.body = main_body_stream;
            func.arguments = build_arguments_token_stream(arguments.unwrap());
        }
        TranslatedObjectChild::Assignment(assign) => {
            if is_object {
                assign.value = AssignmentChildValue::Object(
                    parse_qml_into_simple_object(main_body_stream).unwrap(),
                )
            } else if let Some(arguments) = arguments {
                // This used to be a function. Regenerate fully.
                assign.value = AssignmentChildValue::Other(build_arrow_func(
                    arguments,
                    main_body_stream,
                    is_enclosed,
                ));
            } else {
                // Simple non-function
                assign.value = AssignmentChildValue::Other(main_body_stream);
            }
        }
        TranslatedObjectChild::Property(prop) => {
            if is_object {
                prop.default_value = Some(AssignmentChildValue::Object(
                    parse_qml_into_simple_object(main_body_stream).unwrap(),
                ))
            } else if let Some(arguments) = arguments {
                // This used to be a function. Regenerate fully.
                prop.default_value = Some(AssignmentChildValue::Other(build_arrow_func(
                    arguments,
                    main_body_stream,
                    is_enclosed,
                )));
            } else {
                // Simple non-function
                prop.default_value = Some(AssignmentChildValue::Other(main_body_stream));
            }
        }
        TranslatedObjectChild::Object(obj) => {
            *obj = translate(parse_qml_into_simple_object(main_body_stream).unwrap());
        }
        TranslatedObjectChild::ObjectAssignment(obj) => {
            obj.value = translate(parse_qml_into_simple_object(main_body_stream).unwrap());
        }
        TranslatedObjectChild::ObjectProperty(obj) => {
            obj.default_value = translate(parse_qml_into_simple_object(main_body_stream).unwrap());
        }
        _ => unreachable!(),
    }
    Ok(())
}

pub fn process(absolute_root: &mut TranslatedTree, diff: &Change, slots: &mut Slots) -> Result<()> {
    let mut root_stack: Vec<RootReference> = Vec::new();
    let mut current_root = RootReference {
        root: vec![TreeRoot::Object(absolute_root.root.clone())],
        cursor: None,
        is_replicating: false,
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
            FileChangeAction::End(Keyword::Traverse) if !current_root.is_replicating => {
                // Pop the last object from the stack to return to the previous root
                if let Some(root) = root_stack.pop() {
                    current_root = root;
                } else {
                    return Err(Error::msg("Cannot END TRAVERSE - end of scope!"));
                }
            }
            FileChangeAction::End(Keyword::Replicate) if current_root.is_replicating => {
                if let Some(previous_root) = root_stack.pop() {
                    // Grab the children
                    let children_to_merge = {
                        let fake_root = unambiguous_root!();
                        match fake_root {
                            TreeRoot::Object(obj) => take(&mut obj.borrow_mut().children),
                            _ => unreachable!("Fake root is created as object, always!"),
                        }
                    };
                    current_root = previous_root;
                    // Merge
                    let (root, cursor) = unambiguous_root_cursor_set!();
                    match root {
                        TreeRoot::Object(obj) => {
                            obj.borrow_mut()
                                .children
                                .splice(cursor..cursor, children_to_merge);
                        }
                        _ => {
                            return Err(Error::msg(
                                "Cannot END REPLICATE - the old parent is not an object!",
                            ));
                        }
                    }
                } else {
                    return Err(Error::msg("Cannot END REPLICATE - end of scope!"));
                }
            }
            FileChangeAction::End(_) => {
                return Err(Error::msg("END TRAVERSE / END REPLICATE first!"));
            }
            FileChangeAction::Replicate(tree) => {
                let object = locate_in_tree(current_root.root.clone(), tree, true);
                if object.len() != 1 {
                    return Err(Error::msg(format!(
                        "Cannot locate exactly one elemnt for replication: {}",
                        tree_to_string(tree)
                    )));
                }

                // Push the current root onto the stack and create a new root that will consist of the replicated object

                root_stack.push(current_root);
                let element = match object.first().unwrap() {
                    TreeRoot::Child {
                        parent,
                        child_index,
                    } => parent.borrow().children[*child_index].deep_clone(),
                    _ => unreachable!("force_all_children = true"),
                };
                current_root = RootReference {
                    root: vec![TreeRoot::Object(Rc::new(RefCell::new(TranslatedObject {
                        name: String::default(),
                        full_name: String::default(),
                        children: vec![element],
                    })))],
                    cursor: None,
                    is_replicating: true,
                }
            }
            FileChangeAction::Traverse(tree) => {
                // Attempt to locate the child object in the current root
                let object = locate_in_tree(current_root.root.clone(), tree, false);
                if object.is_empty() {
                    return Err(Error::msg(format!(
                        "Cannot locate element in tree: {}",
                        tree_to_string(tree)
                    )));
                }

                // Push the current root onto the stack and set the new current root
                root_stack.push(current_root);
                current_root = RootReference {
                    root: object,
                    cursor: None,
                    is_replicating: false,
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
                            TreeRoot::Child {
                                parent: _,
                                child_index: _,
                            } => traverse_no_raw_children!(),
                        }
                    }
                    !locate_in_tree(vec![e.clone()], tree_selector, false).is_empty()
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
                    Insertable::Template(_, _) => {
                        panic!("Cannot insert template! Use `process_templates()` first!")
                    }
                } {
                    let (root, mut cursor) = unambiguous_root_cursor_set!();
                    insert_into_root(&mut cursor, root, code, slots)?;
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
                            TreeRoot::Child {
                                parent: _,
                                child_index: _,
                            } => traverse_no_raw_children!(),
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
                    TreeRoot::Child {
                        parent: _,
                        child_index: _,
                    } => traverse_no_raw_children!(),
                };
                insert_into_root(
                    &mut element_idx,
                    root,
                    match &replacer.content {
                        Insertable::Code(code) => code,
                        Insertable::Slot(_) => {
                            panic!("Cannot insert slot! Use `process_slots()` first!")
                        }
                        Insertable::Template(_, _) => {
                            panic!("Cannot insert template! Use `process_slots()` first!")
                        }
                    },
                    slots,
                )?;
                current_root.cursor = Some(element_idx);
            }
            FileChangeAction::Rename(rename) => {
                let root = unambiguous_root!();
                let element_idx = find_first_matching_child(root, &rename.selector)?;
                match root {
                    TreeRoot::Enum(_) => {
                        return Err(Error::msg("Cannot RENAME a value within an enum!"))
                    }
                    TreeRoot::Object(obj) => {
                        obj.borrow_mut().children[element_idx].set_name(rename.name_to.clone())?;
                    }
                    TreeRoot::Child {
                        parent: _,
                        child_index: _,
                    } => traverse_no_raw_children!(),
                }
                current_root.cursor = Some(element_idx + 1);
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
                    TreeRoot::Child {
                        parent: _,
                        child_index: _,
                    } => traverse_no_raw_children!(),
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
            FileChangeAction::Rebuild(rebuild) => {
                let root = unambiguous_root!();
                let element_idx = find_first_matching_child(root, &vec![rebuild.selector.clone()])?;
                match root {
                    TreeRoot::Enum(_) => {
                        return Err(Error::msg("Cannot rebuild an enum!"));
                    }
                    TreeRoot::Object(obj) => {
                        if rebuild.redefine {
                            let child = obj.borrow_mut().children.remove(element_idx);
                            let new_children = redefine_child(rebuild, child)?;
                            obj.borrow_mut()
                                .children
                                .splice(element_idx..element_idx, new_children.into_iter());
                        } else {
                            let child_reference = &mut obj.borrow_mut().children[element_idx];
                            rebuild_child(rebuild, child_reference)?;
                        }
                    }
                    TreeRoot::Child {
                        parent: _,
                        child_index: _,
                    } => traverse_no_raw_children!(),
                };
            }
            FileChangeAction::AllowMultiple => {
                return Err(Error::msg("Not supported yet!"));
            }
        }
    }

    Ok(())
}
