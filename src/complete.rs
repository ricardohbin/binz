//! What `binz lsp` offers at a cursor.
//!
//! This reads tokens, not the AST, on purpose. A buffer is completed while it
//! is being typed, so it is almost never a program: `io.` does not parse, and
//! neither does anything after it. The lossy lexer still hands back every
//! token it can read, and that is enough to find the imports, the structs,
//! the functions and the locals alive at the cursor -- which is all a
//! completion needs to know.
//!
//! Nothing here offers a spelling the compiler would refuse: no member a
//! module does not define, no field on a pointer (`(*p).x` is the one way),
//! no test function (nothing can call one), and no module that is not
//! imported (an import is the only way a name comes into a file).

use std::path::{Path, PathBuf};

use crate::lexer::{describe, Lexer, Span, Tok, Token};
use crate::stdlib;
use crate::types::{error_struct, type_name};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Keyword,
    Type,
    Module,
    Function,
    Variable,
    Field,
    Struct,
    File,
    Folder,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub label: String,
    pub kind: ItemKind,
    pub detail: String,
}

fn item(label: &str, kind: ItemKind, detail: impl Into<String>) -> Item {
    Item { label: label.to_string(), kind, detail: detail.into() }
}

pub struct Import {
    pub local: bool,
    pub path: Vec<String>,
    pub binding: String,
    pub span: Span,
}

pub struct FnSym {
    pub name: String,
    /// `(a: i64, b: i64): i64`, as written.
    pub sig: String,
    pub is_test: bool,
}

pub struct StructSym {
    pub name: String,
    /// Name and type, the type as written.
    pub fields: Vec<(String, String)>,
}

/// The top-level declarations of one file.
pub struct Outline {
    pub imports: Vec<Import>,
    pub fns: Vec<FnSym>,
    pub structs: Vec<StructSym>,
}

impl Outline {
    pub fn local_imports(&self) -> Vec<Vec<String>> {
        self.imports.iter().filter(|i| i.local).map(|i| i.path.clone()).collect()
    }

    /// The fields of struct `name`, counting the `Error` every file sees.
    fn fields_of(&self, name: &str) -> Option<Vec<(String, String)>> {
        if let Some(s) = self.structs.iter().find(|s| s.name == name) {
            return Some(s.fields.clone());
        }
        if name == "Error" {
            let e = error_struct();
            return Some(
                e.fields.iter().map(|f| (f.name.clone(), type_name(&f.ty, &[]))).collect(),
            );
        }
        None
    }
}

const PRIMITIVES: &[&str] = &["i32", "i64", "f64", "bool", "str", "void"];
const GENERICS: &[&str] = &["Vector", "LinkedList", "Set", "SortedSet", "HashMap", "SortedMap"];
const STATEMENT_KEYWORDS: &[&str] =
    &["const", "var", "if", "else", "while", "return", "cast", "true", "false", "stub"];
const TOP_KEYWORDS: &[&str] = &["import", "struct", "function"];

pub fn tokens(src: &str) -> Vec<Token> {
    Lexer::new(src).tokenize_lossy()
}

fn before(a: Span, b: Span) -> bool {
    (a.line, a.col) < (b.line, b.col)
}

/// Token text joined back into source, for a type written out in a detail.
fn render(toks: &[Token]) -> String {
    let mut out = String::new();
    for t in toks {
        match t.kind {
            Tok::Comma | Tok::Colon => {
                out.push_str(&describe(&t.kind));
                out.push(' ');
            }
            Tok::Semi => out.push_str("; "),
            _ => out.push_str(&describe(&t.kind)),
        }
    }
    out.trim_end().to_string()
}

/// The end of a type that starts at `i`: the first `,` `)` `=` `;` `{` or
/// `}` that is not nested inside it.
fn type_end(toks: &[Token], mut i: usize) -> usize {
    let mut depth = 0i32;
    while i < toks.len() {
        match toks[i].kind {
            Tok::LParen | Tok::LBracket | Tok::Lt => depth += 1,
            Tok::RParen | Tok::RBracket | Tok::Gt if depth > 0 => depth -= 1,
            Tok::Comma if depth > 0 => {}
            Tok::Comma | Tok::RParen | Tok::Assign | Tok::Semi | Tok::LBrace | Tok::RBrace | Tok::Eof => {
                return i
            }
            _ => {}
        }
        i += 1;
    }
    i
}

