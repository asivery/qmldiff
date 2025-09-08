use anyhow::{Error, Result};

use crate::parser::qml::emitter::emit_simple_token_stream;
use crate::parser::qml::parser::{
    AssignmentChild, AssignmentChildValue, ComponentDefinition, EnumChild, FunctionChild, Object,
    ObjectAssignmentChild, ObjectChild, PropertyChild, QMLTree, SignalChild, TreeElement,
};
use std::cell::RefCell;
use std::mem::take;
use std::rc::Rc;

type TranslatedEnumChildValues = Rc<RefCell<Vec<(String, Option<String>)>>>;

#[derive(Debug, Clone)]
pub struct TranslatedEnumChild {
    pub name: String,
    pub values: TranslatedEnumChildValues,
}

impl TranslatedEnumChild {
    pub fn deep_clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            values: Rc::new(RefCell::new(self.values.borrow().clone())),
        }
    }
}

#[derive(Debug)]
pub struct TranslatedObjectAssignmentChild {
    pub name: String,
    pub value: TranslatedObjectRef,
}

impl TranslatedObjectAssignmentChild {
    pub fn deep_clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            value: deep_clone_translated_object(&self.value),
        }
    }
}

#[derive(Debug)]
pub enum TranslatedObjectChild {
    Signal(SignalChild),
    Property(PropertyChild<Option<AssignmentChildValue>>),
    ObjectProperty(PropertyChild<TranslatedObjectRef>),
    Assignment(AssignmentChild),
    ObjectAssignment(TranslatedObjectAssignmentChild),
    Function(FunctionChild),
    Object(TranslatedObjectRef),
    Enum(TranslatedEnumChild),
    Component(TranslatedObjectAssignmentChild),
}

impl TranslatedObjectChild {
    pub fn deep_clone(&self) -> Self {
        // Objects all have to be deep cloned!
        match self {
            Self::Assignment(a) => Self::Assignment(a.clone()),
            Self::Enum(e) => Self::Enum(e.deep_clone()),
            Self::Component(c) => Self::Component(c.deep_clone()),
            Self::Function(f) => Self::Function(f.clone()),
            Self::Object(o) => Self::Object(deep_clone_translated_object(o)),
            Self::ObjectAssignment(a) => Self::ObjectAssignment(a.deep_clone()),
            Self::ObjectProperty(p) => Self::ObjectProperty(deep_clone_property_child(p)),
            Self::Property(p) => Self::Property(p.clone()),
            Self::Signal(s) => Self::Signal(s.clone()),
        }
    }
}

pub type TranslatedObjectRef = Rc<RefCell<TranslatedObject>>;

pub fn deep_clone_translated_object(obj: &TranslatedObjectRef) -> TranslatedObjectRef {
    let instance = obj.borrow();
    Rc::new(RefCell::new(TranslatedObject {
        name: instance.name.clone(),
        children: instance
            .children
            .iter()
            .map(|e| e.deep_clone())
            .collect::<Vec<_>>(),
        full_name: instance.full_name.clone(),
    }))
}

pub fn deep_clone_property_child(
    pc: &PropertyChild<TranslatedObjectRef>,
) -> PropertyChild<TranslatedObjectRef> {
    PropertyChild {
        name: pc.name.clone(),
        default_value: deep_clone_translated_object(&pc.default_value),
        modifiers: pc.modifiers.clone(),
        r#type: pc.r#type.clone(),
    }
}

#[derive(Debug, Default)]
pub struct TranslatedObject {
    pub name: String,
    pub children: Vec<TranslatedObjectChild>,
    pub full_name: String,
}

impl<'a> TranslatedObjectChild {
    pub fn get_name(&'a self) -> Option<&'a String> {
        match self {
            TranslatedObjectChild::Assignment(assi) => Some(&assi.name),
            TranslatedObjectChild::ObjectAssignment(assi) => Some(&assi.name),
            TranslatedObjectChild::Component(cmp) => Some(&cmp.name),
            TranslatedObjectChild::Enum(e) => Some(&e.name),
            TranslatedObjectChild::Function(fnc) => Some(&fnc.name),
            TranslatedObjectChild::Object(_) => None,
            TranslatedObjectChild::Property(prop) => Some(&prop.name),
            TranslatedObjectChild::ObjectProperty(prop) => Some(&prop.name),
            TranslatedObjectChild::Signal(signal) => Some(&signal.name),
        }
    }

    pub fn get_str_value(&'a self) -> Option<String> {
        match self {
            TranslatedObjectChild::Assignment(assigned) => match &assigned.value {
                AssignmentChildValue::Other(generic_value) => {
                    Some(emit_simple_token_stream(generic_value))
                }
                _ => None,
            },
            TranslatedObjectChild::ObjectAssignment(_) => None,
            TranslatedObjectChild::Component(_) => None,
            TranslatedObjectChild::Enum(_) => None,
            TranslatedObjectChild::Function(_) => None,
            TranslatedObjectChild::Object(_) => None,
            TranslatedObjectChild::Property(prop) => match &prop.default_value {
                Some(AssignmentChildValue::Other(generic_value)) => {
                    Some(emit_simple_token_stream(generic_value))
                }
                _ => None,
            },
            TranslatedObjectChild::ObjectProperty(_) => None,
            TranslatedObjectChild::Signal(_) => None,
        }
    }
    pub fn set_name(&'a mut self, name: String) -> Result<()> {
        macro_rules! error {
            () => {
                Err(Error::msg(format!("Cannot rename object: {:?}", self)))
            };
        }
        match self {
            TranslatedObjectChild::Assignment(assigned) => assigned.name = name,
            TranslatedObjectChild::Component(cmp) => cmp.name = name,
            TranslatedObjectChild::Function(func) => func.name = name,
            TranslatedObjectChild::Object(_) => return error!(),
            TranslatedObjectChild::Property(prop) => prop.name = name,
            TranslatedObjectChild::ObjectProperty(prop) => prop.name = name,
            TranslatedObjectChild::Signal(sig) => sig.name = name,
            TranslatedObjectChild::ObjectAssignment(asi) => asi.name = name,
            TranslatedObjectChild::Enum(enu) => enu.name = name,
        };
        Ok(())
    }
}

