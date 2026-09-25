//! `binz/json`: reading JSON text into an `@json` struct and writing one
//! back out, driven by the schemas the compiler put in the artifact.
//!
//! Reading happens in two steps -- text to a `Json` tree, then the tree to
//! the struct's slots -- so a malformed document is refused before a single
//! field is looked at, and the destination is written only once the whole
//! value has been read. A failed `json.parse` leaves it exactly as it was.

use std::rc::Rc;

use crate::bytecode::{JType, Schema};
use crate::obj;
use crate::vm::{format_f64, Value};

/// The `code` of the `Error` a `json.*` call answers. `0` is success.
pub const OK: i32 = 0;
/// The text is not JSON.
pub const SYNTAX: i32 = 1;
/// A value is JSON, but not what the field holds -- a string where an `i32`
/// goes, `1.5` for an integer, a number too large for its field.
pub const TYPE: i32 = 2;
/// A field of the struct has no key in the object.
pub const MISSING: i32 = 3;
/// An object names the same key twice, and there is no telling which one
/// was meant.
pub const DUPLICATE: i32 = 4;
/// `json.stringify` met an `f64` JSON cannot spell: NaN or an infinity.
pub const UNREPRESENTABLE: i32 = 5;

/// Nesting deeper than this is refused rather than recursed into, so a
/// hostile document cannot overflow the host stack.
const MAX_DEPTH: usize = 512;

#[derive(Debug)]
pub struct Failure {
    pub code: i32,
    pub reason: String,
}

fn fail<T>(code: i32, reason: String) -> Result<T, Failure> {
    Err(Failure { code, reason })
}

#[derive(Debug)]
enum Json {
    Null,
    Bool(bool),
    /// The number exactly as written, so an integer field can refuse `1.0`
    /// and an `i64` keeps every digit.
    Num(String),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn kind(&self) -> &'static str {
        match self {
            Json::Null => "null",
            Json::Bool(_) => "a boolean",
            Json::Num(_) => "a number",
            Json::Str(_) => "a string",
            Json::Arr(_) => "an array",
            Json::Obj(_) => "an object",
        }
    }
}

// --------------------------------------------------------------- reading

/// `json.parse`: the slots of one `@json` struct, read from `text`.
pub fn parse(schemas: &[Schema], schema: u32, text: &str) -> Result<Vec<Value>, Failure> {
    let doc = Reader { src: text.as_bytes(), pos: 0 }.document()?;
    let sc = &schemas[schema as usize];
    let mut out = vec![Value::Void; sc.size as usize];
    read(schemas, &JType::Struct(schema), &doc, &mut out, "$")?;
    Ok(out)
}

