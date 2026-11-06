//! The wire side of `binz/http`: routes, reading a request off a socket and
//! writing a response back. Everything that touches binZ values -- building
//! the `Request` a handler is handed, reading the `Response` it returns --
//! lives in the virtual machine; this file only speaks HTTP/1.1.
//!
//! The server is deliberately small. One request per connection
//! (`Connection: close`), one connection at a time, no TLS, no chunked
//! request bodies. That is enough to put a handful of routes behind a port,
//! which is all `binz/http` promises so far.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// The request line plus every header may not exceed this.
const MAX_HEAD: usize = 64 * 1024;
/// Nor may a body.
const MAX_BODY: usize = 8 * 1024 * 1024;
/// A client that stops sending stalls every client behind it, since the
/// server takes one at a time -- so it is not waited on for long.
const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// One segment of a route: `/users/:id` is `[Literal("users"), Param("id")]`.
#[derive(Debug, Clone, PartialEq)]
pub enum Seg {
    Literal(String),
    Param(String),
}

#[derive(Debug, Clone)]
pub struct Route {
    pub method: &'static str,
    pub pattern: String,
    pub segs: Vec<Seg>,
    /// The function id of the handler.
    pub handler: u32,
}

/// Splits a route pattern, or says why it is not one. A pattern starts with
/// `/`, and a `:name` segment captures exactly one path segment.
pub fn parse_pattern(p: &str) -> Result<Vec<Seg>, String> {
    if !p.starts_with('/') {
        return Err(format!("the route `{}` must start with `/`", p));
    }
    if p.contains('?') || p.contains('#') {
        return Err(format!(
            "the route `{}` is a path only; the query string arrives in `req.query`",
            p
        ));
    }
    let mut segs = Vec::new();
    for s in segments(p) {
        if let Some(name) = s.strip_prefix(':') {
            if name.is_empty() {
                return Err(format!("the route `{}` has a `:` with no name after it", p));
            }
            if segs.iter().any(|x| *x == Seg::Param(name.to_string())) {
                return Err(format!("the route `{}` names `:{}` twice", p, name));
            }
            segs.push(Seg::Param(name.to_string()));
        } else {
            segs.push(Seg::Literal(s.to_string()));
        }
    }
    Ok(segs)
}

/// The non-empty segments of a path, so `/a/b`, `/a/b/` and `/a//b` are all
/// `["a", "b"]` -- the same route, as Express treats them.
fn segments(path: &str) -> impl Iterator<Item = &str> {
    path.split('/').filter(|s| !s.is_empty())
}

/// The `:name` captures if `path` (already stripped of its query string)
/// matches the route, decoded.
pub fn match_route(segs: &[Seg], path: &str) -> Option<Vec<(String, String)>> {
    let parts: Vec<&str> = segments(path).collect();
    if parts.len() != segs.len() {
        return None;
    }
    let mut params = Vec::new();
    for (seg, part) in segs.iter().zip(parts) {
        match seg {
            Seg::Literal(l) => {
                if decode(part, false) != *l {
                    return None;
                }
            }
            Seg::Param(name) => params.push((name.clone(), decode(part, false))),
        }
    }
    Some(params)
}

/// Percent-decoding. In a query string `+` is also a space. Bytes that do
/// not decode to UTF-8 are replaced, rather than failing the request.
pub fn decode(s: &str, plus_is_space: bool) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                match (hex(b[i + 1]), hex(b[i + 2])) {
                    (Some(h), Some(l)) => {
                        out.push(h * 16 + l);
                        i += 3;
                        continue;
                    }
                    _ => out.push(b'%'),
                }
            }
            b'+' if plus_is_space => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// `a=1&b=two` as pairs, decoded. A key with no `=` has the value `""`; a
/// repeated key keeps its last value, since `req.query` is a map.
pub fn parse_query(q: &str) -> Vec<(String, String)> {
    q.split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| match kv.split_once('=') {
            Some((k, v)) => (decode(k, true), decode(v, true)),
            None => (decode(kv, true), String::new()),
        })
        .collect()
}

/// A request as it came off the wire, before it is a binZ `Request`.
#[derive(Debug)]
pub struct Incoming {
    pub method: String,
    /// The path, without its query string, still percent-encoded.
    pub path: String,
    pub query: Vec<(String, String)>,
    /// Lowercased names. A repeated header is joined with `, `.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// Why a request could not be read: the status to answer with, or `None`
/// when the client went away and there is no one to answer.
pub type Refusal = Option<(u16, &'static str)>;

pub fn read_request(stream: &TcpStream) -> Result<Incoming, Refusal> {
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let mut r = BufReader::new(stream);
    let mut head = 0usize;

    let line = read_line(&mut r, &mut head)?;
    if line.is_empty() {
        return Err(None);
    }
    let mut parts = line.split(' ');
    let (method, target, version) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(m), Some(t), Some(v), None) => (m, t, v),
        _ => return Err(Some((400, "malformed request line"))),
    };
    if !version.starts_with("HTTP/1.") {
        return Err(Some((505, "only HTTP/1.x is served")));
    }
    if method.is_empty() || !method.bytes().all(|c| c.is_ascii_uppercase()) {
        return Err(Some((400, "malformed method")));
    }
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p, parse_query(q)),
        None => (target, Vec::new()),
    };
    if !path.starts_with('/') {
        return Err(Some((400, "the request target must be a path")));
    }

    let mut headers: Vec<(String, String)> = Vec::new();
    loop {
        let line = read_line(&mut r, &mut head)?;
        if line.is_empty() {
            break;
        }
        let (name, value) = match line.split_once(':') {
            Some((n, v)) if !n.is_empty() && !n.contains(' ') => (n.to_ascii_lowercase(), v.trim()),
            _ => return Err(Some((400, "malformed header"))),
        };
        match headers.iter_mut().find(|(n, _)| *n == name) {
            Some((_, v)) => {
                v.push_str(", ");
                v.push_str(value);
            }
            None => headers.push((name, value.to_string())),
        }
    }

    let header = |n: &str| headers.iter().find(|(k, _)| k == n).map(|(_, v)| v.as_str());
    if header("transfer-encoding").is_some() {
        return Err(Some((501, "chunked request bodies are not supported; send a Content-Length")));
    }
    let len = match header("content-length") {
        None => 0,
        Some(v) => match v.parse::<usize>() {
            Ok(n) => n,
            Err(_) => return Err(Some((400, "malformed Content-Length"))),
        },
    };
    if len > MAX_BODY {
        return Err(Some((413, "request body too large")));
    }
    let mut body = vec![0u8; len];
    if r.read_exact(&mut body).is_err() {
        return Err(None);
    }
    let body = match String::from_utf8(body) {
        Ok(b) => b,
        Err(_) => return Err(Some((400, "the request body is not UTF-8"))),
    };

    Ok(Incoming { method: method.to_string(), path: path.to_string(), query, headers, body })
}