pub fn translate_object_child(child: ObjectChild) -> TranslatedObjectChild {
    match child {
        ObjectChild::Assignment(z) => TranslatedObjectChild::Assignment(z),
        ObjectChild::Function(z) => TranslatedObjectChild::Function(z),
        ObjectChild::Property(z) => TranslatedObjectChild::Property(z),
        ObjectChild::Signal(z) => TranslatedObjectChild::Signal(z),

        ObjectChild::ObjectAssignment(z) => {
            TranslatedObjectChild::ObjectAssignment(TranslatedObjectAssignmentChild {
                name: z.name,
                value: translate(z.value),
            })
        }
        ObjectChild::ObjectProperty(z) => {
            TranslatedObjectChild::ObjectProperty(PropertyChild::<TranslatedObjectRef> {
                name: z.name,
                default_value: translate(z.default_value),
                modifiers: z.modifiers,
                r#type: z.r#type,
            })
        }
        ObjectChild::Component(z) => {
            TranslatedObjectChild::Component(TranslatedObjectAssignmentChild {
                name: z.name,
                value: translate(z.object),
            })
        }
        ObjectChild::Object(z) => TranslatedObjectChild::Object(translate(z)),
        ObjectChild::Enum(z) => TranslatedObjectChild::Enum(TranslatedEnumChild {
            name: z.name,
            values: Rc::new(RefCell::new(z.values)),
        }),
    }
}

pub fn translate(object: Object) -> TranslatedObjectRef {
    Rc::new(RefCell::new(TranslatedObject {
        name: object.name,
        full_name: object.full_name,
        children: object
            .children
            .into_iter()
            .map(translate_object_child)
            .collect(),
    }))
}

pub fn untranslate_object_child(child: TranslatedObjectChild) -> ObjectChild {
    match child {
        TranslatedObjectChild::Assignment(z) => ObjectChild::Assignment(z),
        TranslatedObjectChild::Function(z) => ObjectChild::Function(z),
        TranslatedObjectChild::Property(z) => ObjectChild::Property(z),
        TranslatedObjectChild::Signal(z) => ObjectChild::Signal(z),

        TranslatedObjectChild::Component(z) => ObjectChild::Component(ComponentDefinition {
            name: z.name,
            object: untranslate(z.value),
        }),
        TranslatedObjectChild::ObjectProperty(z) => {
            ObjectChild::ObjectProperty(PropertyChild::<Object> {
                name: z.name,
                default_value: untranslate(z.default_value),
                modifiers: z.modifiers,
                r#type: z.r#type,
            })
        }
        TranslatedObjectChild::ObjectAssignment(z) => {
            ObjectChild::ObjectAssignment(ObjectAssignmentChild {
                name: z.name,
                value: untranslate(z.value),
            })
        }
        TranslatedObjectChild::Object(z) => ObjectChild::Object(untranslate(z)),
        TranslatedObjectChild::Enum(z) => ObjectChild::Enum(EnumChild {
            name: z.name,
            values: z.values.take(),
        }),
    }
}

pub fn untranslate(object: TranslatedObjectRef) -> Object {
    let taken: TranslatedObject = take(&mut *object.borrow_mut());
    Object {
        name: taken.name,
        full_name: taken.full_name,
        children: taken
            .children
            .into_iter()
            .map(untranslate_object_child)
            .collect(),
    }
}

#[derive(Debug)]
pub struct TranslatedTree {
    pub root: TranslatedObjectRef,
    pub leftovers: Vec<TreeElement>,
}

pub fn translate_from_root(tree: QMLTree) -> TranslatedTree {
    let mut leftovers = Vec::new();
    let mut root = TranslatedObject {
        full_name: "!<VIRTUAL ROOT>!".into(),
        name: "VIRTUAL ROOT".into(),
        children: Vec::new(),
    };
    for element in tree {
        match element {
            TreeElement::Object(object) => root
                .children
                .push(TranslatedObjectChild::Object(translate(object))),
            any => leftovers.push(any),
        }
    }

    TranslatedTree {
        leftovers,
        root: Rc::new(RefCell::new(root)),
    }
}

pub fn untranslate_from_root(tree: TranslatedTree) -> QMLTree {
    let mut out = Vec::default();
    out.extend(tree.leftovers);
    for object in &tree.root.borrow_mut().children {
        if let TranslatedObjectChild::Object(object) = object {
            out.push(TreeElement::Object(untranslate(object.clone())));
        }
    }

    out
}
