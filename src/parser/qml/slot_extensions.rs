use anyhow::Error;

use crate::{
    parser::common::{ChainIteratorRemapper, IteratorRemapper},
    slots::Slots,
};

use super::lexer::{QMLExtensionToken, TokenType};

pub struct QMLSlotRemapper<'a> {
    slots: &'a mut Slots,
}

impl<'a> QMLSlotRemapper<'a> {
    pub fn new(slots: &'a mut Slots) -> Self {
        Self {
            slots,
        }
    }
}

impl IteratorRemapper<TokenType> for QMLSlotRemapper<'_> {
    fn remap(&mut self, value: TokenType) -> ChainIteratorRemapper<TokenType> {
        match value {
            TokenType::Extension(QMLExtensionToken::Slot(id)) => {
                if let Some(slot_ref) = self.slots.0.get_mut(&id) {
                    slot_ref.read_back = true;
                    if slot_ref.template {
                        ChainIteratorRemapper::Error(Error::msg(format!(
                            "Cannot insert template {} as a slot",
                            id
                        )))
                    } else {
                        ChainIteratorRemapper::Link(Box::new(
                            self.slots
                                .resolve_slot_final_state(&id)
                                .unwrap()
                                .into_iter(),
                        ))
                    }
                } else {
                    ChainIteratorRemapper::Error(Error::msg(format!("No such slot {}", id)))
                }
            }
            other => ChainIteratorRemapper::Value(other),
        }
    }
}
