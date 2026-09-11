//! Single pass over the AST that type checks and emits bytecode at the same
//! time. binZ has no type inference beyond literal typing, so one pass with a
//! type hint threaded downwards is enough.

use std::collections::HashMap;

use crate::ast::*;
use crate::bytecode::*;
use crate::error::{CResult, CompileError};
use crate::lexer::Span;
use crate::types::*;
use crate::vm::NATIVES;

/// Upper bound on the slots one value may occupy. Fixed arrays are frame
/// storage, so this is what keeps a type annotation from asking for a
/// gigabyte of frame.
const MAX_SLOTS: u64 = 4_000_000;

#[derive(Debug, Clone)]
struct Local {
    name: String,
    ty: Type,
    slot: u32,
    mutable: bool,
}

#[derive(Debug, Clone)]
struct FnSig {
    name: String,
    params: Vec<Type>,
    ret: Type,
    span: Span,
}

pub struct Compiler {
    structs: Vec<StructInfo>,
    struct_ids: HashMap<String, usize>,
    sigs: Vec<FnSig>,
    fn_ids: HashMap<String, usize>,
    strings: Vec<String>,
    string_ids: HashMap<String, u32>,

    // state of the function currently being compiled
    code: Vec<u8>,
    scopes: Vec<Vec<Local>>,
    next_slot: u32,
    max_slots: u32,
    cur_ret: Type,
    cur_sret: bool,
}

pub fn compile(items: &[Item]) -> CResult<Module> {
    let mut c = Compiler {
        structs: Vec::new(),
        struct_ids: HashMap::new(),
        sigs: Vec::new(),
        fn_ids: HashMap::new(),
        strings: Vec::new(),
        string_ids: HashMap::new(),
        code: Vec::new(),
        scopes: Vec::new(),
        next_slot: 0,
        max_slots: 0,
        cur_ret: Type::Void,
        cur_sret: false,
    };
    c.run(items)
}

impl Compiler {
    fn run(&mut self, items: &[Item]) -> CResult<Module> {
        // 1. struct names
        for item in items {
            if let Item::Struct(sd) = item {
                if self.struct_ids.contains_key(&sd.name) {
                    return Err(CompileError::new(
                        format!("struct `{}` is already defined", sd.name),
                        sd.span,
                    ));
                }
                self.struct_ids.insert(sd.name.clone(), self.structs.len());
                self.structs.push(StructInfo {
                    name: sd.name.clone(),
                    fields: Vec::new(),
                    size: 0,
                    laid_out: false,
                });
            }
        }

        // 2. field types
        for item in items {
            if let Item::Struct(sd) = item {
                let id = self.struct_ids[&sd.name];
                let mut fields = Vec::new();
                for f in &sd.fields {
                    if fields.iter().any(|x: &FieldInfo| x.name == f.name) {
                        return Err(CompileError::new(
                            format!("duplicate field `{}` in struct `{}`", f.name, sd.name),
                            f.span,
                        ));
                    }
                    let ty = self.resolve_type(&f.ty)?;
                    if ty == Type::Void {
                        return Err(CompileError::new("a field cannot have type `void`", f.ty.span()));
                    }
                    fields.push(FieldInfo { name: f.name.clone(), ty, offset: 0 });
                }
                self.structs[id].fields = fields;
            }
        }

        // 3. layouts (detects value-recursive structs)
        for item in items {
            if let Item::Struct(sd) = item {
                let id = self.struct_ids[&sd.name];
                self.layout(id, sd.span, &mut vec![false; self.structs.len()])?;
            }
        }

        // 4. function signatures
        for item in items {
            if let Item::Fn(fd) = item {
                if NATIVES.iter().any(|n| n.0 == fd.name) {
                    return Err(CompileError::new(
                        format!("`{}` is a builtin and cannot be redefined", fd.name),
                        fd.span,
                    ));
                }
                if self.fn_ids.contains_key(&fd.name) {
                    return Err(CompileError::new(
                        format!("function `{}` is already defined", fd.name),
                        fd.span,
                    ));
                }
                if self.struct_ids.contains_key(&fd.name) {
                    return Err(CompileError::new(
                        format!("`{}` is already the name of a struct", fd.name),
                        fd.span,
                    ));
                }
                let mut params = Vec::new();
                for p in &fd.params {
                    let ty = self.resolve_type(&p.ty)?;
                    if ty == Type::Void {
                        return Err(CompileError::new(
                            "a parameter cannot have type `void`",
                            p.ty.span(),
                        ));
                    }
                    params.push(ty);
                }
                let ret = self.resolve_type(&fd.ret)?;
                self.fn_ids.insert(fd.name.clone(), self.sigs.len());
                self.sigs.push(FnSig { name: fd.name.clone(), params, ret, span: fd.span });
            }
        }

        let entry = match self.fn_ids.get("main") {
            Some(i) => *i,
            None => {
                return Err(CompileError::new(
                    "every program needs `function main(): i32`",
                    Span { line: 1, col: 1 },
                ))
            }
        };
        {
            let m = &self.sigs[entry];
            if !m.params.is_empty() || m.ret != Type::I32 {
                return Err(CompileError::new("`main` must be declared `function main(): i32`", m.span));
            }
        }

        // 5. bodies
        let mut funcs = Vec::new();
        for item in items {
            if let Item::Fn(fd) = item {
                let idx = self.fn_ids[&fd.name];
                funcs.push(self.compile_fn(fd, idx)?);
            }
        }

        Ok(Module { strings: std::mem::take(&mut self.strings), funcs, entry: entry as u32 })
    }

    // ------------------------------------------------------------- types