/// `name: T, name: T` up to the `)` that closes the list opened at `open`.
/// Answers the parameters and the index of that `)`.
fn params(toks: &[Token], open: usize) -> (Vec<(String, String)>, usize) {
    let mut out = Vec::new();
    let mut i = open + 1;
    while i < toks.len() {
        match (&toks[i].kind, toks.get(i + 1).map(|t| &t.kind)) {
            (Tok::RParen, _) | (Tok::Eof, _) => return (out, i),
            (Tok::Ident(n), Some(Tok::Colon)) => {
                let end = type_end(toks, i + 2);
                out.push((n.clone(), render(&toks[i + 2..end])));
                i = end;
            }
            _ => i += 1,
        }
    }
    (out, i)
}

pub fn outline(toks: &[Token]) -> Outline {
    let mut o = Outline { imports: Vec::new(), fns: Vec::new(), structs: Vec::new() };
    let mut depth = 0;
    let mut i = 0;
    while i < toks.len() {
        match &toks[i].kind {
            Tok::LBrace => depth += 1,
            Tok::RBrace => depth -= 1,
            Tok::Import if depth == 0 => {
                let span = toks[i].span;
                let local = matches!(toks.get(i + 1).map(|t| &t.kind), Some(Tok::At));
                let mut path = Vec::new();
                let mut alias = None;
                let mut j = i + 1;
                while j < toks.len() && !matches!(toks[j].kind, Tok::Semi | Tok::Eof | Tok::Import) {
                    // A segment follows a `/`, so neither the anchor
                    // (`binz`, `@root`) nor the `.binz` extension is one.
                    match &toks[j].kind {
                        Tok::Ident(s) if matches!(toks[j - 1].kind, Tok::Slash) => path.push(s.clone()),
                        Tok::Ident(s) if matches!(toks[j - 1].kind, Tok::As) => alias = Some(s.clone()),
                        _ => {}
                    }
                    j += 1;
                }
                if let Some(last) = path.last().cloned() {
                    o.imports.push(Import { local, path, binding: alias.unwrap_or(last), span });
                }
                i = j;
                continue;
            }
            Tok::Struct if depth == 0 => {
                if let (Some(Tok::Ident(name)), Some(Tok::LBrace)) =
                    (toks.get(i + 1).map(|t| &t.kind), toks.get(i + 2).map(|t| &t.kind))
                {
                    let mut fields = Vec::new();
                    let mut j = i + 3;
                    while j < toks.len() && !matches!(toks[j].kind, Tok::RBrace | Tok::Eof) {
                        match (&toks[j].kind, toks.get(j + 1).map(|t| &t.kind)) {
                            (Tok::Ident(f), Some(Tok::Colon)) => {
                                let end = type_end(toks, j + 2);
                                fields.push((f.clone(), render(&toks[j + 2..end])));
                                j = end;
                            }
                            _ => j += 1,
                        }
                    }
                    o.structs.push(StructSym { name: name.clone(), fields });
                    // Past the closing brace, whose opening one was never
                    // counted.
                    i = j + 1;
                    continue;
                }
            }
            Tok::Function if depth == 0 => {
                if let (Some(Tok::Ident(name)), Some(Tok::LParen)) =
                    (toks.get(i + 1).map(|t| &t.kind), toks.get(i + 2).map(|t| &t.kind))
                {
                    let is_test = i >= 2
                        && toks[i - 2].kind == Tok::At
                        && toks[i - 1].kind == Tok::Ident("test".into());
                    let (ps, close) = params(toks, i + 2);
                    let ps: Vec<String> = ps.iter().map(|(n, t)| format!("{}: {}", n, t)).collect();
                    let mut sig = format!("({})", ps.join(", "));
                    let mut j = close + 1;
                    if matches!(toks.get(j).map(|t| &t.kind), Some(Tok::Colon)) {
                        let end = type_end(toks, j + 1);
                        sig.push_str(&format!(": {}", render(&toks[j + 1..end])));
                        j = end;
                    }
                    o.fns.push(FnSym { name: name.clone(), sig, is_test });
                    i = j;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    o
}

struct Local {
    name: String,
    ty: String,
    detail: &'static str,
}

/// What an unclosed bracket before the cursor opened.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Open {
    Paren,
    Bracket,
    /// The `<` of `cast<`, `Vector<` or `HashMap<`; never a comparison.
    Angle,
    /// The body of `struct Name { ... }`.
    StructBody,
    Brace,
}

/// The locals alive at `at`, innermost last, and the brackets still open
/// there.
fn scan(toks: &[Token], at: Span) -> (Vec<Local>, Vec<Open>) {
    let mut scopes: Vec<Vec<Local>> = Vec::new();
    let mut pending: Vec<Local> = Vec::new();
    let mut open: Vec<Open> = Vec::new();
    let mut i = 0;
    while i < toks.len() && before(toks[i].span, at) {
        let prev = if i > 0 { Some(&toks[i - 1].kind) } else { None };
        match &toks[i].kind {
            Tok::LBrace => {
                let is_struct = i >= 2 && toks[i - 2].kind == Tok::Struct;
                open.push(if is_struct { Open::StructBody } else { Open::Brace });
                scopes.push(std::mem::take(&mut pending));
            }
            Tok::RBrace => {
                open.pop();
                scopes.pop();
            }
            Tok::LParen => {
                open.push(Open::Paren);
                // `function name(` and `stub mod.name(`: the parameters are
                // the first locals of the body that follows.
                let header = matches!(prev, Some(Tok::Ident(_)))
                    && i >= 2
                    && (toks[i - 2].kind == Tok::Function
                        || (i >= 4 && toks[i - 2].kind == Tok::Dot && toks[i - 4].kind == Tok::Stub));
                if header {
                    let (ps, _) = params(toks, i);
                    pending = ps
                        .into_iter()
                        .map(|(name, ty)| Local { name, ty, detail: "parameter" })
                        .collect();
                }
            }
            Tok::RParen | Tok::RBracket => {
                open.pop();
            }
            Tok::LBracket => open.push(Open::Bracket),
            Tok::Lt if matches!(prev, Some(Tok::Cast | Tok::Container(_) | Tok::Map(_))) => {
                open.push(Open::Angle)
            }
            Tok::Gt if open.last() == Some(&Open::Angle) => {
                open.pop();
            }
            Tok::Const | Tok::Var => {
                if let (Some(Tok::Ident(name)), Some(Tok::Colon)) =
                    (toks.get(i + 1).map(|t| &t.kind), toks.get(i + 2).map(|t| &t.kind))
                {
                    let end = type_end(toks, i + 3);
                    let detail = if toks[i].kind == Tok::Const { "const" } else { "var" };
                    let local = Local { name: name.clone(), ty: render(&toks[i + 3..end]), detail };
                    if let Some(s) = scopes.last_mut() {
                        s.push(local);
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    (scopes.into_iter().flatten().collect(), open)
}

/// `@root` for a file that is not necessarily the one `binz` will be handed:
/// the nearest directory, from the file's own upwards, under which every
/// `@root/...` import of the file exists. With no local import there is no
/// evidence either way, and the file's own directory is what `binz run` on it
/// would use.
pub fn root_for(file: &Path, imports: &[Vec<String>]) -> PathBuf {
    let dir = file.parent().unwrap_or(Path::new(".")).to_path_buf();
    if imports.is_empty() {
        return dir;
    }
    for d in dir.ancestors() {
        let all = imports.iter().all(|segs| module_path(d, segs).is_file());
        if all {
            return d.to_path_buf();
        }
    }
    dir
}

pub fn module_path(root: &Path, segs: &[String]) -> PathBuf {
    let mut p = root.to_path_buf();
    for s in segs {
        p.push(s);
    }
    p.set_extension("binz");
    p
}

/// Whether the cursor, at the end of `line`, is inside a string literal or a
/// line comment -- where nothing is completed.
fn in_text(line: &str) -> bool {
    let mut in_str = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if in_str => {
                chars.next();
            }
            '"' => in_str = !in_str,
            '/' if !in_str && chars.peek() == Some(&'/') => return true,
            _ => {}
        }
    }
    in_str
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Everything the file itself can name at `at`.
pub struct Buffer<'a> {
    pub src: &'a str,
    /// The file on disk, when there is one: local imports resolve against it.
    pub path: Option<&'a Path>,
    /// Reads another file of the project, preferring an open editor buffer.
    pub read: &'a dyn Fn(&Path) -> Option<String>,
}

/// The completions at `line` / `col`, both counted from 0 and `col` in
/// characters.
pub fn complete(buf: &Buffer, line: u32, col: u32) -> Vec<Item> {
    let text = buf.src.lines().nth(line as usize).unwrap_or("");
    let head: String = text.chars().take(col as usize).collect();
    if in_text(&head) {
        return Vec::new();
    }
    let toks = tokens(buf.src);
    let out = outline(&toks);

    let trimmed = head.trim_start();
    if let Some(rest) = trimmed.strip_prefix("import") {
        if rest.is_empty() || rest.starts_with(char::is_whitespace) {
            return complete_import(buf, &out, rest.trim_start());
        }
    }

    let word_len = head.chars().rev().take_while(|c| is_ident(*c)).count();
    let lead: String = head.chars().take(head.chars().count() - word_len).collect();
    // The cursor, moved to the start of the word being typed, so that word
    // is not mistaken for a token before it.
    let at = Span { line: line + 1, col: (head.chars().count() - word_len) as u32 + 1 };
    let (locals, open) = scan(&toks, at);

    if let Some(base) = lead.strip_suffix('.') {
        return complete_member(buf, &out, &locals, base);
    }
    if lead.ends_with('@') {
        let tags: &[&str] = if open.last() == Some(&Open::StructBody) {
            &["field"]
        } else if open.is_empty() {
            &["test", "json"]
        } else {
            &[]
        };
        return tags.iter().map(|t| item(t, ItemKind::Keyword, "tag")).collect();
    }

    let prior: Vec<&Token> = toks.iter().filter(|t| before(t.span, at) && t.kind != Tok::Eof).collect();
    if wants_type(&prior, &open) {
        return types(&out);
    }

    if open.is_empty() {
        return TOP_KEYWORDS.iter().map(|k| item(k, ItemKind::Keyword, "keyword")).collect();
    }
    let mut items: Vec<Item> = Vec::new();
    for l in locals.iter().rev() {
        if !items.iter().any(|i| i.label == l.name) {
            items.push(item(&l.name, ItemKind::Variable, format!("{} {}: {}", l.detail, l.name, l.ty)));
        }
    }
    for f in out.fns.iter().filter(|f| !f.is_test) {
        items.push(item(&f.name, ItemKind::Function, format!("function {}{}", f.name, f.sig)));
    }
    for i in &out.imports {
        items.push(item(&i.binding, ItemKind::Module, i.path.join("/")));
    }
    for s in &out.structs {
        items.push(item(&s.name, ItemKind::Struct, "struct"));
    }
    items.push(item("Error", ItemKind::Struct, "struct Error { reason: str, stacktrace: str, code: i32 }"));
    for k in STATEMENT_KEYWORDS {
        items.push(item(k, ItemKind::Keyword, "keyword"));
    }
    items
}

/// Whether what comes next is a type: after the `:` of a declaration, a
/// parameter or a return, inside `cast<...>` and a container's `<...>`,
/// after a `*` in any of those, and inside `function(...)`.
fn wants_type(prior: &[&Token], open: &[Open]) -> bool {
    let mut n = prior.len();
    while n > 0 && prior[n - 1].kind == Tok::Star {
        n -= 1;
    }
    let kind = |k: usize| if k < n { Some(&prior[n - 1 - k].kind) } else { None };
    match kind(0) {
        Some(Tok::Colon) => match kind(1) {
            // `): T` -- a return type, of a function or of a function type.
            Some(Tok::RParen) => true,
            Some(Tok::Ident(_)) => {
                matches!(kind(2), Some(Tok::Const | Tok::Var))
                    || matches!(open.last(), Some(Open::Paren | Open::StructBody))
            }
            _ => false,
        },
        Some(Tok::Lt | Tok::Comma) => open.last() == Some(&Open::Angle),
        Some(Tok::LParen) => matches!(kind(1), Some(Tok::Function)),
        _ => false,
    }
}

fn types(out: &Outline) -> Vec<Item> {
    let mut items: Vec<Item> = PRIMITIVES.iter().map(|t| item(t, ItemKind::Type, "type")).collect();
    for g in GENERICS {
        items.push(item(g, ItemKind::Type, "container"));
    }
    for s in &out.structs {
        items.push(item(&s.name, ItemKind::Struct, "struct"));
    }
    items.push(item("Error", ItemKind::Struct, "struct"));
    items.push(item("function", ItemKind::Keyword, "function type"));
    items
}

/// After `import`: the two anchors, a standard library module, or a
/// directory or `.binz` file under `@root`.
fn complete_import(buf: &Buffer, out: &Outline, rest: &str) -> Vec<Item> {
    if rest.starts_with("binz/") {
        return stdlib::MODULES.iter().map(|m| item(m, ItemKind::Module, format!("binz/{}", m))).collect();
    }
    if let Some(sub) = rest.strip_prefix("@root/") {
        let file = match buf.path {
            Some(p) => p,
            None => return Vec::new(),
        };
        let root = root_for(file, &out.local_imports());
        let dir = match sub.rfind('/') {
            Some(k) => root.join(&sub[..k]),
            None => root,
        };
        let mut items = Vec::new();
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => return items,
        };
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || Some(p.as_path()) == buf.path {
                continue;
            }
            if p.is_dir() {
                items.push(item(&format!("{}/", name), ItemKind::Folder, "directory"));
            } else if p.extension().map(|x| x == "binz").unwrap_or(false) {
                items.push(item(&name, ItemKind::File, "module"));
            }
        }
        items.sort_by(|a, b| a.label.cmp(&b.label));
        return items;
    }
    if rest.contains('/') {
        return Vec::new();
    }
    vec![item("binz/", ItemKind::Folder, "the standard library"), item("@root/", ItemKind::Folder, "this project")]
}

/// After `base.`: a module's members, or a struct's fields.
fn complete_member(buf: &Buffer, out: &Outline, locals: &[Local], base: &str) -> Vec<Item> {
    // `(*p).` reads through the pointer; nothing else does.
    let (chain, deref) = match base.strip_suffix(')') {
        Some(inner) => {
            let start = inner.rfind("(*").map(|k| k + 2);
            match start {
                Some(k) if inner[k..].chars().all(is_ident) => (inner[k..].to_string(), true),
                _ => return Vec::new(),
            }
        }
        None => {
            let n = base.chars().rev().take_while(|c| is_ident(*c) || *c == '.').count();
            (base.chars().skip(base.chars().count() - n).collect(), false)
        }
    };
    let segs: Vec<&str> = chain.split('.').collect();
    if segs.iter().any(|s| s.is_empty()) {
        return Vec::new();
    }

    if segs.len() == 1 && !deref {
        if let Some(im) = out.imports.iter().find(|i| i.binding == segs[0]) {
            return module_members(buf, out, im);
        }
    }

    let mut ty = match locals.iter().rev().find(|l| l.name == segs[0]) {
        Some(l) => l.ty.clone(),
        None => return Vec::new(),
    };
    if deref {
        ty = match ty.strip_prefix('*') {
            Some(t) => t.to_string(),
            None => return Vec::new(),
        };
    }
    for field in &segs[1..] {
        ty = match out.fields_of(&ty).and_then(|fs| fs.into_iter().find(|(n, _)| n == field)) {
            Some((_, t)) => t,
            None => return Vec::new(),
        };
    }
    match out.fields_of(&ty) {
        Some(fields) => fields
            .iter()
            .map(|(n, t)| item(n, ItemKind::Field, format!("{}: {}", n, t)))
            .collect(),
        None => Vec::new(),
    }
}

fn module_members(buf: &Buffer, out: &Outline, im: &Import) -> Vec<Item> {
    if !im.local {
        let m = im.path.join("/");
        let m = m.as_str();
        let structs = [error_struct()];
        let mut items: Vec<Item> = stdlib::NATIVES
            .iter()
            .filter(|n| n.module == m)
            .map(|n| {
                let sig = type_name(&(n.sig)(), &structs);
                let sig = sig.strip_prefix("function").unwrap_or(&sig);
                item(n.name, ItemKind::Function, format!("{}.{}{}", m, n.name, sig))
            })
            .collect();
        for f in stdlib::FORMS.iter().filter(|f| f.module == m) {
            let args = if f.arity == 1 { "argument" } else { "arguments" };
            items.push(item(
                f.name,
                ItemKind::Function,
                format!("{}.{}, generic, {} {}", m, f.name, f.arity, args),
            ));
        }
        return items;
    }
    let file = match buf.path {
        Some(p) => p,
        None => return Vec::new(),
    };
    let path = module_path(&root_for(file, &out.local_imports()), &im.path);
    let src = match (buf.read)(&path) {
        Some(s) => s,
        None => return Vec::new(),
    };
    outline(&tokens(&src))
        .fns
        .iter()
        .filter(|f| !f.is_test && f.name != "main")
        .map(|f| item(&f.name, ItemKind::Function, format!("{}.{}{}", im.binding, f.name, f.sig)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The labels offered where `|` is in `src`.
    fn at(src: &str) -> Vec<String> {
        let k = src.find('|').expect("no cursor");
        let before = &src[..k];
        let line = before.matches('\n').count() as u32;
        let col = before.rsplit('\n').next().unwrap().chars().count() as u32;
        let text = src.replacen('|', "", 1);
        let read = |_: &Path| None;
        let buf = Buffer { src: &text, path: None, read: &read };
        complete(&buf, line, col).into_iter().map(|i| i.label).collect()
    }

    const HEAD: &str = "import binz/io;\nimport binz/string;\n\
                        struct Point {\n    x: i64,\n    y: i64,\n}\n\
                        struct Line {\n    from: Point,\n    to: Point,\n}\n\
                        function add(a: i64, b: i64): i64 {\n    return a + b;\n}\n\
                        @test function sumsTwo(): void {\n    return;\n}\n";

    #[test]
    fn a_stdlib_module_offers_its_members_and_nothing_else() {
        let got = at(&format!("{}function main(): i32 {{\n    io.|\n}}\n", HEAD));
        assert_eq!(got, vec!["print"]);
        let got = at(&format!("{}function main(): i32 {{\n    string.sta|\n}}\n", HEAD));
        assert!(got.contains(&"startsWith".to_string()) && got.contains(&"size".to_string()));
    }

    #[test]
    fn a_struct_offers_its_fields_through_a_chain() {
        let src = format!(
            "{}function main(): i32 {{\n    const l: Line = Line {{ from: Point {{ x: 1, y: 2 }}, to: Point {{ x: 3, y: 4 }} }};\n    l.from.|\n}}\n",
            HEAD
        );
        assert_eq!(at(&src), vec!["x", "y"]);
        let src = format!("{}function main(): i32 {{\n    const l: Line = x;\n    l.|\n}}\n", HEAD);
        assert_eq!(at(&src), vec!["from", "to"]);
    }

    #[test]
    fn a_pointer_offers_fields_only_through_a_deref() {
        let src = format!("{}function shift(p: *Point): void {{\n    (*p).|\n}}\n", HEAD);
        assert_eq!(at(&src), vec!["x", "y"]);
        let src = format!("{}function shift(p: *Point): void {{\n    p.|\n}}\n", HEAD);
        assert!(at(&src).is_empty());
    }

    #[test]
    fn an_error_offers_its_three_fields() {
        let src = format!("{}function main(): i32 {{\n    var err: Error = Error{{}};\n    err.|\n}}\n", HEAD);
        assert_eq!(at(&src), vec!["reason", "stacktrace", "code"]);
    }

    #[test]
    fn a_block_scope_ends_at_its_brace() {
        let src = format!(
            "{}function main(): i32 {{\n    var total: i64 = 0;\n    {{\n        var i: i64 = 0;\n    }}\n    |\n}}\n",
            HEAD
        );
        let got = at(&src);
        assert!(got.contains(&"total".to_string()));
        assert!(!got.contains(&"i".to_string()));
        assert!(got.contains(&"add".to_string()) && got.contains(&"io".to_string()));
        assert!(!got.contains(&"sumsTwo".to_string()), "a test is not callable");
    }

    #[test]
    fn parameters_are_locals_of_the_body() {
        let got = at(&format!("{}function twice(n: i64): i64 {{\n    return |\n}}\n", HEAD));
        assert!(got.contains(&"n".to_string()));
        let got = at(&format!("{}function other(): i64 {{\n    return |\n}}\n", HEAD));
        assert!(!got.contains(&"n".to_string()) && !got.contains(&"a".to_string()));
    }

    #[test]
    fn a_type_position_offers_types() {
        for src in [
            "function main(): i32 {\n    const x: |\n}\n",
            "function f(a: |",
            "function f(a: i32): |",
            "function main(): i32 {\n    const x: str = cast<|\n}\n",
            "function main(): i32 {\n    const m: HashMap<str, |\n}\n",
            "function f(p: *|",
        ] {
            let got = at(&format!("{}{}", HEAD, src));
            assert!(got.contains(&"i64".to_string()), "{} -> {:?}", src, got);
            assert!(got.contains(&"Point".to_string()), "{} -> {:?}", src, got);
            assert!(!got.contains(&"while".to_string()), "{} -> {:?}", src, got);
        }
    }

    #[test]
    fn a_struct_literal_field_is_a_value_not_a_type() {
        let src = format!("{}function main(): i32 {{\n    const x: i64 = 1;\n    const p: Point = Point {{ x: |\n}}\n", HEAD);
        let got = at(&src);
        assert!(got.contains(&"x".to_string()) && !got.contains(&"i64".to_string()), "{:?}", got);
    }

    #[test]
    fn imports_offer_anchors_then_modules() {
        assert_eq!(at("import |"), vec!["binz/", "@root/"]);
        let got = at("import binz/|");
        assert_eq!(got.len(), stdlib::MODULES.len());
        assert!(got.contains(&"json".to_string()));
    }

    #[test]
    fn tags_depend_on_where_they_are_written() {
        assert_eq!(at("@|"), vec!["test", "json"]);
        assert_eq!(at("@json struct P {\n    @|"), vec!["field"]);
    }

    #[test]
    fn the_top_level_offers_only_items() {
        assert_eq!(at(&format!("{}|", HEAD)), vec!["import", "struct", "function"]);
    }

    #[test]
    fn nothing_is_offered_in_a_string_or_a_comment() {
        assert!(at("function main(): i32 {\n    io.print(\"io.|\n}\n").is_empty());
        assert!(at("function main(): i32 {\n    // io.|\n}\n").is_empty());
    }

    #[test]
    fn a_half_typed_file_still_completes() {
        // An unterminated string above the cursor must not hide the rest.
        let src = format!("{}function main(): i32 {{\n    io.print(\"oops\n    const q: Point = x;\n    q.|\n}}\n", HEAD);
        assert_eq!(at(&src), vec!["x", "y"]);
    }

    #[test]
    fn a_local_module_offers_its_functions() {
        let dir = std::env::temp_dir().join("binz_complete_local_module");
        std::fs::create_dir_all(dir.join("utils")).unwrap();
        std::fs::write(
            dir.join("utils/math.binz"),
            "function add(a: i32, b: i32): i32 {\n    return a + b;\n}\n\
             @test function addsTwo(): void {\n    return;\n}\n",
        )
        .unwrap();
        let entry = dir.join("main.binz");
        let src = "import @root/utils/math.binz;\nfunction main(): i32 {\n    math.\n}\n";
        let read = |p: &Path| std::fs::read_to_string(p).ok();
        let buf = Buffer { src, path: Some(&entry), read: &read };
        let got: Vec<String> = complete(&buf, 2, 9).into_iter().map(|i| i.detail).collect();
        assert_eq!(got, vec!["math.add(a: i32, b: i32): i32"]);

        // ... and the directory it lives in, when an import is being written.
        let buf = Buffer { src: "import @root/", path: Some(&entry), read: &read };
        let got: Vec<String> = complete(&buf, 0, 13).into_iter().map(|i| i.label).collect();
        assert!(got.contains(&"utils/".to_string()), "{:?}", got);
    }

    #[test]
    fn root_is_where_every_local_import_resolves() {
        let dir = std::env::temp_dir().join("binz_complete_root_for");
        std::fs::create_dir_all(dir.join("modules/number")).unwrap();
        std::fs::write(dir.join("modules/number/format.binz"), "").unwrap();
        let module = dir.join("modules/rates.binz");
        let imports = vec![vec!["modules".to_string(), "number".to_string(), "format".to_string()]];
        assert_eq!(root_for(&module, &imports), dir);
        assert_eq!(root_for(&module, &[]), dir.join("modules"));
    }
}
