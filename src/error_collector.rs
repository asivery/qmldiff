use std::fmt;

#[derive(Debug, Clone)]
pub struct HashLookupError {
    pub hash_id: u64,
    pub source_file: String,
}

impl HashLookupError {
    pub fn new(hash_id: u64, source_file: String) -> Self {
        Self {
            hash_id,
            source_file,
        }
    }
}

impl fmt::Display for HashLookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} - Cannot resolve hash {}",
            self.source_file, self.hash_id
        )
    }
}

#[derive(Debug, Clone)]
pub struct LexerError {
    pub message: String,
    pub position: usize,
    pub line: usize,
}

impl LexerError {
    pub fn new(message: String, position: usize, line: usize) -> Self {
        Self {
            message,
            position,
            line,
        }
    }
}

impl fmt::Display for LexerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Lexer error at position {} (line {}): {}",
            self.position, self.line, self.message
        )
    }
}

#[derive(Debug, Default, Clone)]
pub struct ErrorCollector {
    hash_errors: Vec<HashLookupError>,
    lexer_errors: Vec<LexerError>,
}

impl ErrorCollector {
    pub fn new() -> Self {
        Self {
            hash_errors: Vec::new(),
            lexer_errors: Vec::new(),
        }
    }

    pub fn add_error(&mut self, error: HashLookupError) {
        self.hash_errors.push(error);
    }

    pub fn add_lexer_error(&mut self, message: String, position: usize, line: usize) {
        self.lexer_errors.push(LexerError::new(message, position, line));
    }

    pub fn has_errors(&self) -> bool {
        !self.hash_errors.is_empty() || !self.lexer_errors.is_empty()
    }

    pub fn error_count(&self) -> usize {
        self.hash_errors.len() + self.lexer_errors.len()
    }

    pub fn errors(&self) -> &[HashLookupError] {
        &self.hash_errors
    }

    pub fn lexer_errors(&self) -> &[LexerError] {
        &self.lexer_errors
    }

    pub fn print_errors(&self) {
        for error in &self.hash_errors {
            eprintln!("{}", error);
        }
        for error in &self.lexer_errors {
            eprintln!("{}", error);
        }
    }
}
