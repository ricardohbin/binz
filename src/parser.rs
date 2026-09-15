use crate::ast::*;
use crate::error::{CResult, CompileError};
use crate::lexer::{describe, Span, Tok, Token};

pub struct Parser {
    toks: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(toks: Vec<Token>) -> Self {
        Parser { toks, pos: 0 }
    }

    fn peek(&self) -> &Tok {
        &self.toks[self.pos].kind
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, k: &Tok) -> bool {
        if self.peek() == k {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, k: Tok) -> CResult<Token> {
        if self.peek() == &k {
            Ok(self.bump())
        } else {
            Err(CompileError::new(
                format!("expected `{}`, found `{}`", describe(&k), describe(self.peek())),
                self.span(),
            ))
        }
    }

    fn ident(&mut self) -> CResult<(String, Span)> {
        let sp = self.span();
        match self.peek().clone() {
            Tok::Ident(n) => {
                self.bump();
                Ok((n, sp))
            }
            other => Err(CompileError::new(
                format!("expected identifier, found `{}`", describe(&other)),
                sp,
            )),
        }
    }

    // ---------------------------------------------------------------- items

    pub fn parse_program(&mut self) -> CResult<Vec<Item>> {
        let mut items = Vec::new();
        // Imports come first, all of them, so the head of a file always says
        // exactly what it depends on.
        while self.peek() == &Tok::Import {
            items.push(Item::Import(self.parse_import()?));
        }
        while self.peek() != &Tok::Eof {
            match self.peek() {
                Tok::Struct => items.push(Item::Struct(self.parse_struct()?)),
                Tok::Function => items.push(Item::Fn(self.parse_fn()?)),
                Tok::Import => {
                    return Err(CompileError::new(
                        "every `import` goes at the top of the file, before the first `struct` or `function`",
                        self.span(),
                    ))
                }
                Tok::Fn => {
                    return Err(CompileError::new(
                        "binZ declares functions with `function`, not `fn`",
                        self.span(),
                    ))
                }
                other => {
                    return Err(CompileError::new(
                        format!(
                            "expected `function` or `struct` at top level, found `{}`",
                            describe(other)
                        ),
                        self.span(),
                    ))
                }
            }
        }
        Ok(items)
    }

    /// `import binz/io;` or `import @root/utils/math.binz;` -- one module per
    /// statement, bound to the last segment of its path and to nothing else.
    fn parse_import(&mut self) -> CResult<ImportDef> {
        let span = self.span();
        self.expect(Tok::Import)?;
        let (local, path) = if self.eat(&Tok::At) {
            (true, self.parse_root_path()?)
        } else {
            let mut path = vec![self.ident()?.0];
            while self.eat(&Tok::Slash) {
                path.push(self.ident()?.0);
            }
            (false, path)
        };
        let alias = if self.eat(&Tok::As) {
            let (name, asp) = self.ident()?;
            check_module_name(&name, asp, "a renamed module")?;
            Some(Alias { name, span: asp })
        } else {
            None
        };
        if self.peek() == &Tok::Comma {
            return Err(CompileError::new(
                "one module per `import`; write a second `import` line instead",
                self.span(),
            ));
        }
        self.expect(Tok::Semi)?;
        Ok(ImportDef { local, path, alias, span })
    }

    /// The `root/utils/math.binz` of `import @root/utils/math.binz;`, returned
    /// as the segments below the root with the extension stripped -- so the
    /// last one is the file's own name, which is the binding.
    fn parse_root_path(&mut self) -> CResult<Vec<String>> {
        let (anchor, asp) = self.ident()?;
        if anchor != "root" {
            return Err(CompileError::new(
                format!(
                    "`@{}` is not an anchor; every local import starts at `@root`, \
                     the directory of the file passed to `binz`",
                    anchor
                ),
                asp,
            ));
        }
        let mut path = Vec::new();
        while self.eat(&Tok::Slash) {
            let (seg, ssp) = self.ident()?;
            check_path_segment(&seg, ssp)?;
            path.push(seg);
        }
        if path.is_empty() {
            return Err(CompileError::new(
                "`@root` is a directory; name a file in it, as in `@root/math.binz`",
                asp,
            ));
        }
        // A file name is one lowercase word because it *is* the binding, so
        // `math-utils.binz` stops here rather than at an unreadable `math.add`.
        if self.peek() == &Tok::Minus {
            return Err(CompileError::new(
                "`-` cannot appear in a module file name: the name is the binding, \
                 the `math` you would write in `math.square`, so it is one lowercase word",
                self.span(),
            ));
        }
        if self.peek() == &Tok::Semi {
            return Err(CompileError::new(
                format!(
                    "a local import names the file, extension and all; write `@root/{}.binz`",
                    path.join("/")
                ),
                self.span(),
            ));
        }
        self.expect(Tok::Dot)?;
        let (ext, esp) = self.ident()?;
        if ext != "binz" {
            return Err(CompileError::new(
                format!("`.{}` is not a binZ source file; a local import ends in `.binz`", ext),
                esp,
            ));
        }
        Ok(path)
    }

    fn parse_struct(&mut self) -> CResult<StructDef> {
        let span = self.span();
        self.expect(Tok::Struct)?;
        let (name, _) = self.ident()?;
        self.expect(Tok::LBrace)?;
        let mut fields = Vec::new();
        while self.peek() != &Tok::RBrace {
            let (fname, fspan) = self.ident()?;
            self.expect(Tok::Colon)?;
            let ty = self.parse_type()?;
            fields.push(Param { name: fname, ty, span: fspan });
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(Tok::RBrace)?;
        Ok(StructDef { name, fields, span })
    }

    fn parse_fn(&mut self) -> CResult<FnDef> {
        let span = self.span();
        self.expect(Tok::Function)?;
        let (name, _) = self.ident()?;
        self.expect(Tok::LParen)?;
        let mut params = Vec::new();
        while self.peek() != &Tok::RParen {
            let (pname, pspan) = self.ident()?;
            self.expect(Tok::Colon)?;
            let ty = self.parse_type()?;
            params.push(Param { name: pname, ty, span: pspan });
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(Tok::RParen)?;
        self.expect_return_colon()?;
        let ret = self.parse_type()?;
        let body = self.parse_block()?;
        Ok(FnDef { name, params, ret, body, span })
    }

    /// A return type is introduced by `:`, exactly like every other type
    /// annotation in the language.
    fn expect_return_colon(&mut self) -> CResult<()> {
        if self.peek() == &Tok::Arrow {
            return Err(CompileError::new(
                "binZ writes the return type after `:`, not `->`",
                self.span(),
            ));
        }
        self.expect(Tok::Colon)?;
        Ok(())
    }

    /// A length is always a plain positive integer literal: binZ has no
    /// constant folding, so there is exactly one thing that can appear here.
    fn parse_length(&mut self) -> CResult<u32> {
        let sp = self.span();
        match self.peek().clone() {
            Tok::Int(v) => {
                self.bump();
                if v <= 0 {
                    return Err(CompileError::new("a length must be at least 1", sp));
                }
                if v > u32::MAX as i64 {
                    return Err(CompileError::new("length is too large", sp));
                }
                Ok(v as u32)
            }
            other => Err(CompileError::new(
                format!("expected an integer length, found `{}`", describe(&other)),
                sp,
            )),
        }
    }

    /// `<K, V>`, the one place in binZ where a type takes two arguments.
    fn parse_map_args(&mut self) -> CResult<(TypeExpr, TypeExpr)> {
        self.expect(Tok::Lt)?;
        let k = self.parse_type()?;
        self.expect(Tok::Comma)?;
        let v = self.parse_type()?;
        self.expect(Tok::Gt)?;
        Ok((k, v))
    }

    fn parse_type(&mut self) -> CResult<TypeExpr> {
        let sp = self.span();
        match self.peek().clone() {
            Tok::LBracket => {
                self.bump();
                let elem = self.parse_type()?;
                self.expect(Tok::Semi)?;
                let n = self.parse_length()?;
                self.expect(Tok::RBracket)?;
                Ok(TypeExpr::Array(Box::new(elem), n, sp))
            }
            Tok::Container(k) => {
                self.bump();
                self.expect(Tok::Lt)?;
                let elem = self.parse_type()?;
                self.expect(Tok::Gt)?;
                Ok(TypeExpr::Container(k, Box::new(elem), sp))
            }
            Tok::Map(mk) => {
                self.bump();
                let (k, v) = self.parse_map_args()?;
                Ok(TypeExpr::Map(mk, Box::new(k), Box::new(v), sp))
            }
            Tok::Star => {
                self.bump();
                let inner = self.parse_type()?;
                Ok(TypeExpr::Ptr(Box::new(inner), sp))
            }
            Tok::Fn => {
                return Err(CompileError::new(
                    "binZ spells the function type `function(...): T`, not `fn`",
                    sp,
                ))
            }
            Tok::Function => {
                self.bump();
                self.expect(Tok::LParen)?;
                let mut params = Vec::new();
                while self.peek() != &Tok::RParen {
                    params.push(self.parse_type()?);
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                self.expect(Tok::RParen)?;
                self.expect_return_colon()?;
                let ret = self.parse_type()?;
                Ok(TypeExpr::Fn(params, Box::new(ret), sp))
            }
            Tok::Ident(n) => {
                self.bump();
                Ok(TypeExpr::Name(n, sp))
            }
            other => Err(CompileError::new(
                format!("expected a type, found `{}`", describe(&other)),
                sp,
            )),
        }
    }

    // ----------------------------------------------------------- statements

    fn parse_block(&mut self) -> CResult<Block> {
        let span = self.span();
        self.expect(Tok::LBrace)?;
        let mut stmts = Vec::new();
        while self.peek() != &Tok::RBrace {
            if self.peek() == &Tok::Eof {
                return Err(CompileError::new("unexpected end of file, missing `}`", self.span()));
            }
            stmts.push(self.parse_stmt()?);
        }
        self.expect(Tok::RBrace)?;
        Ok(Block { stmts, span })
    }

    fn parse_stmt(&mut self) -> CResult<Stmt> {
        let span = self.span();
        match self.peek().clone() {
            Tok::Const | Tok::Var => {
                let mutable = self.peek() == &Tok::Var;
                self.bump();
                let (name, _) = self.ident()?;
                self.expect(Tok::Colon)?;
                let ty = self.parse_type()?;
                self.expect(Tok::Assign)?;
                let init = self.parse_expr()?;
                self.expect(Tok::Semi)?;
                Ok(Stmt::Let { mutable, name, ty, init, span })
            }
            Tok::Return => {
                self.bump();
                if self.eat(&Tok::Semi) {
                    Ok(Stmt::Return(None, span))
                } else {
                    let e = self.parse_expr()?;
                    self.expect(Tok::Semi)?;
                    Ok(Stmt::Return(Some(e), span))
                }
            }
            Tok::If => self.parse_if(),
            Tok::While => {
                self.bump();
                self.expect(Tok::LParen)?;
                let cond = self.parse_expr()?;
                self.expect(Tok::RParen)?;
                let body = self.parse_block()?;
                Ok(Stmt::While { cond, body, span })
            }
            Tok::LBrace => {
                let b = self.parse_block()?;
                Ok(Stmt::Nested(b, span))
            }
            _ => {
                let e = self.parse_expr()?;
                if self.eat(&Tok::Assign) {
                    let value = self.parse_expr()?;
                    self.expect(Tok::Semi)?;
                    Ok(Stmt::Assign { target: e, value, span })
                } else {
                    self.expect(Tok::Semi)?;
                    Ok(Stmt::ExprStmt(e, span))
                }
            }
        }
    }

    fn parse_if(&mut self) -> CResult<Stmt> {
        let span = self.span();
        self.expect(Tok::If)?;
        self.expect(Tok::LParen)?;
        let cond = self.parse_expr()?;
        self.expect(Tok::RParen)?;
        let then = self.parse_block()?;
        let els = if self.eat(&Tok::Else) {
            if self.peek() == &Tok::If {
                let inner = self.parse_if()?;
                let sp = inner.span_of();
                Some(Block { stmts: vec![inner], span: sp })
            } else {
                Some(self.parse_block()?)
            }
        } else {
            None
        };
        Ok(Stmt::If { cond, then, els, span })
    }

    // ---------------------------------------------------------- expressions

    fn parse_expr(&mut self) -> CResult<Expr> {
        self.parse_bin(1)
    }

    fn prec(t: &Tok) -> Option<(u8, BinOp)> {
        Some(match t {
            Tok::OrOr => (1, BinOp::Or),
            Tok::AndAnd => (2, BinOp::And),
            Tok::EqEq => (3, BinOp::Eq),
            Tok::Ne => (3, BinOp::Ne),
            Tok::Lt => (4, BinOp::Lt),
            Tok::Le => (4, BinOp::Le),
            Tok::Gt => (4, BinOp::Gt),
            Tok::Ge => (4, BinOp::Ge),
            Tok::Plus => (5, BinOp::Add),
            Tok::Minus => (5, BinOp::Sub),
            Tok::Star => (6, BinOp::Mul),
            Tok::Slash => (6, BinOp::Div),
            Tok::Percent => (6, BinOp::Rem),
            _ => return None,
        })
    }

    fn parse_bin(&mut self, min_prec: u8) -> CResult<Expr> {
        let mut lhs = self.parse_unary()?;
        loop {
            let (p, op) = match Self::prec(self.peek()) {
                Some(x) => x,
                None => break,
            };
            if p < min_prec {
                break;
            }
            let span = self.span();
            self.bump();
            let rhs = self.parse_bin(p + 1)?;
            lhs = Expr::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> CResult<Expr> {
        let span = self.span();
        let op = match self.peek() {
            Tok::Minus => Some(UnOp::Neg),
            Tok::Bang => Some(UnOp::Not),
            Tok::Star => Some(UnOp::Deref),
            Tok::Amp => Some(UnOp::AddrOf),
            _ => None,
        };
        if let Some(op) = op {
            self.bump();
            let expr = self.parse_unary()?;
            return Ok(Expr::Unary { op, expr: Box::new(expr), span });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> CResult<Expr> {
        let mut e = self.parse_primary()?;
        loop {
            let span = self.span();
            if self.eat(&Tok::LParen) {
                let mut args = Vec::new();
                while self.peek() != &Tok::RParen {
                    args.push(self.parse_expr()?);
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                self.expect(Tok::RParen)?;
                e = Expr::Call { callee: Box::new(e), args, span };
            } else if self.eat(&Tok::LBracket) {
                let index = self.parse_expr()?;
                self.expect(Tok::RBracket)?;
                e = Expr::Index { base: Box::new(e), index: Box::new(index), span };
            } else if self.eat(&Tok::Dot) {
                let (name, _) = self.ident()?;
                e = Expr::Field { base: Box::new(e), name, span };
            } else {
                break;
            }
        }
        Ok(e)
    }

    fn parse_primary(&mut self) -> CResult<Expr> {
        let span = self.span();
        match self.peek().clone() {
            Tok::Int(v) => {
                self.bump();
                Ok(Expr::Int(v, span))
            }
            Tok::Float(v) => {
                self.bump();
                Ok(Expr::Float(v, span))
            }
            Tok::Str(s) => {
                self.bump();
                Ok(Expr::Str(s, span))
            }
            Tok::True => {
                self.bump();
                Ok(Expr::Bool(true, span))
            }
            Tok::False => {
                self.bump();
                Ok(Expr::Bool(false, span))
            }
            Tok::LParen => {
                self.bump();
                let e = self.parse_expr()?;
                self.expect(Tok::RParen)?;
                Ok(e)
            }
            Tok::LBracket => {
                self.bump();
                if self.peek() == &Tok::RBracket {
                    return Err(CompileError::new(
                        "an array needs at least one element; use `Vector<T>{}` for an empty container",
                        span,
                    ));
                }
                let first = self.parse_expr()?;
                if self.eat(&Tok::Semi) {
                    let count = self.parse_length()?;
                    self.expect(Tok::RBracket)?;
                    return Ok(Expr::ArrayRepeat { value: Box::new(first), count, span });
                }
                let mut elems = vec![first];
                while self.eat(&Tok::Comma) {
                    if self.peek() == &Tok::RBracket {
                        break;
                    }
                    elems.push(self.parse_expr()?);
                }
                self.expect(Tok::RBracket)?;
                Ok(Expr::ArrayLit { elems, span })
            }
            Tok::Container(kind) => {
                self.bump();
                self.expect(Tok::Lt)?;
                let elem = self.parse_type()?;
                self.expect(Tok::Gt)?;
                self.expect(Tok::LBrace)?;
                let mut elems = Vec::new();
                while self.peek() != &Tok::RBrace {
                    elems.push(self.parse_expr()?);
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                self.expect(Tok::RBrace)?;
                Ok(Expr::ContainerLit { kind, elem, elems, span })
            }
            Tok::Map(kind) => {
                self.bump();
                let (key, val) = self.parse_map_args()?;
                self.expect(Tok::LBrace)?;
                let mut entries = Vec::new();
                while self.peek() != &Tok::RBrace {
                    let k = self.parse_expr()?;
                    self.expect(Tok::Colon)?;
                    let v = self.parse_expr()?;
                    entries.push((k, v));
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                self.expect(Tok::RBrace)?;
                Ok(Expr::MapLit { kind, key, val, entries, span })
            }
            Tok::Cast => {
                self.bump();
                self.expect(Tok::Lt)?;
                let ty = self.parse_type()?;
                self.expect(Tok::Gt)?;
                self.expect(Tok::LParen)?;
                let e = self.parse_expr()?;
                self.expect(Tok::RParen)?;
                Ok(Expr::Cast { ty, expr: Box::new(e), span })
            }
            Tok::Ident(name) => {
                self.bump();
                if self.peek() == &Tok::LBrace {
                    self.bump();
                    let mut fields = Vec::new();
                    while self.peek() != &Tok::RBrace {
                        let (fname, fspan) = self.ident()?;
                        self.expect(Tok::Colon)?;
                        let val = self.parse_expr()?;
                        fields.push((fname, val, fspan));
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    self.expect(Tok::RBrace)?;
                    Ok(Expr::StructLit { name, fields, span })
                } else {
                    Ok(Expr::Ident(name, span))
                }
            }
            other => Err(CompileError::new(
                format!("expected an expression, found `{}`", describe(&other)),
                span,
            )),
        }
    }
}

impl Stmt {
    pub fn span_of(&self) -> Span {
        match self {
            Stmt::Let { span, .. }
            | Stmt::Assign { span, .. }
            | Stmt::ExprStmt(_, span)
            | Stmt::Return(_, span)
            | Stmt::If { span, .. }
            | Stmt::While { span, .. }
            | Stmt::Nested(_, span) => *span,
        }
    }
}

/// A directory or file name inside `@root/...` is lowercase, like a module
/// name -- the file's own name becomes the binding, and binZ spells a module
/// in lowercase. One convention, checked where the path is written.
fn check_path_segment(seg: &str, span: Span) -> CResult<()> {
    check_module_name(
        seg,
        span,
        "a directory or file under `@root`",
    )
}

/// One lowercase word, which is how binZ spells every module name -- so an
/// `as` rename reads exactly like the name it replaces.
fn check_module_name(name: &str, span: Span, what: &str) -> CResult<()> {
    let ok = name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    if !ok {
        return Err(CompileError::new(
            format!("`{}` is not a module name; {} is lowercase, like every module name", name, what),
            span,
        ));
    }
    Ok(())
}
