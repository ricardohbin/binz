//! Turns one entry file into the whole graph of files it imports.
//!
//! `@root` is the directory of the file handed to `binz`, and every local
//! import is written from there -- `import @root/utils/math.binz;` -- so a
//! path means the same thing no matter which file it is written in. There is
//! deliberately no `../`, and no way to reach outside the root: one spelling
//! for one file.
//!
//! The binding is the file's own name, always, exactly as the last segment of
//! `binz/io` binds `io`. That is why a file name has to be one lowercase word
//! (the parser enforces it): the name is not decoration, it is the identifier
//! the importing file will type.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::ast::Item;
use crate::error::{CResult, CompileError};
use crate::lexer::{Lexer, Span};
use crate::parser::Parser;

pub struct SourceFile {
    /// The real path on disk, as diagnostics print it.
    pub path: String,
    /// The `@root/...` spelling, for prose. The entry file has no import
    /// path, so it reuses `path`.
    pub display: String,
    pub src: String,
    pub items: Vec<Item>,
    /// One entry per local import of this file, in source order, naming the
    /// file it resolved to. The compiler pairs them back up with the
    /// `ImportDef`s it walks.
    pub deps: Vec<usize>,
}

pub struct Program {
    /// Every file of the program, dependencies before dependents.
    pub files: Vec<SourceFile>,
    pub entry: usize,
}

impl Program {
    /// The source of `path`, for rendering a diagnostic that names a file.
    pub fn source_of(&self, path: &str) -> Option<&SourceFile> {
        self.files.iter().find(|f| f.path == path)
    }
}

pub fn load(entry: &str) -> CResult<Program> {
    let entry_path = PathBuf::from(entry);
    // `@root` is resolved once, and absolutely, so a diagnostic about a file
    // that is not there can say where it looked.
    let dir = entry_path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let root = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let mut l = Loader { root, files: Vec::new(), done: HashMap::new(), open: Vec::new() };
    let entry_idx = l.load(&entry_path, entry.to_string(), None)?;
    Ok(Program { files: l.files, entry: entry_idx })
}

struct Loader {
    root: PathBuf,
    files: Vec<SourceFile>,
    /// Canonical path -> index, so a file imported twice is compiled once.
    done: HashMap<PathBuf, usize>,
    /// The files currently being loaded, innermost last. A path that shows up
    /// here again is a cycle.
    open: Vec<(PathBuf, String)>,
}

impl Loader {
    /// `blame` is the file and span of the `import` that asked for this one,
    /// so "cannot read" points at the line that named it.
    fn load(
        &mut self,
        real: &Path,
        display: String,
        blame: Option<(&str, Span)>,
    ) -> CResult<usize> {
        let fail = |msg: String| match blame {
            Some((from, span)) => CompileError::new(msg, span).at_file(from),
            None => CompileError::new(msg, Span { line: 1, col: 1 }),
        };

        let key = match std::fs::canonicalize(real) {
            Ok(k) => k,
            Err(e) => {
                return Err(fail(format!(
                    "cannot read `{}`: {} (looked in {})",
                    display,
                    e,
                    real.display()
                )))
            }
        };
        if let Some(i) = self.done.get(&key) {
            return Ok(*i);
        }
        if let Some(at) = self.open.iter().position(|(k, _)| *k == key) {
            let mut chain: Vec<&str> = self.open[at..].iter().map(|(_, d)| d.as_str()).collect();
            chain.push(&display);
            return Err(fail(format!("import cycle: {}", chain.join(" -> "))));
        }

        let src = match std::fs::read_to_string(real) {
            Ok(s) => s,
            Err(e) => return Err(fail(format!("cannot read `{}`: {}", display, e))),
        };
        let path = real.display().to_string();
        let items = Lexer::new(&src)
            .tokenize()
            .and_then(|toks| Parser::new(toks).parse_program())
            .map_err(|e| e.at_file(&path))?;

        self.open.push((key.clone(), display.clone()));
        let mut deps = Vec::new();
        for item in &items {
            let im = match item {
                Item::Import(im) if im.local => im,
                _ => continue,
            };
            let mut child = self.root.clone();
            for seg in &im.path {
                child.push(seg);
            }
            child.set_extension("binz");
            let text = im.text();
            deps.push(self.load(&child, text, Some((&path, im.span)))?);
        }
        self.open.pop();

        let i = self.files.len();
        self.files.push(SourceFile { path, display, src, items, deps });
        self.done.insert(key, i);
        Ok(i)
    }
}
