use crate::lexer::Span;
use crate::types::{Kind, MapKind};

#[derive(Debug, Clone)]
pub enum TypeExpr {
    Name(String, Span),
    Ptr(Box<TypeExpr>, Span),
    Fn(Vec<TypeExpr>, Box<TypeExpr>, Span),
    /// `[T; N]`
    Array(Box<TypeExpr>, u32, Span),
    /// `Vector<T>`, `LinkedList<T>`, `Set<T>`, `SortedSet<T>`
    Container(Kind, Box<TypeExpr>, Span),
    /// `HashMap<K, V>` / `SortedMap<K, V>`
    Map(MapKind, Box<TypeExpr>, Box<TypeExpr>, Span),
}

impl TypeExpr {
    pub fn span(&self) -> Span {
        match self {
            TypeExpr::Name(_, s)
            | TypeExpr::Ptr(_, s)
            | TypeExpr::Fn(_, _, s)
            | TypeExpr::Array(_, _, s)
            | TypeExpr::Container(_, _, s)
            | TypeExpr::Map(_, _, _, s) => *s,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: TypeExpr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<Param>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FnDef {
    pub name: String,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
    pub body: Block,
    pub span: Span,
}

/// `import binz/io;` or `import @root/utils/math.binz;`. The path is stored
/// segment by segment; the last one is the name the module is bound to,
/// always, with no way to rename it.
#[derive(Debug, Clone)]
pub struct ImportDef {
    /// True for `@root/...`, a file of this project. A local path holds only
    /// the segments below the root, without the `.binz` extension, so the
    /// last one is the file's own name and therefore the binding.
    pub local: bool,
    pub path: Vec<String>,
    /// `as othername`. Legal only when this file imports two or more modules
    /// that are named the same -- and then required on every one of them, so
    /// a name is never the default for one import and a rename for another.
    pub alias: Option<Alias>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Alias {
    pub name: String,
    pub span: Span,
}

impl ImportDef {
    /// The module's own name: the `io` of `binz/io`, the `math` of
    /// `@root/utils/math.binz`.
    pub fn own_name(&self) -> &str {
        self.path.last().map(|s| s.as_str()).unwrap_or("")
    }

    /// The binding, i.e. the `io` in `io.print` or the `math` in `math.add`.
    pub fn binding(&self) -> &str {
        match &self.alias {
            Some(a) => &a.name,
            None => self.own_name(),
        }
    }

    /// The import exactly as it is written in source, for diagnostics.
    pub fn text(&self) -> String {
        if self.local {
            format!("@root/{}.binz", self.path.join("/"))
        } else {
            self.path.join("/")
        }
    }
}

#[derive(Debug, Clone)]
pub enum Item {
    Import(ImportDef),
    Struct(StructDef),
    Fn(FnDef),
}

#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Let {
        mutable: bool,
        name: String,
        ty: TypeExpr,
        init: Expr,
        span: Span,
    },
    Assign {
        target: Expr,
        value: Expr,
        span: Span,
    },
    ExprStmt(Expr, Span),
    Return(Option<Expr>, Span),
    If {
        cond: Expr,
        then: Block,
        els: Option<Block>,
        span: Span,
    },
    While {
        cond: Expr,
        body: Block,
        span: Span,
    },
    Nested(Block, Span),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
    Deref,
    AddrOf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "&&",
            BinOp::Or => "||",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Expr {
    Int(i64, Span),
    Float(f64, Span),
    Str(String, Span),
    Bool(bool, Span),
    Ident(String, Span),
    Unary {
        op: UnOp,
        expr: Box<Expr>,
        span: Span,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
    Field {
        base: Box<Expr>,
        name: String,
        span: Span,
    },
    StructLit {
        name: String,
        fields: Vec<(String, Expr, Span)>,
        span: Span,
    },
    Cast {
        ty: TypeExpr,
        expr: Box<Expr>,
        span: Span,
    },
    /// `c[i]`, the one way to reach an element of any container.
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    /// `[a, b, c]`
    ArrayLit {
        elems: Vec<Expr>,
        span: Span,
    },
    /// `[x; N]`
    ArrayRepeat {
        value: Box<Expr>,
        count: u32,
        span: Span,
    },
    /// `Vector<i32>{ 1, 2, 3 }`
    ContainerLit {
        kind: Kind,
        elem: TypeExpr,
        elems: Vec<Expr>,
        span: Span,
    },
    /// `HashMap<str, i32>{ "a": 1, "b": 2 }`, and the same for `SortedMap`.
    MapLit {
        kind: MapKind,
        key: TypeExpr,
        val: TypeExpr,
        entries: Vec<(Expr, Expr)>,
        span: Span,
    },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Int(_, s)
            | Expr::Float(_, s)
            | Expr::Str(_, s)
            | Expr::Bool(_, s)
            | Expr::Ident(_, s)
            | Expr::Unary { span: s, .. }
            | Expr::Binary { span: s, .. }
            | Expr::Call { span: s, .. }
            | Expr::Field { span: s, .. }
            | Expr::StructLit { span: s, .. }
            | Expr::Cast { span: s, .. }
            | Expr::Index { span: s, .. }
            | Expr::ArrayLit { span: s, .. }
            | Expr::ArrayRepeat { span: s, .. }
            | Expr::ContainerLit { span: s, .. }
            | Expr::MapLit { span: s, .. } => *s,
        }
    }

    /// Literals carry no side effects, so the compiler may reorder them to
    /// recover the type of the other operand first.
    pub fn is_literal(&self) -> bool {
        match self {
            Expr::Int(..) | Expr::Float(..) => true,
            Expr::Unary { op: UnOp::Neg, expr, .. } => expr.is_literal(),
            _ => false,
        }
    }
}
