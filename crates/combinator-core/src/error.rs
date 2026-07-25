//! Typed errors returned by core processing.

#[derive(Debug)]
pub struct CoreError {
    pub code: &'static str,
    pub message: String,
    pub context: Vec<(String, String)>,
    pub kind: ErrorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Usage,
    Runtime,
}

impl CoreError {
    pub fn usage(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            context: Vec::new(),
            kind: ErrorKind::Usage,
        }
    }

    pub fn runtime(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            context: Vec::new(),
            kind: ErrorKind::Runtime,
        }
    }

    pub fn with(mut self, key: &str, value: impl ToString) -> Self {
        self.context.push((key.to_string(), value.to_string()));
        self
    }

    pub fn with_context(mut self, context: &[(String, String)]) -> Self {
        self.context.extend(context.iter().cloned());
        self
    }
}

impl From<std::io::Error> for CoreError {
    fn from(error: std::io::Error) -> Self {
        Self::runtime("WRITE_FAILED", format!("failed writing output: {error}"))
    }
}
