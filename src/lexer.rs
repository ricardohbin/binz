use crate::error::{CResult, CompileError};
use crate::types::{Kind, MapKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String),

    // keywords
    Function,
    /// `import`: only ever begins a top-level `import binz/<module>;`.
    Import,
    /// Reserved so `fn` gets a pointed diagnostic instead of "unknown name".
    Fn,
    Struct,
    Const,
    Var,
    Return,
    If,
    Else,
    While,
    Cast,
    True,
    False,
    /// `Vector` / `LinkedList` / `Set` / `SortedSet`: reserved type names, so
    /// `Vector<i32>` never has to be disambiguated from a comparison.
    Container(Kind),
    /// `HashMap` / `SortedMap`: reserved for the same reason, and separate
    /// from `Container` because maps are spelled with two type arguments.
    Map(MapKind),

    // punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semi,
    Colon,
    Dot,
    Arrow,
    Assign,
    EqEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Amp,
    AndAnd,
    OrOr,
    Bang,

    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: Tok,
    pub span: Span,
}

pub fn describe(t: &Tok) -> String {
    match t {
        Tok::Ident(n) => n.clone(),
        Tok::Int(v) => v.to_string(),
        Tok::Float(v) => v.to_string(),
        Tok::Str(_) => "string literal".to_string(),
        Tok::Function => "function".into(),
        Tok::Import => "import".into(),
        Tok::Fn => "fn".into(),
        Tok::Struct => "struct".into(),
        Tok::Const => "const".into(),
        Tok::Var => "var".into(),
        Tok::Return => "return".into(),
        Tok::If => "if".into(),
        Tok::Else => "else".into(),
        Tok::While => "while".into(),
        Tok::Cast => "cast".into(),
        Tok::True => "true".into(),
        Tok::False => "false".into(),
        Tok::Container(k) => k.name().into(),
        Tok::Map(mk) => mk.name().into(),
        Tok::LParen => "(".into(),
        Tok::RParen => ")".into(),
        Tok::LBrace => "{".into(),
        Tok::RBrace => "}".into(),
        Tok::LBracket => "[".into(),
        Tok::RBracket => "]".into(),
        Tok::Comma => ",".into(),
        Tok::Semi => ";".into(),
        Tok::Colon => ":".into(),
        Tok::Dot => ".".into(),
        Tok::Arrow => "->".into(),
        Tok::Assign => "=".into(),
        Tok::EqEq => "==".into(),
        Tok::Ne => "!=".into(),
        Tok::Lt => "<".into(),
        Tok::Le => "<=".into(),
        Tok::Gt => ">".into(),
        Tok::Ge => ">=".into(),
        Tok::Plus => "+".into(),
        Tok::Minus => "-".into(),
        Tok::Star => "*".into(),
        Tok::Slash => "/".into(),
        Tok::Percent => "%".into(),
        Tok::Amp => "&".into(),
        Tok::AndAnd => "&&".into(),
        Tok::OrOr => "||".into(),
        Tok::Bang => "!".into(),
        Tok::Eof => "end of file".into(),
    }
}

pub struct Lexer {
    src: Vec<char>,
    pos: usize,
    line: u32,
    col: u32,
}

impl Lexer {
    pub fn new(src: &str) -> Self {
        Lexer { src: src.chars().collect(), pos: 0, line: 1, col: 1 }
    }

    fn span(&self) -> Span {
        Span { line: self.line, col: self.col }
    }

    fn peek(&self) -> char {
        *self.src.get(self.pos).unwrap_or(&'\0')
    }

    fn peek2(&self) -> char {
        *self.src.get(self.pos + 1).unwrap_or(&'\0')
    }

    fn bump(&mut self) -> char {
        let c = self.peek();
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        c
    }

    fn skip_trivia(&mut self) -> CResult<()> {
        loop {
            let c = self.peek();
            if c == ' ' || c == '\t' || c == '\r' || c == '\n' {
                self.bump();
            } else if c == '/' && self.peek2() == '/' {
                while self.peek() != '\n' && self.peek() != '\0' {
                    self.bump();
                }
            } else if c == '/' && self.peek2() == '*' {
                let start = self.span();
                self.bump();
                self.bump();
                loop {
                    if self.peek() == '\0' {
                        return Err(CompileError::new("unterminated block comment", start));
                    }
                    if self.peek() == '*' && self.peek2() == '/' {
                        self.bump();
                        self.bump();
                        break;
                    }
                    self.bump();
                }
            } else {
                return Ok(());
            }
        }
    }

