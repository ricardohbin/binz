//! The binZ standard library: `binz/io`, `binz/string`, `binz/int`,
//! `binz/float` and `binz/container`.
//!
//! A module is made visible with `import binz/<name>;` and every one of its
//! members is then reached as `<name>.<member>`. That is the only spelling:
//! no bare import, no alias, no wildcard, and no implicitly-visible name.
//! The last path segment is always the binding, so a reader never has to
//! look up where `print` came from.
//!
//! Each module splits into two tables, for one reason only:
//!
//!   * `NATIVES` are monomorphic. Their type can be written down in binZ, so
//!     they are ordinary first-class values -- `const say: function(str):
//!     void = io.print;` works.
//!   * `FORMS` are generic over a type binZ cannot yet write in a signature
//!     (`container.size` over five containers, `int.max` over `i32` and
//!     `i64`). The compiler resolves them at the call site, and they are not
//!     values.
//!
//! So the rule is: **a stdlib function is a value exactly when its type is
//! writable in binZ.** Nothing else distinguishes the two tables.
//!
//! Naming: a module is lowercase, a member is `camelCase` (`startsWith`,
//! `canParse`), and a type is `PascalCase` (`Vector`, `HashMap`). The
//! `members_are_named_in_camel_case` test below holds the line, since a
//! single `snake_case` slip would leave two conventions in one library.

use crate::types::{Kind, Type};

/// Every module, in the order they are documented. The `binz/` prefix is
/// part of the import path and is not repeated here.
pub const MODULES: &[&str] = &["io", "string", "int", "float", "container"];

pub type NativeSig = fn() -> Type;

pub struct Native {
    pub module: &'static str,
    pub name: &'static str,
    pub sig: NativeSig,
}

pub struct Form {
    pub module: &'static str,
    pub name: &'static str,
    pub id: u8,
    pub arity: usize,
}

macro_rules! sig {
    ($($p:expr),* => $r:expr) => { || Type::Fn(vec![$($p),*], Box::new($r)) };
}

fn vec_of_str() -> Type {
    Type::Container(Kind::Vector, Box::new(Type::Str))
}

/// Monomorphic stdlib entries. **The index is the bytecode operand**, so
/// entries are only ever appended.
pub const NATIVES: &[Native] = &[
    // ---------------------------------------------------------- binz/io
    Native { module: "io", name: "print", sig: sig!(Type::Str => Type::Void) },
    // ------------------------------------------------------ binz/string
    // Every position here is a character index, matching `string.size`.
    Native { module: "string", name: "size", sig: sig!(Type::Str => Type::I32) },
    Native { module: "string", name: "at", sig: sig!(Type::Str, Type::I32 => Type::Str) },
    Native { module: "string", name: "slice", sig: sig!(Type::Str, Type::I32, Type::I32 => Type::Str) },
    Native { module: "string", name: "find", sig: sig!(Type::Str, Type::Str => Type::I32) },
    Native { module: "string", name: "contains", sig: sig!(Type::Str, Type::Str => Type::Bool) },
    Native { module: "string", name: "startsWith", sig: sig!(Type::Str, Type::Str => Type::Bool) },
    Native { module: "string", name: "endsWith", sig: sig!(Type::Str, Type::Str => Type::Bool) },
    Native { module: "string", name: "upper", sig: sig!(Type::Str => Type::Str) },
    Native { module: "string", name: "lower", sig: sig!(Type::Str => Type::Str) },
    Native { module: "string", name: "trim", sig: sig!(Type::Str => Type::Str) },
    Native { module: "string", name: "repeat", sig: sig!(Type::Str, Type::I32 => Type::Str) },
    Native { module: "string", name: "replace", sig: sig!(Type::Str, Type::Str, Type::Str => Type::Str) },
    Native { module: "string", name: "split", sig: sig!(Type::Str, Type::Str => vec_of_str()) },
    Native { module: "string", name: "join", sig: sig!(vec_of_str(), Type::Str => Type::Str) },
    // --------------------------------------------------------- binz/int
    // `parse` answers in the widest integer; `cast<i32>` narrows, because
    // `cast` is already the one converter.
    Native { module: "int", name: "parse", sig: sig!(Type::Str => Type::I64) },
    Native { module: "int", name: "canParse", sig: sig!(Type::Str => Type::Bool) },
    // ------------------------------------------------------- binz/float
    Native { module: "float", name: "parse", sig: sig!(Type::Str => Type::F64) },
    Native { module: "float", name: "canParse", sig: sig!(Type::Str => Type::Bool) },
    Native { module: "float", name: "abs", sig: sig!(Type::F64 => Type::F64) },
    Native { module: "float", name: "min", sig: sig!(Type::F64, Type::F64 => Type::F64) },
    Native { module: "float", name: "max", sig: sig!(Type::F64, Type::F64 => Type::F64) },
    Native { module: "float", name: "floor", sig: sig!(Type::F64 => Type::F64) },
    Native { module: "float", name: "ceil", sig: sig!(Type::F64 => Type::F64) },
    Native { module: "float", name: "round", sig: sig!(Type::F64 => Type::F64) },
    Native { module: "float", name: "sqrt", sig: sig!(Type::F64 => Type::F64) },
    Native { module: "float", name: "pow", sig: sig!(Type::F64, Type::F64 => Type::F64) },
    Native { module: "float", name: "isNan", sig: sig!(Type::F64 => Type::Bool) },
];

use crate::bytecode::*;

