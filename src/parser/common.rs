use anyhow::Error;

#[macro_export]
macro_rules! error_received_expected {
    ($recvd: expr, $expected: expr) => {
        Err(Error::msg(format!(
            "Error while parsing: expected {}, got {:?}",
            $expected, $recvd
        )))
    };
}

pub enum ChainIteratorRemapper<T> {
    End,
    Skip,
    Value(T),
    Chain(Vec<Box<dyn Iterator<Item = T>>>),
    Link(Box<dyn Iterator<Item = T>>),
    Error(Error),
}

pub trait IteratorRemapper<T> {
    fn remap(&mut self, value: T) -> ChainIteratorRemapper<T>;
}

pub struct IteratorPipeline<'a, T> {
    iterators: Vec<Box<dyn Iterator<Item = T>>>,
    remappers: Vec<&'a mut dyn IteratorRemapper<T>>,
}

enum InternalChainIterValue<T> {
    Value(T),
    End,
    Reload,
}
impl<'a, T> IteratorPipeline<'a, T> {
    pub fn new(root_iterator: Box<dyn Iterator<Item = T>>) -> Self {
        Self {
            iterators: vec![root_iterator],
            remappers: Vec::new(),
        }
    }

    pub fn add_remapper(&mut self, remapper: &'a mut dyn IteratorRemapper<T>) {
        self.remappers.push(remapper);
    }

    fn remap(&mut self, mut item: T) -> InternalChainIterValue<T> {
        for rm in self.remappers.iter_mut() {
            let remapped = match rm.remap(item) {
                ChainIteratorRemapper::Chain(ch) => {
                    self.iterators.extend(ch);
                    InternalChainIterValue::Reload
                }
                ChainIteratorRemapper::End => InternalChainIterValue::End,
                ChainIteratorRemapper::Link(lnk) => {
                    self.iterators.push(lnk);
                    InternalChainIterValue::Reload
                }
                ChainIteratorRemapper::Skip => InternalChainIterValue::Reload,
                ChainIteratorRemapper::Value(v) => InternalChainIterValue::Value(v),
                ChainIteratorRemapper::Error(err) => panic!("{:?}", err), // TODO!
            };

            if let InternalChainIterValue::Value(i) = remapped {
                item = i;
            } else {
                return remapped;
            }
        }

        InternalChainIterValue::Value(item)
    }
}

impl<T> Iterator for IteratorPipeline<'_, T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        loop {
            if self.iterators.is_empty() {
                return None;
            }
            let item = self.iterators.last_mut().unwrap().next();
            if let Some(item) = item {
                let val = self.remap(item);
                match val {
                    InternalChainIterValue::End => {
                        self.iterators.clear();
                        return None;
                    }
                    InternalChainIterValue::Reload => {
                        continue;
                    }
                    InternalChainIterValue::Value(v) => {
                        return Some(v);
                    }
                }
            } else {
                self.iterators.pop();
            }
        }
    }
}