    fn resolve_type(&self, te: &TypeExpr) -> CResult<Type> {
        Ok(match te {
            TypeExpr::Name(n, sp) => match n.as_str() {
                "i32" => Type::I32,
                "i64" => Type::I64,
                "f64" => Type::F64,
                "bool" => Type::Bool,
                "str" => Type::Str,
                "void" => Type::Void,
                other => match self.struct_ids.get(other) {
                    Some(id) => Type::Struct(*id),
                    None => {
                        return Err(CompileError::new(format!("unknown type `{}`", other), *sp))
                    }
                },
            },
            TypeExpr::Ptr(inner, sp) => {
                let t = self.resolve_type(inner)?;
                if t == Type::Void {
                    return Err(CompileError::new("`*void` is not a type in binZ", *sp));
                }
                Type::Ptr(Box::new(t))
            }
            TypeExpr::Fn(params, ret, _) => {
                let mut ps = Vec::new();
                for p in params {
                    ps.push(self.resolve_type(p)?);
                }
                Type::Fn(ps, Box::new(self.resolve_type(ret)?))
            }
            TypeExpr::Array(inner, n, sp) => {
                let t = self.resolve_type(inner)?;
                if t == Type::Void {
                    return Err(CompileError::new("`[void; N]` is not a type in binZ", *sp));
                }
                let ty = Type::Array(Box::new(t), *n);
                if self.slots64(&ty) > MAX_SLOTS {
                    return Err(CompileError::new(
                        format!(
                            "`{}` needs more than {} slots; use `Vector<T>` for storage this large",
                            self.tn(&ty),
                            MAX_SLOTS
                        ),
                        *sp,
                    ));
                }
                ty
            }
            TypeExpr::Container(kind, inner, sp) => {
                let t = self.resolve_type(inner)?;
                if kind.is_set() {
                    if !t.is_key() {
                        return Err(CompileError::new(
                            format!(
                                "`{}` holds keys, so its element must be i32, i64, f64, bool or str, not `{}`",
                                kind.name(),
                                self.tn(&t)
                            ),
                            *sp,
                        ));
                    }
                } else if !t.is_slot() {
                    return Err(CompileError::new(
                        format!(
                            "`{}` holds one-slot values and cannot hold `{}`; put it behind a pointer, or use `[{}; N]`",
                            kind.name(),
                            self.tn(&t),
                            self.tn(&t)
                        ),
                        *sp,
                    ));
                }
                Type::Container(*kind, Box::new(t))
            }
        })
    }

    /// Slot footprint in `u64`, so that oversized array types are caught
    /// before they can overflow the real `u32` computation.
    fn slots64(&self, t: &Type) -> u64 {
        match t {
            Type::Struct(id) => self.structs[*id].size as u64,
            Type::Array(elem, n) => self.slots64(elem).saturating_mul(*n as u64),
            _ => 1,
        }
    }

    fn layout(&mut self, id: usize, span: Span, visiting: &mut Vec<bool>) -> CResult<u32> {
        if self.structs[id].laid_out {
            return Ok(self.structs[id].size);
        }
        if visiting[id] {
            return Err(CompileError::new(
                format!(
                    "struct `{}` contains itself by value; use a pointer field instead",
                    self.structs[id].name
                ),
                span,
            ));
        }
        visiting[id] = true;
        let mut offset = 0u32;
        let field_types: Vec<Type> =
            self.structs[id].fields.iter().map(|f| f.ty.clone()).collect();
        for (i, ty) in field_types.iter().enumerate() {
            let size = self.field_size(ty, span, visiting)?;
            self.structs[id].fields[i].offset = offset;
            offset += size;
        }
        visiting[id] = false;
        self.structs[id].size = offset;
        self.structs[id].laid_out = true;
        Ok(offset)
    }

    /// Size of a field type during layout, laying out whatever it depends on
    /// first. Arrays are transparent here, so `[Node; 2]` inside `Node` is
    /// still caught as value recursion.
    fn field_size(&mut self, ty: &Type, span: Span, visiting: &mut Vec<bool>) -> CResult<u32> {
        Ok(match ty {
            Type::Struct(inner) => self.layout(*inner, span, visiting)?,
            Type::Array(elem, n) => {
                let each = self.field_size(elem, span, visiting)?;
                match each.checked_mul(*n) {
                    Some(total) if (total as u64) <= MAX_SLOTS => total,
                    _ => {
                        return Err(CompileError::new(
                            format!("`{}` needs more than {} slots", self.tn(ty), MAX_SLOTS),
                            span,
                        ))
                    }
                }
            }
            _ => 1,
        })
    }

    fn size_of(&self, t: &Type) -> u32 {
        match t {
            Type::Struct(id) => self.structs[*id].size,
            Type::Array(elem, n) => self.size_of(elem) * n,
            _ => 1,
        }
    }

    fn tn(&self, t: &Type) -> String {
        type_name(t, &self.structs)
    }

    fn expect_type(&self, want: &Type, got: &Type, span: Span, what: &str) -> CResult<()> {
        if want == got {
            Ok(())
        } else {
            Err(CompileError::new(
                format!("{}: expected `{}`, found `{}`", what, self.tn(want), self.tn(got)),
                span,
            ))
        }
    }

    // ------------------------------------------------------------ emitting

    fn emit(&mut self, b: u8) {
        self.code.push(b);
    }
    fn emit_u32(&mut self, v: u32) {
        self.code.extend_from_slice(&v.to_le_bytes());
    }
    fn emit_op_u32(&mut self, op: u8, v: u32) {
        self.emit(op);
        self.emit_u32(v);
    }
    fn emit_jump(&mut self, op: u8) -> usize {
        self.emit(op);
        let at = self.code.len();
        self.emit_u32(0);
        at
    }
    fn patch_jump(&mut self, at: usize) {
        let rel = (self.code.len() as i64 - (at + 4) as i64) as i32;
        self.code[at..at + 4].copy_from_slice(&rel.to_le_bytes());
    }
    fn emit_jump_back(&mut self, op: u8, target: usize) {
        self.emit(op);
        let rel = (target as i64 - (self.code.len() + 4) as i64) as i32;
        self.emit_u32(rel as u32);
    }