    pub fn tokenize(mut self) -> CResult<Vec<Token>> {
        let mut out = Vec::new();
        loop {
            self.skip_trivia()?;
            let span = self.span();
            let c = self.peek();
            if c == '\0' {
                out.push(Token { kind: Tok::Eof, span });
                return Ok(out);
            }

            let kind = if c.is_ascii_alphabetic() || c == '_' {
                let mut s = String::new();
                while self.peek().is_ascii_alphanumeric() || self.peek() == '_' {
                    s.push(self.bump());
                }
                match s.as_str() {
                    "function" => Tok::Function,
                    "import" => Tok::Import,
                    "fn" => Tok::Fn,
                    "struct" => Tok::Struct,
                    "const" => Tok::Const,
                    "var" => Tok::Var,
                    "return" => Tok::Return,
                    "if" => Tok::If,
                    "else" => Tok::Else,
                    "while" => Tok::While,
                    "cast" => Tok::Cast,
                    "true" => Tok::True,
                    "false" => Tok::False,
                    _ => match (Kind::from_name(&s), MapKind::from_name(&s)) {
                        (Some(k), _) => Tok::Container(k),
                        (_, Some(mk)) => Tok::Map(mk),
                        _ => Tok::Ident(s),
                    },
                }
            } else if c.is_ascii_digit() {
                let mut s = String::new();
                while self.peek().is_ascii_digit() {
                    s.push(self.bump());
                }
                if self.peek() == '.' && self.peek2().is_ascii_digit() {
                    s.push(self.bump());
                    while self.peek().is_ascii_digit() {
                        s.push(self.bump());
                    }
                    let v: f64 = s
                        .parse()
                        .map_err(|_| CompileError::new("invalid float literal", span))?;
                    Tok::Float(v)
                } else {
                    let v: i64 = s
                        .parse()
                        .map_err(|_| CompileError::new("integer literal out of range", span))?;
                    Tok::Int(v)
                }
            } else if c == '"' {
                self.bump();
                let mut s = String::new();
                loop {
                    let ch = self.peek();
                    if ch == '\0' || ch == '\n' {
                        return Err(CompileError::new("unterminated string literal", span));
                    }
                    self.bump();
                    if ch == '"' {
                        break;
                    }
                    if ch == '\\' {
                        let esc = self.bump();
                        s.push(match esc {
                            'n' => '\n',
                            't' => '\t',
                            'r' => '\r',
                            '0' => '\0',
                            '\\' => '\\',
                            '"' => '"',
                            other => {
                                return Err(CompileError::new(
                                    format!("unknown escape sequence `\\{}`", other),
                                    span,
                                ))
                            }
                        });
                    } else {
                        s.push(ch);
                    }
                }
                Tok::Str(s)
            } else {
                self.bump();
                match c {
                    '(' => Tok::LParen,
                    ')' => Tok::RParen,
                    '{' => Tok::LBrace,
                    '}' => Tok::RBrace,
                    '[' => Tok::LBracket,
                    ']' => Tok::RBracket,
                    ',' => Tok::Comma,
                    ';' => Tok::Semi,
                    ':' => Tok::Colon,
                    '.' => Tok::Dot,
                    '+' => Tok::Plus,
                    '*' => Tok::Star,
                    '/' => Tok::Slash,
                    '%' => Tok::Percent,
                    '-' => {
                        if self.peek() == '>' {
                            self.bump();
                            Tok::Arrow
                        } else {
                            Tok::Minus
                        }
                    }
                    '=' => {
                        if self.peek() == '=' {
                            self.bump();
                            Tok::EqEq
                        } else {
                            Tok::Assign
                        }
                    }
                    '!' => {
                        if self.peek() == '=' {
                            self.bump();
                            Tok::Ne
                        } else {
                            Tok::Bang
                        }
                    }
                    '<' => {
                        if self.peek() == '=' {
                            self.bump();
                            Tok::Le
                        } else {
                            Tok::Lt
                        }
                    }
                    '>' => {
                        if self.peek() == '=' {
                            self.bump();
                            Tok::Ge
                        } else {
                            Tok::Gt
                        }
                    }
                    '&' => {
                        if self.peek() == '&' {
                            self.bump();
                            Tok::AndAnd
                        } else {
                            Tok::Amp
                        }
                    }
                    '|' => {
                        if self.peek() == '|' {
                            self.bump();
                            Tok::OrOr
                        } else {
                            return Err(CompileError::new(
                                "unexpected `|` (binZ has no bitwise operators; use `||`)",
                                span,
                            ));
                        }
                    }
                    other => {
                        return Err(CompileError::new(
                            format!("unexpected character `{}`", other),
                            span,
                        ))
                    }
                }
            };
            out.push(Token { kind, span });
        }
    }
}
