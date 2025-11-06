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

#[derive(Debug, Default, Clone)]
pub struct ErrorCollector {
    errors: Vec<HashLookupError>,
}

impl ErrorCollector {
    pub fn new() -> Self {
        Self { errors: Vec::new() }
    }

    pub fn add_error(&mut self, error: HashLookupError) {
        self.errors.push(error);
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    pub fn error_count(&self) -> usize {
        self.errors.len()
    }

    pub fn errors(&self) -> &[HashLookupError] {
        &self.errors
    }

    pub fn print_errors(&self) {
        for error in &self.errors {
            eprintln!("{}", error);
        }
    }
}