    fn intern(&mut self, s: &str) -> u32 {
        if let Some(i) = self.string_ids.get(s) {
            return *i;
        }
        let i = self.strings.len() as u32;
        self.strings.push(s.to_string());
        self.string_ids.insert(s.to_string(), i);
        i
    }

    // ------------------------------------------------------------- scopes

    fn reserve(&mut self, size: u32) -> u32 {
        let slot = self.next_slot;
        self.next_slot += size;
        if self.next_slot > self.max_slots {
            self.max_slots = self.next_slot;
        }
        slot
    }

    fn declare(&mut self, name: &str, ty: Type, slot: u32, mutable: bool, span: Span) -> CResult<()> {
        if self.scopes.last().unwrap().iter().any(|l| l.name == name) {
            return Err(CompileError::new(
                format!("`{}` is already declared in this scope", name),
                span,
            ));
        }
        self.scopes
            .last_mut()
            .unwrap()
            .push(Local { name: name.to_string(), ty, slot, mutable });
        Ok(())
    }

    fn lookup(&self, name: &str) -> Option<Local> {
        for scope in self.scopes.iter().rev() {
            if let Some(l) = scope.iter().rev().find(|l| l.name == name) {
                return Some(l.clone());
            }
        }
        None
    }

    // ---------------------------------------------------------- functions

    fn compile_fn(&mut self, def: &FnDef, idx: usize) -> CResult<FnMeta> {
        let sig = self.sigs[idx].clone();
        self.code = Vec::new();
        self.scopes = vec![Vec::new()];
        self.next_slot = 0;
        self.max_slots = 0;
        self.cur_ret = sig.ret.clone();
        self.cur_sret = sig.ret.is_aggregate();

        let mut param_sizes = Vec::new();
        for (i, p) in def.params.iter().enumerate() {
            let ty = sig.params[i].clone();
            let size = self.size_of(&ty);
            param_sizes.push(size);
            let slot = self.reserve(size);
            self.declare(&p.name, ty, slot, true, p.span)?;
        }

        self.compile_block(&def.body)?;

        if !block_returns(&def.body) {
            if sig.ret == Type::Void {
                self.emit(OP_PUSH_VOID);
                self.emit(OP_RET);
            } else {
                return Err(CompileError::new(
                    format!(
                        "function `{}` must return `{}` on every path",
                        sig.name,
                        self.tn(&sig.ret)
                    ),
                    def.body.span,
                ));
            }
        }

        Ok(FnMeta {
            name: sig.name.clone(),
            param_sizes,
            n_slots: self.max_slots,
            sret: self.cur_sret,
            ret_size: self.size_of(&sig.ret),
            code: std::mem::take(&mut self.code),
        })
    }

    fn compile_block(&mut self, b: &Block) -> CResult<()> {
        self.scopes.push(Vec::new());
        let mark = self.next_slot;
        for s in &b.stmts {
            self.compile_stmt(s)?;
        }
        self.scopes.pop();
        self.next_slot = mark;
        Ok(())
    }

    // --------------------------------------------------------- statements

    fn compile_stmt(&mut self, s: &Stmt) -> CResult<()> {
        match s {
            Stmt::Let { mutable, name, ty, init, span } => {
                let declared = self.resolve_type(ty)?;
                if declared == Type::Void {
                    return Err(CompileError::new("a variable cannot have type `void`", ty.span()));
                }
                let size = self.size_of(&declared);
                let slot = self.reserve(size);
                let temp_mark = self.next_slot;
                if declared.is_aggregate() {
                    self.emit_op_u32(OP_ADDR_LOCAL, slot);
                    let got = self.compile_expr(init, Some(&declared))?;
                    self.expect_type(&declared, &got, init.span(), "in variable initializer")?;
                    self.emit_op_u32(OP_COPY, size);
                } else {
                    let got = self.compile_expr(init, Some(&declared))?;
                    self.expect_type(&declared, &got, init.span(), "in variable initializer")?;
                    self.emit_op_u32(OP_STORE_LOCAL, slot);
                }
                self.next_slot = temp_mark;
                self.declare(name, declared, slot, *mutable, *span)?;
            }

            Stmt::Assign { target, value, span } => {
                let mark = self.next_slot;
                // Fast path: plain scalar local.
                if let Expr::Ident(name, isp) = target {
                    if let Some(local) = self.lookup(name) {
                        if !local.mutable {
                            return Err(CompileError::new(
                                format!("`{}` is `const` and cannot be reassigned", name),
                                *isp,
                            ));
                        }
                        if !local.ty.is_aggregate() {
                            let got = self.compile_expr(value, Some(&local.ty))?;
                            self.expect_type(&local.ty, &got, value.span(), "in assignment")?;
                            self.emit_op_u32(OP_STORE_LOCAL, local.slot);
                            self.next_slot = mark;
                            return Ok(());
                        }
                    }
                }
                // An element of a heap container is not a place: the store
                // goes through the handle instead of through an address.
                if let Expr::Index { base, index, .. } = target {
                    let probe = self.next_slot;
                    let saved = std::mem::take(&mut self.code);
                    let bt = self.compile_expr(base, None)?;
                    let scratch = std::mem::replace(&mut self.code, saved);
                    self.next_slot = probe;
                    if let Type::Container(kind, elem) = bt {
                        if kind.is_set() {
                            return Err(CompileError::new(
                                format!(
                                    "the elements of `{}` are its keys; use `add` and `remove` instead of assigning",
                                    kind.name()
                                ),
                                *span,
                            ));
                        }
                        self.code.extend_from_slice(&scratch);
                        self.compile_index(index)?;
                        let got = self.compile_expr(value, Some(&elem))?;
                        self.expect_type(&elem, &got, value.span(), "in assignment")?;
                        self.emit(OP_SET);
                        self.next_slot = mark;
                        return Ok(());
                    }
                }
                let (tty, mutable) = self.compile_place(target)?;
                if !mutable {
                    return Err(CompileError::new(
                        "this place is immutable; declare it with `var` to assign to it",
                        *span,
                    ));
                }
                let got = self.compile_expr(value, Some(&tty))?;
                self.expect_type(&tty, &got, value.span(), "in assignment")?;
                if tty.is_aggregate() {
                    let n = self.size_of(&tty);
                    self.emit_op_u32(OP_COPY, n);
                } else {
                    self.emit(OP_STORE_PTR);
                }
                self.next_slot = mark;
            }

            Stmt::ExprStmt(e, span) => {
                if !matches!(e, Expr::Call { .. }) {
                    return Err(CompileError::new(
                        "only a function call may be used as a statement",
                        *span,
                    ));
                }
                let mark = self.next_slot;
                self.compile_expr(e, None)?;
                self.emit(OP_POP);
                self.next_slot = mark;
            }

            Stmt::Return(e, span) => {
                let mark = self.next_slot;
                let ret = self.cur_ret.clone();
                match e {
                    None => {
                        self.expect_type(&ret, &Type::Void, *span, "in `return`")?;
                        self.emit(OP_PUSH_VOID);
                    }
                    Some(e) => {
                        if ret == Type::Void {
                            return Err(CompileError::new(
                                "this function returns `void`; write `return;`",
                                *span,
                            ));
                        }
                        let got = self.compile_expr(e, Some(&ret))?;
                        self.expect_type(&ret, &got, e.span(), "in `return`")?;
                        if self.cur_sret {
                            let n = self.size_of(&ret);
                            self.emit_op_u32(OP_COPY_SRET, n);
                        }
                    }
                }
                self.emit(OP_RET);
                self.next_slot = mark;
            }

            Stmt::If { cond, then, els, .. } => {
                let mark = self.next_slot;
                let ct = self.compile_expr(cond, Some(&Type::Bool))?;
                self.expect_type(&Type::Bool, &ct, cond.span(), "in `if` condition")?;
                self.next_slot = mark;
                let else_jump = self.emit_jump(OP_JMP_IF_FALSE);
                self.compile_block(then)?;
                match els {
                    Some(eb) => {
                        let end_jump = self.emit_jump(OP_JMP);
                        self.patch_jump(else_jump);
                        self.compile_block(eb)?;
                        self.patch_jump(end_jump);
                    }
                    None => self.patch_jump(else_jump),
                }
            }

            Stmt::While { cond, body, .. } => {
                let top = self.code.len();
                let mark = self.next_slot;
                let ct = self.compile_expr(cond, Some(&Type::Bool))?;
                self.expect_type(&Type::Bool, &ct, cond.span(), "in `while` condition")?;
                self.next_slot = mark;
                let exit = self.emit_jump(OP_JMP_IF_FALSE);
                self.compile_block(body)?;
                self.emit_jump_back(OP_JMP, top);
                self.patch_jump(exit);
            }

            Stmt::Nested(b, _) => self.compile_block(b)?,
        }
        Ok(())
    }

