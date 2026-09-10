use crate::lexer::Span;

#[derive(Debug)]
pub struct CompileError {
    pub msg: String,
    pub span: Span,
}

impl CompileError {
    pub fn new(msg: impl Into<String>, span: Span) -> Self {
        CompileError { msg: msg.into(), span }
    }

    /// Render a rustc-style diagnostic with the offending source line.
    pub fn report(&self, path: &str, src: &str) -> String {
        let mut out = format!(
            "error: {}\n  --> {}:{}:{}\n",
            self.msg, path, self.span.line, self.span.col
        );
        let idx = self.span.line.saturating_sub(1) as usize;
        if let Some(line) = src.lines().nth(idx) {
            let num = self.span.line.to_string();
            out.push_str(&format!("{:>4} | {}\n", num, line));
            let pad = " ".repeat(self.span.col.saturating_sub(1) as usize);
            out.push_str(&format!("     | {}^\n", pad));
        }
        out
    }
}

pub type CResult<T> = Result<T, CompileError>;
