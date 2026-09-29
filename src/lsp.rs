//! `binz lsp`: a language server over stdin and stdout.
//!
//! It answers two things -- completion, from `complete.rs`, and the
//! diagnostics of the real compiler, run in test mode so a module with no
//! `main` is checked too and a test is type checked as it is written. The
//! whole document is sent on every change: binZ files are small, and one sync
//! mode is one less thing to get wrong.
//!
//! JSON-RPC is read and written with `json.rs`, the same parser `binz/json`
//! uses, so the server adds no dependency to binZ.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use crate::complete::{self, Buffer, ItemKind};
use crate::compiler;
use crate::json::{self, Json};
use crate::loader;

fn obj(entries: Vec<(&str, Json)>) -> Json {
    Json::Obj(entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn num(n: i64) -> Json {
    Json::Num(n.to_string())
}

fn string(s: &str) -> Json {
    Json::Str(s.to_string())
}

/// How the client counts `character`. LSP defaults to UTF-16 code units;
/// UTF-32 is one per `char`, which is how the lexer counts columns.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Units {
    Utf16,
    Utf32,
}

struct Server {
    /// Open documents by URI, as the editor has them.
    docs: HashMap<String, String>,
    units: Units,
    shut_down: bool,
}

pub fn serve() -> i32 {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    let mut s = Server { docs: HashMap::new(), units: Units::Utf16, shut_down: false };
    loop {
        let msg = match read_message(&mut input) {
            Some(m) => m,
            // The client went away without `exit`.
            None => return 1,
        };
        let doc = match json::document(&msg) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let method = doc.get("method").and_then(|m| m.as_str()).unwrap_or("").to_string();
        if method == "exit" {
            return if s.shut_down { 0 } else { 1 };
        }
        let params = doc.get("params").cloned().unwrap_or(Json::Null);
        let out = s.handle(&method, &params);
        if let Some(id) = doc.get("id") {
            let reply = match out.result {
                Ok(result) => obj(vec![("jsonrpc", string("2.0")), ("id", id.clone()), ("result", result)]),
                Err((code, message)) => obj(vec![
                    ("jsonrpc", string("2.0")),
                    ("id", id.clone()),
                    ("error", obj(vec![("code", num(code)), ("message", string(&message))])),
                ]),
            };
            write_message(&mut output, &reply);
        }
        for n in out.notes {
            write_message(&mut output, &n);
        }
    }
}

/// `Content-Length: N`, a blank line, then N bytes of JSON.
fn read_message(input: &mut impl BufRead) -> Option<String> {
    let mut len: Option<usize> = None;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(v) = line.strip_prefix("Content-Length:") {
            len = v.trim().parse().ok();
        }
    }
    let mut body = vec![0u8; len?];
    input.read_exact(&mut body).ok()?;
    String::from_utf8(body).ok()
}

fn write_message(output: &mut impl Write, msg: &Json) {
    let text = msg.to_text();
    let _ = write!(output, "Content-Length: {}\r\n\r\n{}", text.len(), text);
    let _ = output.flush();
}

/// What one message produces: the reply, when it was a request, and the
/// notifications to send after it.
struct Outcome {
    result: Result<Json, (i64, String)>,
    notes: Vec<Json>,
}

impl Outcome {
    fn reply(result: Json) -> Self {
        Outcome { result: Ok(result), notes: Vec::new() }
    }
}

const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_REQUEST: i64 = -32600;

impl Server {
    fn handle(&mut self, method: &str, params: &Json) -> Outcome {
        if self.shut_down && method != "exit" {
            return Outcome { result: Err((INVALID_REQUEST, "the server is shut down".into())), notes: Vec::new() };
        }
        let uri = || {
            params.get("textDocument").and_then(|d| d.get("uri")).and_then(|u| u.as_str()).map(String::from)
        };
        match method {
            "initialize" => Outcome::reply(self.initialize(params)),
            "shutdown" => {
                self.shut_down = true;
                Outcome::reply(Json::Null)
            }
            "textDocument/didOpen" => {
                let text = params.get("textDocument").and_then(|d| d.get("text")).and_then(|t| t.as_str());
                if let (Some(u), Some(t)) = (uri(), text) {
                    self.docs.insert(u, t.to_string());
                }
                self.republish()
            }
            "textDocument/didChange" => {
                // Full sync: the last change is the whole document.
                let text = match params.get("contentChanges") {
                    Some(Json::Arr(cs)) => cs.last().and_then(|c| c.get("text")).and_then(|t| t.as_str()),
                    _ => None,
                };
                if let (Some(u), Some(t)) = (uri(), text) {
                    self.docs.insert(u, t.to_string());
                }
                self.republish()
            }
            // A file saved on disk can change what another open file sees.
            "textDocument/didSave" => self.republish(),
            "textDocument/didClose" => {
                let mut out = Outcome::reply(Json::Null);
                if let Some(u) = uri() {
                    self.docs.remove(&u);
                    out.notes.push(publish(&u, Vec::new()));
                }
                out
            }
            "textDocument/completion" => Outcome::reply(self.completion(params)),
            _ => Outcome {
                result: Err((METHOD_NOT_FOUND, format!("`{}` is not handled", method))),
                notes: Vec::new(),
            },
        }
    }