/// One line of the head, without its `\r\n`.
fn read_line(r: &mut impl BufRead, head: &mut usize) -> Result<String, Refusal> {
    let mut buf = Vec::new();
    let limit = (MAX_HEAD - *head) as u64 + 1;
    match r.take(limit).read_until(b'\n', &mut buf) {
        Ok(0) => return Ok(String::new()),
        Ok(_) => {}
        Err(_) => return Err(None),
    }
    *head += buf.len();
    if *head > MAX_HEAD {
        return Err(Some((431, "request head too large")));
    }
    if buf.last() != Some(&b'\n') {
        return Err(None);
    }
    buf.pop();
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    String::from_utf8(buf).map_err(|_| Some((400, "the request head is not UTF-8")))
}

pub fn write_response(
    stream: &mut TcpStream,
    status: u16,
    headers: &[(String, String)],
    body: &str,
    send_body: bool,
) {
    let mut out = format!("HTTP/1.1 {} {}\r\n", status, reason(status));
    for (k, v) in headers {
        // A header the server owns is not the handler's to set: binZ would
        // otherwise send it twice.
        if k.eq_ignore_ascii_case("content-length") || k.eq_ignore_ascii_case("connection") {
            continue;
        }
        out.push_str(&format!("{}: {}\r\n", k, v));
    }
    out.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()));
    if send_body {
        out.push_str(body);
    }
    let _ = stream.write_all(out.as_bytes());
    let _ = stream.flush();
}

/// Whether a header name or value can be written as is: no line breaks, so
/// a value built from user input cannot inject a header of its own.
pub fn header_ok(name: &str, value: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|c| c.is_ascii_graphic() && c != b':')
        && !value.bytes().any(|c| c == b'\r' || c == b'\n')
}

pub fn reason(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        410 => "Gone",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        422 => "Unprocessable Content",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        505 => "HTTP Version Not Supported",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(s: &str) -> Seg {
        Seg::Literal(s.into())
    }

    #[test]
    fn patterns_split_into_literals_and_params() {
        assert_eq!(parse_pattern("/").unwrap(), vec![]);
        assert_eq!(
            parse_pattern("/users/:id/posts").unwrap(),
            vec![lit("users"), Seg::Param("id".into()), lit("posts")]
        );
        assert!(parse_pattern("users").is_err());
        assert!(parse_pattern("/a/:").is_err());
        assert!(parse_pattern("/a/:x/:x").is_err());
        assert!(parse_pattern("/a?b=1").is_err());
    }

    #[test]
    fn routes_match_whole_paths_and_capture_params() {
        let r = parse_pattern("/users/:id").unwrap();
        assert_eq!(match_route(&r, "/users/42"), Some(vec![("id".into(), "42".into())]));
        assert_eq!(match_route(&r, "/users/42/"), Some(vec![("id".into(), "42".into())]));
        assert_eq!(match_route(&r, "/users"), None);
        assert_eq!(match_route(&r, "/users/42/x"), None);
        assert_eq!(match_route(&r, "/posts/42"), None);
        assert_eq!(
            match_route(&r, "/users/ana%20lu"),
            Some(vec![("id".into(), "ana lu".into())])
        );
        assert_eq!(match_route(&parse_pattern("/").unwrap(), "/"), Some(vec![]));
    }

    #[test]
    fn query_strings_decode() {
        assert_eq!(
            parse_query("a=1&b=two+words&c&d=%2Fx"),
            vec![
                ("a".into(), "1".into()),
                ("b".into(), "two words".into()),
                ("c".into(), "".into()),
                ("d".into(), "/x".into()),
            ]
        );
        assert_eq!(decode("100%", false), "100%");
        assert_eq!(decode("%zz", false), "%zz");
        assert_eq!(decode("a+b", false), "a+b");
    }

    #[test]
    fn header_values_cannot_break_lines() {
        assert!(header_ok("X-Id", "42"));
        assert!(!header_ok("X-Id", "42\r\nSet-Cookie: a=b"));
        assert!(!header_ok("X Id", "42"));
        assert!(!header_ok("", "42"));
    }
}
