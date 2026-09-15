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

/// The two maps. Both are spelled `Name<K, V>`, both are reached only by
/// key, and both answer `binz/map` -- they differ in one thing, the order
/// `map.keys` hands back, exactly as `Set` and `SortedSet` do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapKind {
    /// `HashMap<K, V>`: insertion order.
    Hash,
    /// `SortedMap<K, V>`: ascending key order, held in a red-black tree.
    Sorted,
}

impl MapKind {
    pub fn name(self) -> &'static str {
        match self {
            MapKind::Hash => "HashMap",
            MapKind::Sorted => "SortedMap",
        }
    }

    pub fn from_name(s: &str) -> Option<MapKind> {
        Some(match s {
            "HashMap" => MapKind::Hash,
            "SortedMap" => MapKind::Sorted,
            _ => return None,
        })
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
    /// `HashMap<K, V>` and `SortedMap<K, V>`: a handle to heap storage
    /// keyed by value. Maps are their own variant rather than a `Kind`
    /// because they are the only types with two type arguments.
    Map(MapKind, Box<Type>, Box<Type>),
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

    /// Types a set or a map can key on: everything with a total, printable
    /// identity.
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
        Type::Map(mk, k, v) => {
            format!("{}<{}, {}>", mk.name(), type_name(k, structs), type_name(v, structs))
        }
    }
}