    // -------------------------------------------------------------- places

    /// Emits code pushing the *address* of a place. Returns its type and
    /// whether it may be assigned through.
    fn compile_place(&mut self, e: &Expr) -> CResult<(Type, bool)> {
        match e {
            Expr::Ident(name, span) => match self.lookup(name) {
                Some(l) => {
                    self.emit_op_u32(OP_ADDR_LOCAL, l.slot);
                    Ok((l.ty, l.mutable))
                }
                None => Err(CompileError::new(
                    format!("`{}` is not a variable", name),
                    *span,
                )),
            },
            Expr::Field { base, name, span } => {
                let (bt, mutable) = self.compile_place(base)?;
                let id = match bt {
                    Type::Struct(id) => id,
                    Type::Ptr(_) => {
                        return Err(CompileError::new(
                            format!(
                                "cannot read `.{}` through a pointer; dereference it first: `(*p).{}`",
                                name, name
                            ),
                            *span,
                        ))
                    }
                    other => {
                        return Err(CompileError::new(
                            format!("type `{}` has no fields", self.tn(&other)),
                            *span,
                        ))
                    }
                };
                let field = match self.structs[id].fields.iter().find(|f| &f.name == name) {
                    Some(f) => f.clone(),
                    None => {
                        return Err(CompileError::new(
                            format!("struct `{}` has no field `{}`", self.structs[id].name, name),
                            *span,
                        ))
                    }
                };
                self.emit_op_u32(OP_FIELD, field.offset);
                Ok((field.ty, mutable))
            }
            Expr::Index { base, index, span } => {
                let (bt, mutable) = self.compile_place(base)?;
                match bt {
                    Type::Array(elem, n) => {
                        self.compile_index(index)?;
                        let size = self.size_of(&elem);
                        self.emit(OP_ELEM);
                        self.emit_u32(size);
                        self.emit_u32(n);
                        Ok((*elem, mutable))
                    }
                    Type::Container(kind, _) => Err(CompileError::new(
                        format!(
                            "an element of `{}<...>` lives on the heap and has no address; copy it into a variable first",
                            kind.name()
                        ),
                        *span,
                    )),
                    other => Err(CompileError::new(
                        format!("type `{}` cannot be indexed", self.tn(&other)),
                        *span,
                    )),
                }
            }
            Expr::Unary { op: UnOp::Deref, expr, span } => {
                let t = self.compile_expr(expr, None)?;
                match t {
                    Type::Ptr(inner) => Ok((*inner, true)),
                    other => Err(CompileError::new(
                        format!("cannot dereference `{}`", self.tn(&other)),
                        *span,
                    )),
                }
            }
            other => {
                // A struct rvalue (call result, literal) already lives in memory,
                // so it can serve as a read-only place.
                let span = other.span();
                let t = self.compile_expr(other, None)?;
                if t.is_aggregate() {
                    Ok((t, false))
                } else {
                    Err(CompileError::new("this expression is not a place", span))
                }
            }
        }
    }

    // --------------------------------------------------------- expressions