    fn initialize(&mut self, params: &Json) -> Json {
        let offered = params.get("capabilities").and_then(|c| c.get("general")).and_then(|g| g.get("positionEncodings"));
        if let Some(Json::Arr(encs)) = offered {
            if encs.iter().any(|e| e.as_str() == Some("utf-32")) {
                self.units = Units::Utf32;
            }
        }
        let encoding = if self.units == Units::Utf32 { "utf-32" } else { "utf-16" };
        obj(vec![
            (
                "capabilities",
                obj(vec![
                    ("positionEncoding", string(encoding)),
                    ("textDocumentSync", obj(vec![
                        ("openClose", Json::Bool(true)),
                        ("change", num(1)),
                        ("save", Json::Bool(true)),
                    ])),
                    (
                        "completionProvider",
                        obj(vec![("triggerCharacters", Json::Arr(vec![string("."), string("@"), string("/")]))]),
                    ),
                ]),
            ),
            (
                "serverInfo",
                obj(vec![("name", string("binz")), ("version", string(env!("CARGO_PKG_VERSION")))]),
            ),
        ])
    }

    /// The buffers of the editor by canonical path, so the compiler and the
    /// completer see what is on screen rather than what was last saved.
    fn overlay(&self) -> HashMap<PathBuf, String> {
        self.docs
            .iter()
            .filter_map(|(u, t)| {
                let p = uri_to_path(u)?;
                Some((std::fs::canonicalize(p).ok()?, t.clone()))
            })
            .collect()
    }

    /// Every open document is checked again on any change, since a module
    /// being edited can break -- or fix -- the files that import it.
    fn republish(&self) -> Outcome {
        let overlay = self.overlay();
        let mut out = Outcome::reply(Json::Null);
        for (uri, text) in &self.docs {
            let diags = match uri_to_path(uri) {
                Some(p) => diagnose(&p, text, &overlay, self.units),
                None => Vec::new(),
            };
            out.notes.push(publish(uri, diags));
        }
        out
    }

    fn completion(&self, params: &Json) -> Json {
        let uri = params.get("textDocument").and_then(|d| d.get("uri")).and_then(|u| u.as_str()).unwrap_or("");
        let pos = params.get("position");
        let line = pos.and_then(|p| p.get("line")).and_then(|l| l.as_i64()).unwrap_or(0).max(0) as u32;
        let character = pos.and_then(|p| p.get("character")).and_then(|c| c.as_i64()).unwrap_or(0).max(0) as u32;
        let src = match self.docs.get(uri) {
            Some(s) => s.as_str(),
            None => return Json::Arr(Vec::new()),
        };
        let text = src.lines().nth(line as usize).unwrap_or("");
        let col = to_chars(text, character, self.units);
        let path = uri_to_path(uri);
        let overlay = self.overlay();
        let read = |p: &Path| {
            let key = std::fs::canonicalize(p).ok()?;
            overlay.get(&key).cloned().or_else(|| std::fs::read_to_string(&key).ok())
        };
        let buf = Buffer { src, path: path.as_deref(), read: &read };
        let items = catch_unwind(AssertUnwindSafe(|| complete::complete(&buf, line, col))).unwrap_or_default();
        Json::Arr(
            items
                .into_iter()
                .map(|i| {
                    obj(vec![
                        ("label", string(&i.label)),
                        ("kind", num(kind_code(i.kind))),
                        ("detail", string(&i.detail)),
                    ])
                })
                .collect(),
        )
    }
}

/// `CompletionItemKind` in the LSP specification.
fn kind_code(k: ItemKind) -> i64 {
    match k {
        ItemKind::Function => 3,
        ItemKind::Field => 5,
        ItemKind::Variable => 6,
        ItemKind::Module => 9,
        ItemKind::Keyword => 14,
        ItemKind::File => 17,
        ItemKind::Folder => 19,
        ItemKind::Struct => 22,
        ItemKind::Type => 25,
    }
}