struct Reader<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn document(mut self) -> Result<Json, Failure> {
        let v = self.value(0)?;
        self.ws();
        if self.pos < self.src.len() {
            return self.syntax("unexpected text after the JSON value");
        }
        Ok(v)
    }

    /// Line and column of the current position, counted in characters, for
    /// a reason that points at the mistake.
    fn place(&self) -> String {
        let upto = String::from_utf8_lossy(&self.src[..self.pos.min(self.src.len())]).into_owned();
        let line = upto.matches('\n').count() + 1;
        let col = upto.rsplit('\n').next().map(|l| l.chars().count()).unwrap_or(0) + 1;
        format!("line {}, column {}", line, col)
    }

    fn syntax<T>(&self, what: &str) -> Result<T, Failure> {
        fail(SYNTAX, format!("{}: {}", self.place(), what))
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn ws(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.peek() {
            self.pos += 1;
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, Failure> {
        if depth > MAX_DEPTH {
            return self.syntax("nested too deeply");
        }
        self.ws();
        match self.peek() {
            None => self.syntax("expected a value, found the end of the text"),
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'n') => self.word("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => self.syntax("expected a value"),
        }
    }

    fn word(&mut self, w: &str, v: Json) -> Result<Json, Failure> {
        if self.src[self.pos..].starts_with(w.as_bytes()) {
            self.pos += w.len();
            Ok(v)
        } else {
            self.syntax("expected a value")
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.pos;
        while let Some(b'0'..=b'9') = self.peek() {
            self.pos += 1;
        }
        self.pos - start
    }

    /// `-? (0 | [1-9][0-9]*) (. [0-9]+)? ([eE] [+-]? [0-9]+)?`, and nothing
    /// looser: no `+1`, no `01`, no `.5`, no `1.`.
    fn number(&mut self) -> Result<Json, Failure> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
                if let Some(b'0'..=b'9') = self.peek() {
                    return self.syntax("a number cannot start with `0`");
                }
            }
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return self.syntax("expected a digit"),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if self.digits() == 0 {
                return self.syntax("expected a digit after `.`");
            }
        }
        if let Some(b'e' | b'E') = self.peek() {
            self.pos += 1;
            if let Some(b'+' | b'-') = self.peek() {
                self.pos += 1;
            }
            if self.digits() == 0 {
                return self.syntax("expected a digit in the exponent");
            }
        }
        let text = std::str::from_utf8(&self.src[start..self.pos]).unwrap_or("0");
        Ok(Json::Num(text.to_string()))
    }

    fn hex4(&mut self) -> Result<u32, Failure> {
        let mut v = 0u32;
        for _ in 0..4 {
            let d = match self.peek().and_then(|b| (b as char).to_digit(16)) {
                Some(d) => d,
                None => return self.syntax("expected four hex digits after `\\u`"),
            };
            v = v * 16 + d;
            self.pos += 1;
        }
        Ok(v)
    }

    fn string(&mut self) -> Result<String, Failure> {
        self.pos += 1; // the opening quote
        let mut out: Vec<u8> = Vec::new();
        loop {
            let b = match self.peek() {
                None => return self.syntax("unterminated string"),
                Some(b) => b,
            };
            match b {
                b'"' => {
                    self.pos += 1;
                    break;
                }
                b'\\' => {
                    self.pos += 1;
                    let esc = match self.peek() {
                        None => return self.syntax("unterminated string"),
                        Some(e) => e,
                    };
                    self.pos += 1;
                    let c = match esc {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => self.unicode()?,
                        _ => {
                            self.pos -= 1;
                            return self.syntax("unknown escape sequence");
                        }
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
                0x00..=0x1f => return self.syntax("a control character must be escaped in a string"),
                _ => {
                    out.push(b);
                    self.pos += 1;
                }
            }
        }
        match String::from_utf8(out) {
            Ok(s) => Ok(s),
            Err(_) => self.syntax("a string is not valid UTF-8"),
        }
    }

    /// The character after `\u`, pairing surrogates the way UTF-16 does.
    fn unicode(&mut self) -> Result<char, Failure> {
        let hi = self.hex4()?;
        let code = if (0xD800..0xDC00).contains(&hi) {
            if !self.src[self.pos..].starts_with(b"\\u") {
                return self.syntax("a high surrogate must be followed by a low one");
            }
            self.pos += 2;
            let lo = self.hex4()?;
            if !(0xDC00..0xE000).contains(&lo) {
                return self.syntax("a high surrogate must be followed by a low one");
            }
            0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
        } else {
            hi
        };
        match char::from_u32(code) {
            Some(c) => Ok(c),
            None => self.syntax("a lone surrogate is not a character"),
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, Failure> {
        self.pos += 1;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Arr(items));
                }
                _ => return self.syntax("expected `,` or `]`"),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, Failure> {
        self.pos += 1;
        let mut entries: Vec<(String, Json)> = Vec::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Obj(entries));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return self.syntax("expected a key in double quotes");
            }
            let at = self.pos;
            let key = self.string()?;
            if entries.iter().any(|(k, _)| *k == key) {
                self.pos = at;
                return fail(
                    DUPLICATE,
                    format!("{}: the key \"{}\" appears twice", self.place(), key),
                );
            }
            self.ws();
            if self.peek() != Some(b':') {
                return self.syntax("expected `:` after the key");
            }
            self.pos += 1;
            let v = self.value(depth + 1)?;
            entries.push((key, v));
            self.ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Obj(entries));
                }
                _ => return self.syntax("expected `,` or `}`"),
            }
        }
    }
}

fn mismatch<T>(path: &str, want: &str, got: &Json) -> Result<T, Failure> {
    fail(TYPE, format!("`{}`: expected {}, found {}", path, want, got.kind()))
}