    /// Emits code pushing the value of `e`. Struct-typed expressions push the
    /// *address* of their storage; everything else pushes a scalar.
    fn compile_expr(&mut self, e: &Expr, hint: Option<&Type>) -> CResult<Type> {
        match e {
            Expr::Int(v, span) => {
                let ty = match hint {
                    Some(Type::I64) => Type::I64,
                    Some(Type::F64) => {
                        return Err(CompileError::new(
                            "expected `f64`; write a decimal point (`1.0`) or use `cast<f64>(...)`",
                            *span,
                        ))
                    }
                    _ => Type::I32,
                };
                if ty == Type::I32 {
                    if *v > i32::MAX as i64 {
                        return Err(CompileError::new(
                            "integer literal does not fit in `i32`",
                            *span,
                        ));
                    }
                    self.emit(OP_PUSH_I32);
                    self.emit_u32(*v as i32 as u32);
                } else {
                    self.emit(OP_PUSH_I64);
                    self.code.extend_from_slice(&v.to_le_bytes());
                }
                Ok(ty)
            }
            Expr::Float(v, _) => {
                self.emit(OP_PUSH_F64);
                self.code.extend_from_slice(&v.to_le_bytes());
                Ok(Type::F64)
            }
            Expr::Bool(v, _) => {
                self.emit(OP_PUSH_BOOL);
                self.emit(if *v { 1 } else { 0 });
                Ok(Type::Bool)
            }
            Expr::Str(s, _) => {
                let idx = self.intern(s);
                self.emit_op_u32(OP_PUSH_STR, idx);
                Ok(Type::Str)
            }

            Expr::Ident(name, span) => {
                if let Some(l) = self.lookup(name) {
                    if l.ty.is_aggregate() {
                        self.emit_op_u32(OP_ADDR_LOCAL, l.slot);
                    } else {
                        self.emit_op_u32(OP_LOAD_LOCAL, l.slot);
                    }
                    return Ok(l.ty);
                }
                if let Some(idx) = self.fn_ids.get(name) {
                    let sig = self.sigs[*idx].clone();
                    self.emit_op_u32(OP_PUSH_FN, *idx as u32);
                    return Ok(Type::Fn(sig.params, Box::new(sig.ret)));
                }
                if let Some(idx) = NATIVES.iter().position(|n| n.0 == name) {
                    self.emit_op_u32(OP_PUSH_NATIVE, idx as u32);
                    return Ok(NATIVES[idx].1());
                }
                if BUILTINS.iter().any(|b| b.0 == name) {
                    return Err(CompileError::new(
                        format!(
                            "`{}` is generic over the element type, so it is not a value; call it directly, or declare your own `{}` to shadow it",
                            name, name
                        ),
                        *span,
                    ));
                }
                Err(CompileError::new(format!("`{}` is not defined", name), *span))
            }

            Expr::Unary { op, expr, span } => match op {
                UnOp::Neg => {
                    let t = self.compile_expr(expr, hint)?;
                    if !t.is_numeric() {
                        return Err(CompileError::new(
                            format!("cannot negate `{}`", self.tn(&t)),
                            *span,
                        ));
                    }
                    self.emit(OP_NEG);
                    Ok(t)
                }
                UnOp::Not => {
                    let t = self.compile_expr(expr, Some(&Type::Bool))?;
                    self.expect_type(&Type::Bool, &t, *span, "in `!`")?;
                    self.emit(OP_NOT);
                    Ok(Type::Bool)
                }
                UnOp::Deref => {
                    let t = self.compile_expr(expr, None)?;
                    match t {
                        Type::Ptr(inner) => {
                            // An aggregate is already represented by its address.
                            if !inner.is_aggregate() {
                                self.emit(OP_LOAD_PTR);
                            }
                            Ok(*inner)
                        }
                        other => Err(CompileError::new(
                            format!("cannot dereference `{}`", self.tn(&other)),
                            *span,
                        )),
                    }
                }
                UnOp::AddrOf => {
                    let (t, mutable) = self.compile_place(expr)?;
                    if !mutable {
                        return Err(CompileError::new(
                            "can only take the address of a `var` place",
                            *span,
                        ));
                    }
                    Ok(Type::Ptr(Box::new(t)))
                }
            },

            Expr::Binary { op, lhs, rhs, span } => self.compile_binary(*op, lhs, rhs, *span, hint),

            Expr::Cast { ty, expr, span } => {
                let target = self.resolve_type(ty)?;
                let src = self.compile_expr(expr, None)?;
                if src == target {
                    return Ok(target);
                }
                let kind = match (&src, &target) {
                    (a, Type::I32) if a.is_numeric() => CAST_I32,
                    (a, Type::I64) if a.is_numeric() => CAST_I64,
                    (a, Type::F64) if a.is_numeric() => CAST_F64,
                    (Type::I32, Type::Str)
                    | (Type::I64, Type::Str)
                    | (Type::F64, Type::Str)
                    | (Type::Bool, Type::Str) => CAST_STR,
                    _ => {
                        return Err(CompileError::new(
                            format!(
                                "cannot cast `{}` to `{}`",
                                self.tn(&src),
                                self.tn(&target)
                            ),
                            *span,
                        ))
                    }
                };
                self.emit(OP_CAST);
                self.emit(kind);
                Ok(target)
            }

            Expr::Field { .. } => {
                let (t, _) = self.compile_place(e)?;
                if !t.is_aggregate() {
                    self.emit(OP_LOAD_PTR);
                }
                Ok(t)
            }

            Expr::Index { base, index, span } => {
                // Aggregates leave an address behind, containers a handle, so
                // the base compiles the same way in both cases.
                let bt = self.compile_expr(base, None)?;
                match bt {
                    Type::Array(elem, n) => {
                        self.compile_index(index)?;
                        let size = self.size_of(&elem);
                        self.emit(OP_ELEM);
                        self.emit_u32(size);
                        self.emit_u32(n);
                        if !elem.is_aggregate() {
                            self.emit(OP_LOAD_PTR);
                        }
                        Ok(*elem)
                    }
                    Type::Container(_, elem) => {
                        self.compile_index(index)?;
                        self.emit(OP_GET);
                        Ok(*elem)
                    }
                    other => Err(CompileError::new(
                        format!("type `{}` cannot be indexed", self.tn(&other)),
                        *span,
                    )),
                }
            }

            Expr::ArrayLit { elems, span } => self.compile_array_lit(elems, *span, hint),

            Expr::ArrayRepeat { value, count, span } => {
                self.compile_array_repeat(value, *count, *span, hint)
            }

            Expr::ContainerLit { kind, elem, elems, span } => {
                self.compile_container_lit(*kind, elem, elems, *span)
            }

            Expr::StructLit { name, fields, span } => self.compile_struct_lit(name, fields, *span),

            Expr::Call { callee, args, span } => self.compile_call(callee, args, *span),
        }
    }

