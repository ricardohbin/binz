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
}

impl Type {
    pub fn is_numeric(&self) -> bool {
        matches!(self, Type::I32 | Type::I64 | Type::F64)
    }

    pub fn is_integer(&self) -> bool {
        matches!(self, Type::I32 | Type::I64)
    }

    pub fn is_struct(&self) -> bool {
        matches!(self, Type::Struct(_))
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
    }
}