fn publish(uri: &str, diags: Vec<Json>) -> Json {
    obj(vec![
        ("jsonrpc", string("2.0")),
        ("method", string("textDocument/publishDiagnostics")),
        ("params", obj(vec![("uri", string(uri)), ("diagnostics", Json::Arr(diags))])),
    ])
}

/// The one error the compiler stops at, if there is one. An error in a file
/// this one imports is shown on the import that reaches it, since that is
/// the line of this file that brought it in.
fn diagnose(path: &Path, text: &str, overlay: &HashMap<PathBuf, String>, units: Units) -> Vec<Json> {
    let real = match std::fs::canonicalize(path) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let out = complete::outline(&complete::tokens(text));
    let root = complete::root_for(&real, &out.local_imports());
    let entry = real.display().to_string();
    let result = catch_unwind(AssertUnwindSafe(|| {
        loader::load_with(&entry, &root, overlay).and_then(|p| compiler::compile_tests(&p).map(|_| ()))
    }));
    let e = match result {
        Ok(Ok(())) => return Vec::new(),
        Ok(Err(e)) => e,
        Err(_) => return Vec::new(),
    };
    let here = e.file.as_deref().map(|f| same_file(Path::new(f), &real)).unwrap_or(true);
    let (line, col, msg) = if here {
        (e.span.line.saturating_sub(1), e.span.col.saturating_sub(1), e.msg.clone())
    } else {
        let file = e.file.clone().unwrap_or_default();
        let via = out.imports.iter().find(|i| {
            i.local && same_file(&complete::module_path(&root, &i.path), Path::new(&file))
        });
        let at = via.or_else(|| out.imports.iter().find(|i| i.local)).map(|i| i.span).unwrap_or_default();
        let shown = std::fs::canonicalize(&root)
            .ok()
            .and_then(|r| Path::new(&file).strip_prefix(r).ok().map(|p| format!("@root/{}", p.display())))
            .unwrap_or(file);
        (
            at.line.saturating_sub(1),
            at.col.saturating_sub(1),
            format!("{}:{}:{}: {}", shown, e.span.line, e.span.col, e.msg),
        )
    };
    let src_line = text.lines().nth(line as usize).unwrap_or("");
    // Underline the word the error points at, or one character.
    let len = src_line.chars().skip(col as usize).take_while(|c| c.is_ascii_alphanumeric() || *c == '_').count().max(1);
    let pos = |c: u32| obj(vec![("line", num(line as i64)), ("character", num(from_chars(src_line, c, units) as i64))]);
    vec![obj(vec![
        ("range", obj(vec![("start", pos(col)), ("end", pos(col + len as u32))])),
        ("severity", num(1)),
        ("source", string("binz")),
        ("message", string(&msg)),
    ])]
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// `file:///a%20b/c.binz` -> `/a b/c.binz`. Anything that is not a file
/// has no path, and is completed but never compiled.
fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(PathBuf::from(String::from_utf8(out).ok()?))
}

/// A client column, in its units, as a count of characters.
fn to_chars(line: &str, character: u32, units: Units) -> u32 {
    if units == Units::Utf32 {
        return character;
    }
    let mut n = 0u32;
    let mut count = 0u32;
    for c in line.chars() {
        if n >= character {
            break;
        }
        n += c.len_utf16() as u32;
        count += 1;
    }
    count
}

/// A count of characters, as a client column in its units.
fn from_chars(line: &str, chars: u32, units: Units) -> u32 {
    if units == Units::Utf32 {
        return chars;
    }
    let mut n: u32 = line.chars().take(chars as usize).map(|c| c.len_utf16() as u32).sum();
    // Past the end of the line: the characters that are not there count one each.
    n += chars.saturating_sub(line.chars().count() as u32);
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_decode_to_paths() {
        assert_eq!(uri_to_path("file:///a%20b/c.binz"), Some(PathBuf::from("/a b/c.binz")));
        assert_eq!(uri_to_path("untitled:1"), None);
    }

    #[test]
    fn columns_convert_between_utf16_and_chars() {
        // `😀` is one character and two UTF-16 units.
        let line = "a😀b";
        assert_eq!(to_chars(line, 3, Units::Utf16), 2);
        assert_eq!(from_chars(line, 2, Units::Utf16), 3);
        assert_eq!(to_chars(line, 2, Units::Utf32), 2);
    }

    #[test]
    fn a_message_is_framed_and_read_back() {
        let mut buf = Vec::new();
        write_message(&mut buf, &obj(vec![("a", num(1))]));
        let text = String::from_utf8(buf.clone()).unwrap();
        assert_eq!(text, "Content-Length: 7\r\n\r\n{\"a\":1}");
        assert_eq!(read_message(&mut buf.as_slice()).as_deref(), Some("{\"a\":1}"));
    }
}