    fn compile_binary(
        &mut self,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
        span: Span,
        hint: Option<&Type>,
    ) -> CResult<Type> {
        // Short-circuit operators have their own control flow.
        if op == BinOp::And || op == BinOp::Or {
            let lt = self.compile_expr(lhs, Some(&Type::Bool))?;
            self.expect_type(&Type::Bool, &lt, lhs.span(), "in logical operator")?;
            let short = self.emit_jump(if op == BinOp::And { OP_JMP_IF_FALSE } else { OP_JMP_IF_TRUE });
            let rt = self.compile_expr(rhs, Some(&Type::Bool))?;
            self.expect_type(&Type::Bool, &rt, rhs.span(), "in logical operator")?;
            let end = self.emit_jump(OP_JMP);
            self.patch_jump(short);
            self.emit(OP_PUSH_BOOL);
            self.emit(if op == BinOp::And { 0 } else { 1 });
            self.patch_jump(end);
            return Ok(Type::Bool);
        }

        let arith = matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem);
        let sub_hint = if arith { hint.cloned() } else { None };

        // `1 + n` must type the literal from `n`, so compile the non-literal
        // side into a scratch buffer first and splice it back in order.
        let (lt, rt) = if lhs.is_literal() && !rhs.is_literal() {
            let saved = std::mem::take(&mut self.code);
            let rt = self.compile_expr(rhs, sub_hint.as_ref())?;
            let rbuf = std::mem::replace(&mut self.code, saved);
            let lt = self.compile_expr(lhs, Some(&rt))?;
            self.code.extend_from_slice(&rbuf);
            (lt, rt)
        } else {
            let lt = self.compile_expr(lhs, sub_hint.as_ref())?;
            let rt = self.compile_expr(rhs, Some(&lt))?;
            (lt, rt)
        };

        if lt != rt {
            return Err(CompileError::new(
                format!(
                    "`{}` needs both sides to have the same type, found `{}` and `{}` (binZ never converts implicitly; use `cast<T>(...)`)",
                    op.symbol(),
                    self.tn(&lt),
                    self.tn(&rt)
                ),
                span,
            ));
        }

        let ok = match op {
            BinOp::Add => lt.is_numeric() || lt == Type::Str,
            BinOp::Sub | BinOp::Mul | BinOp::Div => lt.is_numeric(),
            BinOp::Rem => lt.is_integer(),
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => lt.is_numeric() || lt == Type::Str,
            BinOp::Eq | BinOp::Ne => {
                lt.is_numeric() || matches!(lt, Type::Bool | Type::Str | Type::Ptr(_))
            }
            BinOp::And | BinOp::Or => unreachable!(),
        };
        if !ok {
            return Err(CompileError::new(
                format!("`{}` is not defined for `{}`", op.symbol(), self.tn(&lt)),
                span,
            ));
        }

        self.emit(match op {
            BinOp::Add => OP_ADD,
            BinOp::Sub => OP_SUB,
            BinOp::Mul => OP_MUL,
            BinOp::Div => OP_DIV,
            BinOp::Rem => OP_REM,
            BinOp::Eq => OP_EQ,
            BinOp::Ne => OP_NE,
            BinOp::Lt => OP_LT,
            BinOp::Le => OP_LE,
            BinOp::Gt => OP_GT,
            BinOp::Ge => OP_GE,
            BinOp::And | BinOp::Or => unreachable!(),
        });