/// Generic stdlib entries. The `id` is the bytecode operand of `OP_BUILTIN`.
pub const FORMS: &[Form] = &[
    // --------------------------------------------------- binz/container
    Form { module: "container", name: "size", id: B_SIZE, arity: 1 },
    Form { module: "container", name: "find", id: B_FIND, arity: 2 },
    Form { module: "container", name: "push", id: B_PUSH, arity: 2 },
    Form { module: "container", name: "pop", id: B_POP, arity: 1 },
    Form { module: "container", name: "insert", id: B_INSERT, arity: 3 },
    Form { module: "container", name: "erase", id: B_ERASE, arity: 2 },
    Form { module: "container", name: "add", id: B_ADD, arity: 2 },
    Form { module: "container", name: "remove", id: B_REMOVE, arity: 2 },
    Form { module: "container", name: "contains", id: B_CONTAINS, arity: 2 },
    Form { module: "container", name: "clear", id: B_CLEAR, arity: 1 },
    Form { module: "container", name: "copy", id: B_COPY, arity: 1 },
    Form { module: "container", name: "keys", id: B_KEYS, arity: 1 },
    // --------------------------------------------------------- binz/int
    // Generic over `i32` and `i64`: the result is the argument's own type,
    // so no `cast` is forced on either width.
    Form { module: "int", name: "abs", id: B_INT_ABS, arity: 1 },
    Form { module: "int", name: "min", id: B_INT_MIN, arity: 2 },
    Form { module: "int", name: "max", id: B_INT_MAX, arity: 2 },
];

pub fn is_module(name: &str) -> bool {
    MODULES.contains(&name)
}

pub fn find_native(module: &str, name: &str) -> Option<(u32, Type)> {
    NATIVES
        .iter()
        .position(|n| n.module == module && n.name == name)
        .map(|i| (i as u32, (NATIVES[i].sig)()))
}

pub fn find_form(module: &str, name: &str) -> Option<&'static Form> {
    FORMS.iter().find(|f| f.module == module && f.name == name)
}

pub fn native_name(idx: u32) -> String {
    let n = &NATIVES[idx as usize];
    format!("{}.{}", n.module, n.name)
}

pub fn form_name(id: u8) -> &'static str {
    FORMS.iter().find(|f| f.id == id).map(|f| f.name).unwrap_or("?")
}

/// Every module that defines `name`, so a bare `print(...)` or `len(...)`
/// can be answered with the import the program is missing.
pub fn modules_defining(name: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for m in MODULES {
        let here = NATIVES.iter().any(|n| n.module == *m && n.name == name)
            || FORMS.iter().any(|f| f.module == *m && f.name == name);
        if here {
            out.push(m);
        }
    }
    out
}

/// Names that used to be visible with no module at all, and what they are
/// spelled as now. They exist only to turn "`len` is not defined" into the
/// line the program should have written.
pub const RENAMED: &[(&str, &str)] = &[("len", "size")];

pub fn renamed_to(name: &str) -> Option<&'static str> {
    RENAMED.iter().find(|r| r.0 == name).map(|r| r.1)
}

/// "`binz/string`" / "`binz/string` or `binz/container`", for diagnostics.
pub fn describe_modules(mods: &[&str]) -> String {
    let paths: Vec<String> = mods.iter().map(|m| format!("`binz/{}`", m)).collect();
    match paths.len() {
        0 => String::new(),
        1 => paths[0].clone(),
        _ => format!("{} or {}", paths[..paths.len() - 1].join(", "), paths[paths.len() - 1]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The index of a native is its bytecode operand and its arm in
    /// `Vm::call_native`, so this order is part of the artifact format.
    /// Append only.
    #[test]
    fn native_indices_are_frozen() {
        let expected = [
            "io.print",
            "string.size",
            "string.at",
            "string.slice",
            "string.find",
            "string.contains",
            "string.startsWith",
            "string.endsWith",
            "string.upper",
            "string.lower",
            "string.trim",
            "string.repeat",
            "string.replace",
            "string.split",
            "string.join",
            "int.parse",
            "int.canParse",
            "float.parse",
            "float.canParse",
            "float.abs",
            "float.min",
            "float.max",
            "float.floor",
            "float.ceil",
            "float.round",
            "float.sqrt",
            "float.pow",
            "float.isNan",
        ];
        let actual: Vec<String> = (0..NATIVES.len() as u32).map(native_name).collect();
        assert_eq!(actual, expected);
    }

    /// A member is `camelCase`, always. This is a test rather than a note
    /// because the table is the only place the convention lives.
    #[test]
    fn members_are_named_in_camel_case() {
        for (m, n) in NATIVES
            .iter()
            .map(|n| (n.module, n.name))
            .chain(FORMS.iter().map(|f| (f.module, f.name)))
        {
            assert!(!n.contains('_'), "`{}.{}` is snake_case; binZ members are camelCase", m, n);
            assert!(
                n.starts_with(|c: char| c.is_ascii_lowercase()),
                "`{}.{}` must start lowercase",
                m,
                n
            );
        }
        // ... and so is a module name, which is one word and all lowercase.
        for m in MODULES {
            assert!(m.chars().all(|c| c.is_ascii_lowercase()), "module `{}` is not lowercase", m);
        }
    }

    /// Every member belongs to a declared module, and no module declares the
    /// same name twice -- one spelling, one meaning.
    #[test]
    fn members_are_unique_within_a_module() {
        let mut seen: Vec<(&str, &str)> = Vec::new();
        for (m, n) in NATIVES
            .iter()
            .map(|n| (n.module, n.name))
            .chain(FORMS.iter().map(|f| (f.module, f.name)))
        {
            assert!(is_module(m), "`{}` is not a declared module", m);
            assert!(!seen.contains(&(m, n)), "`{}.{}` is declared twice", m, n);
            seen.push((m, n));
        }
    }
}
