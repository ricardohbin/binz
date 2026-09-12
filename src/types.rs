/// The four heap containers. They all share one spelling shape,
/// `Name<T>`, and one construction syntax, `Name<T>{ ... }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Vector,
    List,
    Set,
    SortedSet,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Vector => "Vector",
            Kind::List => "LinkedList",
            Kind::Set => "Set",
            Kind::SortedSet => "SortedSet",
        }
    }

    pub fn from_name(s: &str) -> Option<Kind> {
        Some(match s {
            "Vector" => Kind::Vector,
            "LinkedList" => Kind::List,
            "Set" => Kind::Set,
            "SortedSet" => Kind::SortedSet,
            _ => return None,
        })
    }

    /// Sets are keyed by value: no index assignment, no `push`.
    pub fn is_set(self) -> bool {
        matches!(self, Kind::Set | Kind::SortedSet)
    }

    /// Sequences are keyed by position: indexable and assignable.
    pub fn is_sequence(self) -> bool {
        !self.is_set()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    I32,
    I64,
    F64,
    Bool,
    Str,
    Void,
    Ptr(Box<Type>),
    Fn(Vec<Type>, Box<Type>),
    Struct(usize),
    /// `[T; N]`: fixed length, laid out flat, copied by value.
    Array(Box<Type>, u32),
    /// `Vector<T>` and friends: a handle to heap storage.
    Container(Kind, Box<Type>),
}

impl Type {
    pub fn is_numeric(&self) -> bool {
        matches!(self, Type::I32 | Type::I64 | Type::F64)
    }

    pub fn is_integer(&self) -> bool {
        matches!(self, Type::I32 | Type::I64)
    }

    /// True for types that live in memory across several slots and are
    /// therefore represented by the address of their storage.
    pub fn is_aggregate(&self) -> bool {
        matches!(self, Type::Struct(_) | Type::Array(..))
    }

    /// A value that fits in one slot, and so may be stored inside a heap
    /// container.
    pub fn is_slot(&self) -> bool {
        !self.is_aggregate() && *self != Type::Void
    }

    /// Types a `Set` can key on: everything with a total, printable identity.
    pub fn is_key(&self) -> bool {
        matches!(self, Type::I32 | Type::I64 | Type::F64 | Type::Bool | Type::Str)
    }
}

#[derive(Debug, Clone)]
pub struct FieldInfo {
    pub name: String,
    pub ty: Type,
    pub offset: u32,
}

#[derive(Debug, Clone)]
pub struct StructInfo {
    pub name: String,
    pub fields: Vec<FieldInfo>,
    pub size: u32,
    pub laid_out: bool,
}

/// Renders a type using the concrete source syntax, so diagnostics show
/// exactly what the user would have to write.
pub fn type_name(t: &Type, structs: &[StructInfo]) -> String {
    match t {
        Type::I32 => "i32".into(),
        Type::I64 => "i64".into(),
        Type::F64 => "f64".into(),
        Type::Bool => "bool".into(),
        Type::Str => "str".into(),
        Type::Void => "void".into(),
        Type::Ptr(inner) => format!("*{}", type_name(inner, structs)),
        Type::Fn(params, ret) => {
            let ps: Vec<String> = params.iter().map(|p| type_name(p, structs)).collect();
            format!("function({}): {}", ps.join(", "), type_name(ret, structs))
        }
        Type::Struct(id) => structs[*id].name.clone(),
        Type::Array(elem, n) => format!("[{}; {}]", type_name(elem, structs), n),
        Type::Container(k, elem) => format!("{}<{}>", k.name(), type_name(elem, structs)),
    }
}
