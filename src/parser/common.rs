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

pub trait IteratorRemapper<T, Ctx> {
    fn remap(&mut self, value: T, context: &Ctx) -> ChainIteratorRemapper<T>;
}

pub struct IteratorPipeline<'a, T, Ctx> {
    context: Ctx,
    iterators: Vec<Box<dyn Iterator<Item = T>>>,
    remappers: Vec<&'a mut dyn IteratorRemapper<T, Ctx>>,
}

enum InternalChainIterValue<T> {
    Value(T),
    End,
    Reload,
}
impl<'a, T, Ctx> IteratorPipeline<'a, T, Ctx> {
    pub fn new(root_iterator: Box<dyn Iterator<Item = T>>, context: Ctx) -> Self {
        Self {
            iterators: vec![root_iterator],
            remappers: Vec::new(),
            context,
        }
    }

    pub fn add_remapper(&mut self, remapper: &'a mut dyn IteratorRemapper<T, Ctx>) {
        self.remappers.push(remapper);
    }

    fn remap(&mut self, mut item: T) -> InternalChainIterValue<T> {
        for rm in self.remappers.iter_mut() {
            let remapped = match rm.remap(item, &self.context) {
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

impl<T, Ctx> Iterator for IteratorPipeline<'_, T, Ctx> {
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

pub enum CollectionType {
    Break,
    Include,
    Drop,
}

impl From<bool> for CollectionType {
    fn from(value: bool) -> Self {
        if value {
            CollectionType::Include
        } else {
            CollectionType::Break
        }
    }
}

pub trait GenericLexerBase {
    fn peek(&self) -> Option<char>;
    fn peek_offset(&self, off: usize) -> Option<char>;
    fn advance(&mut self) -> Option<char>;

    fn collect_while<F>(&mut self, mut condition: F) -> String
    where
        F: FnMut(char) -> CollectionType,
        Self: Sized,
    {
        let mut result = String::new();
        while let Some(c) = self.peek() {
            match condition(c) {
                CollectionType::Break => break,
                CollectionType::Drop => {
                    self.advance();
                }
                CollectionType::Include => {
                    result.push(c);
                    self.advance();
                }
            }
        }
        result
    }
}

#[derive(Default)]
pub struct StringCharacterTokenizer {
    pub input: String,   // Raw input string
    pub position: usize, // current position in the input
}

impl StringCharacterTokenizer {
    pub fn new(input: String) -> Self {
        Self { input, position: 0 }
    }

    pub fn peek(&self) -> Option<char> {
        self.input[self.position..].chars().next()
    }

    pub fn peek_offset(&self, off: usize) -> Option<char> {
        self.input[self.position + off..].chars().next()
    }

    pub fn advance(&mut self) -> Option<char> {
        if let Some(c) = self.peek() {
            self.position += c.len_utf8();
            Some(c)
        } else {
            None
        }
    }

    pub fn collect_while<F>(&mut self, mut condition: F) -> String
    where
        F: FnMut(&Self, char) -> CollectionType,
    {
        let mut result = String::new();
        while let Some(c) = self.peek() {
            match condition(self, c) {
                CollectionType::Break => break,
                CollectionType::Drop => {
                    self.advance();
                }
                CollectionType::Include => {
                    result.push(c);
                    self.advance();
                }
            }
        }
        result
    }
}