        Ok(if arith { lt } else { Type::Bool })
    }

    fn compile_struct_lit(
        &mut self,
        name: &str,
        fields: &[(String, Expr, Span)],
        span: Span,
    ) -> CResult<Type> {
        let id = match self.struct_ids.get(name) {
            Some(id) => *id,
            None => return Err(CompileError::new(format!("unknown struct `{}`", name), span)),
        };
        let info = self.structs[id].clone();
        if fields.len() != info.fields.len() {
            return Err(CompileError::new(
                format!(
                    "struct `{}` has {} fields but {} were given; binZ requires every field, in declaration order",
                    name,
                    info.fields.len(),
                    fields.len()
                ),
                span,
            ));
        }
        let slot = self.reserve(info.size);
        for (i, decl) in info.fields.iter().enumerate() {
            let (given_name, value, fspan) = &fields[i];
            if given_name != &decl.name {
                return Err(CompileError::new(
                    format!(
                        "expected field `{}` here; binZ requires fields in declaration order",
                        decl.name
                    ),
                    *fspan,
                ));
            }
            self.emit_op_u32(OP_ADDR_LOCAL, slot);
            self.emit_op_u32(OP_FIELD, decl.offset);
            let got = self.compile_expr(value, Some(&decl.ty))?;
            self.expect_type(&decl.ty, &got, value.span(), "in struct literal")?;
            if decl.ty.is_aggregate() {
                let n = self.size_of(&decl.ty);
                self.emit_op_u32(OP_COPY, n);
            } else {
                self.emit(OP_STORE_PTR);
            }
        }
        self.emit_op_u32(OP_ADDR_LOCAL, slot);
        Ok(Type::Struct(id))
    }

    /// Every index in binZ is an `i32`, so there is one thing to write here.
    fn compile_index(&mut self, index: &Expr) -> CResult<()> {
        let it = self.compile_expr(index, Some(&Type::I32))?;
        self.expect_type(&Type::I32, &it, index.span(), "in an index")?;
        Ok(())
    }

    /// The declared type is what tells an array literal its element type and
    /// its length, so a literal is only legal where that type is known.
    fn array_hint(&self, hint: Option<&Type>, span: Span) -> CResult<(Type, u32)> {
        match hint {
            Some(Type::Array(elem, n)) => Ok(((**elem).clone(), *n)),
            Some(other) => Err(CompileError::new(
                format!("expected `{}`, found an array literal", self.tn(other)),
                span,
            )),
            None => Err(CompileError::new(
                "an array literal needs a declared type: `const a: [i32; 3] = [1, 2, 3];`",
                span,
            )),
        }
    }

    /// Stores one element of an array whose base address is already on the
    /// stack below the value.
    fn store_elem(&mut self, elem: &Type) {
        if elem.is_aggregate() {
            let n = self.size_of(elem);
            self.emit_op_u32(OP_COPY, n);
        } else {
            self.emit(OP_STORE_PTR);
        }
    }

    fn compile_array_lit(
        &mut self,
        elems: &[Expr],
        span: Span,
        hint: Option<&Type>,
    ) -> CResult<Type> {
        let (elem, n) = self.array_hint(hint, span)?;
        if elems.len() as u32 != n {
            return Err(CompileError::new(
                format!(
                    "`[{}; {}]` needs {} element(s) but {} were given; binZ requires every element",
                    self.tn(&elem),
                    n,
                    n,
                    elems.len()
                ),
                span,
            ));
        }
        let esize = self.size_of(&elem);
        let slot = self.reserve(esize * n);
        for (i, e) in elems.iter().enumerate() {
            self.emit_op_u32(OP_ADDR_LOCAL, slot);
            self.emit_op_u32(OP_FIELD, i as u32 * esize);
            let got = self.compile_expr(e, Some(&elem))?;
            self.expect_type(&elem, &got, e.span(), "in array literal")?;
            self.store_elem(&elem);
        }
        self.emit_op_u32(OP_ADDR_LOCAL, slot);
        Ok(Type::Array(Box::new(elem), n))
    }

    /// `[x; N]` evaluates `x` once and fans it out with a runtime loop, so a
    /// long array costs the same code as a short one.
    fn compile_array_repeat(
        &mut self,
        value: &Expr,
        count: u32,
        span: Span,
        hint: Option<&Type>,
    ) -> CResult<Type> {
        let (elem, n) = self.array_hint(hint, span)?;
        if count != n {
            return Err(CompileError::new(
                format!("expected `[{}; {}]`, found {} elements", self.tn(&elem), n, count),
                span,
            ));
        }
        let esize = self.size_of(&elem);
        let slot = self.reserve(esize * n);

        self.emit_op_u32(OP_ADDR_LOCAL, slot);
        let got = self.compile_expr(value, Some(&elem))?;
        self.expect_type(&elem, &got, value.span(), "in array literal")?;
        self.store_elem(&elem);

        if n > 1 {
            let counter = self.reserve(1);
            self.emit(OP_PUSH_I32);
            self.emit_u32(1);
            self.emit_op_u32(OP_STORE_LOCAL, counter);
            let top = self.code.len();
            self.emit_op_u32(OP_LOAD_LOCAL, counter);
            self.emit(OP_PUSH_I32);
            self.emit_u32(n);
            self.emit(OP_LT);
            let exit = self.emit_jump(OP_JMP_IF_FALSE);
            // destination: element `counter`
            self.emit_op_u32(OP_ADDR_LOCAL, slot);
            self.emit_op_u32(OP_LOAD_LOCAL, counter);
            self.emit(OP_ELEM);
            self.emit_u32(esize);
            self.emit_u32(n);
            // source: element 0
            self.emit_op_u32(OP_ADDR_LOCAL, slot);
            if elem.is_aggregate() {
                self.emit_op_u32(OP_COPY, esize);
            } else {
                self.emit(OP_LOAD_PTR);
                self.emit(OP_STORE_PTR);
            }
            self.emit_op_u32(OP_LOAD_LOCAL, counter);
            self.emit(OP_PUSH_I32);
            self.emit_u32(1);
            self.emit(OP_ADD);
            self.emit_op_u32(OP_STORE_LOCAL, counter);
            self.emit_jump_back(OP_JMP, top);
            self.patch_jump(exit);
        }

        self.emit_op_u32(OP_ADDR_LOCAL, slot);
        Ok(Type::Array(Box::new(elem), n))
    }

    fn compile_container_lit(
        &mut self,
        kind: Kind,
        elem: &TypeExpr,
        elems: &[Expr],
        span: Span,
    ) -> CResult<Type> {
        let ty = self.resolve_type(&TypeExpr::Container(kind, Box::new(elem.clone()), span))?;
        let et = match &ty {
            Type::Container(_, e) => (**e).clone(),
            _ => unreachable!(),
        };
        for e in elems {
            let got = self.compile_expr(e, Some(&et))?;
            self.expect_type(&et, &got, e.span(), "in container literal")?;
        }
        self.emit(OP_NEW);
        self.emit(kind_code(kind));
        self.emit_u32(elems.len() as u32);
        Ok(ty)
    }

    /// Container builtins are special forms: they are generic over the
    /// element type, which binZ has no way to write in a signature.
    fn compile_builtin(
        &mut self,
        name: &str,
        id: u8,
        arity: usize,
        args: &[Expr],
        span: Span,
    ) -> CResult<Type> {
        if args.len() != arity {
            return Err(CompileError::new(
                format!("`{}` takes {} argument(s), found {}", name, arity, args.len()),
                span,
            ));
        }
        let ct = self.compile_expr(&args[0], None)?;

        // `len` is the one builtin that also answers for a `str`.
        if let Type::Str = ct {
            if id == B_LEN {
                self.emit(OP_BUILTIN);
                self.emit(B_LEN);
                return Ok(Type::I32);
            }
        }

        let (elem, kind) = match &ct {
            Type::Array(elem, n) => {
                match id {
                    B_LEN => {
                        // The length is part of the type; drop the base address.
                        self.emit(OP_POP);
                        self.emit(OP_PUSH_I32);
                        self.emit_u32(*n);
                        return Ok(Type::I32);
                    }
                    B_FIND => {
                        if elem.is_aggregate() {
                            return Err(CompileError::new(
                                format!(
                                    "`find` compares one-slot values and cannot search `{}`",
                                    self.tn(&ct)
                                ),
                                span,
                            ));
                        }
                        let et = (**elem).clone();
                        let got = self.compile_expr(&args[1], Some(&et))?;
                        self.expect_type(&et, &got, args[1].span(), "in `find`")?;
                        self.emit(OP_ARR_FIND);
                        self.emit_u32(*n);
                        return Ok(Type::I32);
                    }
                    _ => {
                        return Err(CompileError::new(
                            format!(
                                "`{}` is not defined for `{}`; a fixed array never changes size, use `Vector<{}>`",
                                name,
                                self.tn(&ct),
                                self.tn(elem)
                            ),
                            span,
                        ))
                    }
                }
            }
            Type::Container(kind, elem) => ((**elem).clone(), *kind),
            other => {
                return Err(CompileError::new(
                    format!("`{}` needs a container, found `{}`", name, self.tn(other)),
                    span,
                ))
            }
        };

        let wants_sequence = matches!(id, B_FIND | B_PUSH | B_POP | B_INSERT | B_ERASE);
        let wants_set = matches!(id, B_ADD | B_REMOVE | B_CONTAINS);
        if wants_sequence && kind.is_set() {
            let instead = match id {
                B_FIND => "use `contains`",
                B_PUSH => "use `add`",
                B_ERASE => "use `remove`",
                _ => "a set has no positions",
            };
            return Err(CompileError::new(
                format!("`{}` is not defined for `{}`; {}", name, self.tn(&ct), instead),
                span,
            ));
        }
        if wants_set && kind.is_sequence() {
            let instead = match id {
                B_ADD => "use `push`",
                B_REMOVE => "use `erase`",
                _ => "use `find`",
            };
            return Err(CompileError::new(
                format!("`{}` is not defined for `{}`; {}", name, self.tn(&ct), instead),
                span,
            ));
        }

        // Remaining arguments, in source order.
        match id {
            B_INSERT => {
                self.compile_index(&args[1])?;
                let got = self.compile_expr(&args[2], Some(&elem))?;
                self.expect_type(&elem, &got, args[2].span(), "in `insert`")?;
            }
            B_ERASE => self.compile_index(&args[1])?,
            B_FIND | B_PUSH | B_ADD | B_REMOVE | B_CONTAINS => {
                let got = self.compile_expr(&args[1], Some(&elem))?;
                self.expect_type(&elem, &got, args[1].span(), "in argument")?;
            }
            _ => {}
        }

        self.emit(OP_BUILTIN);
        self.emit(id);

        Ok(match id {
            B_LEN | B_FIND => Type::I32,
            B_POP | B_ERASE => elem,
            B_ADD | B_REMOVE | B_CONTAINS => Type::Bool,
            B_COPY => ct,
            _ => Type::Void,
        })
    }

    fn compile_call(&mut self, callee: &Expr, args: &[Expr], span: Span) -> CResult<Type> {
        // Container builtins are resolved last: anything the program declares
        // with the same name wins, exactly like an ordinary shadow.
        if let Expr::Ident(name, _) = callee {
            if self.lookup(name).is_none() && !self.fn_ids.contains_key(name) {
                if let Some((n, id, arity)) = BUILTINS.iter().find(|b| b.0 == name) {
                    return self.compile_builtin(n, *id, *arity, args, span);
                }
            }
        }
        let ct = self.compile_expr(callee, None)?;
        let (params, ret) = match ct {
            Type::Fn(p, r) => (p, *r),
            other => {
                return Err(CompileError::new(
                    format!("`{}` is not callable", self.tn(&other)),
                    span,
                ))
            }
        };
        if args.len() != params.len() {
            return Err(CompileError::new(
                format!("expected {} argument(s), found {}", params.len(), args.len()),
                span,
            ));
        }
        let mut argc = args.len() as u32;
        if ret.is_aggregate() {
            // Reserve the caller-side destination for the returned struct.
            let n = self.size_of(&ret);
            let slot = self.reserve(n);
            self.emit_op_u32(OP_ADDR_LOCAL, slot);
            argc += 1;
        }
        for (a, p) in args.iter().zip(params.iter()) {
            let got = self.compile_expr(a, Some(p))?;
            self.expect_type(p, &got, a.span(), "in argument")?;
        }
        self.emit_op_u32(OP_CALL, argc);
        Ok(ret)
    }
}

fn kind_code(k: Kind) -> u8 {
    match k {
        Kind::Vector => KIND_VECTOR,
        Kind::List => KIND_LIST,
        Kind::Set => KIND_SET,
        Kind::SortedSet => KIND_SORTED_SET,
    }
}

/// Conservative "does this block return on every path" check.
fn block_returns(b: &Block) -> bool {
    b.stmts.iter().any(stmt_returns)
}

fn stmt_returns(s: &Stmt) -> bool {
    match s {
        Stmt::Return(..) => true,
        Stmt::Nested(b, _) => block_returns(b),
        Stmt::If { then, els: Some(e), .. } => block_returns(then) && block_returns(e),
        _ => false,
    }
}