/// An integer field takes an integer as written: `1.0` and `1e3` are
/// numbers, but reading them into an `i32` would be a conversion binZ does
/// not make anywhere else either.
fn integer(path: &str, n: &str, width: &str) -> Result<i64, Failure> {
    if n.contains(['.', 'e', 'E']) {
        return fail(TYPE, format!("`{}`: expected an integer, found {}", path, n));
    }
    match n.parse::<i64>() {
        Ok(v) => Ok(v),
        Err(_) => fail(TYPE, format!("`{}`: {} does not fit in an {}", path, n, width)),
    }
}

/// Reads `v` as a `ty` into `out`, which is exactly that type's slots.
fn read(schemas: &[Schema], ty: &JType, v: &Json, out: &mut [Value], path: &str) -> Result<(), Failure> {
    match (ty, v) {
        (JType::Struct(sid), Json::Obj(entries)) => {
            // A key the struct does not declare is skipped: the struct says
            // what this program reads, not what the sender may add.
            for f in &schemas[*sid as usize].fields {
                let fpath = format!("{}.{}", path, f.key);
                let found = match entries.iter().find(|(k, _)| *k == f.key) {
                    Some((_, v)) => v,
                    None => return fail(MISSING, format!("`{}` is missing", fpath)),
                };
                let size = slots(schemas, &f.ty);
                let at = f.offset as usize;
                read(schemas, &f.ty, found, &mut out[at..at + size], &fpath)?;
            }
            Ok(())
        }
        (JType::Struct(_), other) => mismatch(path, "an object", other),
        (JType::Array(elem, len, size), Json::Arr(items)) => {
            if items.len() != *len as usize {
                return fail(
                    TYPE,
                    format!(
                        "`{}`: expected an array of exactly {}, found {} element(s)",
                        path,
                        len,
                        items.len()
                    ),
                );
            }
            let size = *size as usize;
            for (i, item) in items.iter().enumerate() {
                let ipath = format!("{}[{}]", path, i);
                read(schemas, elem, item, &mut out[i * size..(i + 1) * size], &ipath)?;
            }
            Ok(())
        }
        (JType::Array(..), other) => mismatch(path, "an array", other),
        _ => {
            out[0] = scalar(ty, v, path)?;
            Ok(())
        }
    }
}

/// A one-slot value: a scalar, a `str`, or a whole `Vector<T>`, whose
/// elements are one slot each too.
fn scalar(ty: &JType, v: &Json, path: &str) -> Result<Value, Failure> {
    Ok(match (ty, v) {
        (JType::Bool, Json::Bool(b)) => Value::Bool(*b),
        (JType::Bool, other) => return mismatch(path, "a boolean", other),
        (JType::I32, Json::Num(n)) => {
            let x = integer(path, n, "i32")?;
            match i32::try_from(x) {
                Ok(x) => Value::I32(x),
                Err(_) => return fail(TYPE, format!("`{}`: {} does not fit in an i32", path, n)),
            }
        }
        (JType::I64, Json::Num(n)) => Value::I64(integer(path, n, "i64")?),
        (JType::I32 | JType::I64, other) => return mismatch(path, "an integer", other),
        (JType::F64, Json::Num(n)) => match n.parse::<f64>() {
            Ok(x) if x.is_finite() => Value::F64(x),
            _ => return fail(TYPE, format!("`{}`: {} does not fit in an f64", path, n)),
        },
        (JType::F64, other) => return mismatch(path, "a number", other),
        (JType::Str, Json::Str(s)) => Value::Str(Rc::new(s.clone())),
        (JType::Str, other) => return mismatch(path, "a string", other),
        (JType::Vector(elem), Json::Arr(items)) => {
            let mut values = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                values.push(scalar(elem, item, &format!("{}[{}]", path, i))?);
            }
            obj::new_vector(values)
        }
        (JType::Vector(_), other) => return mismatch(path, "an array", other),
        (JType::Array(..) | JType::Struct(_), _) => unreachable!("`read` handles aggregates"),
    })
}

fn slots(schemas: &[Schema], ty: &JType) -> usize {
    match ty {
        JType::Struct(sid) => schemas[*sid as usize].size as usize,
        JType::Array(_, len, size) => (*len * *size) as usize,
        _ => 1,
    }
}

// --------------------------------------------------------------- writing

/// `json.stringify`: the struct whose slots start at `mem[base]`, as compact
/// JSON with its keys in declaration order. There is one spelling of a value,
/// so there is no pretty-printer and no key sorting.
pub fn stringify(schemas: &[Schema], schema: u32, mem: &[Value], base: usize) -> Result<String, Failure> {
    let mut out = String::new();
    write(schemas, &JType::Struct(schema), mem, base, "$", &mut out)?;
    Ok(out)
}

