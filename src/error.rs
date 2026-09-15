use crate::lexer::Span;

#[derive(Debug)]
pub struct CompileError {
    pub msg: String,
    pub span: Span,
    /// The file the span belongs to. A program is a graph of files once
    /// `import @root/...` exists, so a span alone no longer locates an
    /// error. `None` means the file being compiled at the top level.
    pub file: Option<String>,
}

impl CompileError {
    pub fn new(msg: impl Into<String>, span: Span) -> Self {
        CompileError { msg: msg.into(), span, file: None }
    }

    /// Attribute the error to `path`, unless it already names a file. The
    /// innermost frame that knows the file wins, so a nested compile is
    /// never relabelled by its caller.
    pub fn at_file(mut self, path: &str) -> Self {
        if self.file.is_none() {
            self.file = Some(path.to_string());
        }
        self
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