fn write(
    schemas: &[Schema],
    ty: &JType,
    mem: &[Value],
    at: usize,
    path: &str,
    out: &mut String,
) -> Result<(), Failure> {
    match ty {
        JType::Struct(sid) => {
            out.push('{');
            for (i, f) in schemas[*sid as usize].fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_str(&f.key, out);
                out.push(':');
                let fpath = format!("{}.{}", path, f.key);
                write(schemas, &f.ty, mem, at + f.offset as usize, &fpath, out)?;
            }
            out.push('}');
        }
        JType::Array(elem, len, size) => {
            out.push('[');
            for i in 0..*len as usize {
                if i > 0 {
                    out.push(',');
                }
                let ipath = format!("{}[{}]", path, i);
                write(schemas, elem, mem, at + i * *size as usize, &ipath, out)?;
            }
            out.push(']');
        }
        _ => write_value(ty, &mem[at], path, out)?,
    }
    Ok(())
}

fn write_value(ty: &JType, v: &Value, path: &str, out: &mut String) -> Result<(), Failure> {
    match (ty, v) {
        (_, Value::Bool(b)) => out.push_str(if *b { "true" } else { "false" }),
        (_, Value::I32(x)) => out.push_str(&x.to_string()),
        (_, Value::I64(x)) => out.push_str(&x.to_string()),
        (_, Value::F64(x)) => {
            if !x.is_finite() {
                return fail(
                    UNREPRESENTABLE,
                    format!("`{}`: {} has no JSON spelling", path, format_f64(*x)),
                );
            }
            out.push_str(&format_f64(*x));
        }
        (_, Value::Str(s)) => write_str(s, out),
        (JType::Vector(elem), Value::Obj(h)) => {
            out.push('[');
            for i in 0..obj::obj_len(h) {
                if i > 0 {
                    out.push(',');
                }
                let item = obj::obj_get(h, i).unwrap_or(Value::Void);
                write_value(elem, &item, &format!("{}[{}]", path, i), out)?;
            }
            out.push(']');
        }
        _ => unreachable!("the schema and the slot disagree"),
    }
    Ok(())
}

fn write_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(s: &str) -> Result<Json, Failure> {
        Reader { src: s.as_bytes(), pos: 0 }.document()
    }

    #[test]
    fn accepts_what_rfc_8259_accepts() {
        for ok in [
            "{}",
            "[]",
            " { \"a\" : [1, -0, 2.5, -1e3, 1E+2, true, false, null, \"x\"] } ",
            "\"\\u00e9\\ud83d\\ude00\\n\\/\"",
            "0",
        ] {
            assert!(doc(ok).is_ok(), "refused {}", ok);
        }
    }

    #[test]
    fn refuses_what_rfc_8259_refuses() {
        for bad in [
            "", "{", "[1,]", "{\"a\":1,}", "01", "+1", ".5", "1.", "1e", "'a'", "{a:1}",
            "[1] 2", "\"\\x\"", "\"\t\"", "\"\\ud800\"", "nul", "NaN",
        ] {
            let e = doc(bad).expect_err(bad);
            assert_eq!(e.code, SYNTAX, "{} -> {}", bad, e.reason);
        }
    }

    #[test]
    fn a_duplicate_key_is_its_own_failure() {
        let e = doc("{\"a\": 1,\n \"a\": 2}").unwrap_err();
        assert_eq!(e.code, DUPLICATE);
        assert_eq!(e.reason, "line 2, column 2: the key \"a\" appears twice");
    }

    #[test]
    fn depth_is_bounded() {
        let deep = "[".repeat(MAX_DEPTH + 2);
        assert_eq!(doc(&deep).unwrap_err().code, SYNTAX);
    }

    #[test]
    fn strings_round_trip_through_escapes() {
        let mut out = String::new();
        write_str("a\"b\\c\nd\u{1}é", &mut out);
        assert_eq!(out, "\"a\\\"b\\\\c\\nd\\u0001é\"");
        match doc(&out).unwrap() {
            Json::Str(s) => assert_eq!(s, "a\"b\\c\nd\u{1}é"),
            other => panic!("{:?}", other),
        }
    }
}
