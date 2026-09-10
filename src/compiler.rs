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
        })
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
            let size = match ty {
                Type::Struct(inner) => self.layout(*inner, span, visiting)?,
                _ => 1,
            };
            self.structs[id].fields[i].offset = offset;
            offset += size;
        }
        visiting[id] = false;
        self.structs[id].size = offset;
        self.structs[id].laid_out = true;
        Ok(offset)
    }

    fn size_of(&self, t: &Type) -> u32 {
        match t {
            Type::Struct(id) => self.structs[*id].size,
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
        self.cur_sret = sig.ret.is_struct();

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
                if declared.is_struct() {
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
                        if !local.ty.is_struct() {
                            let got = self.compile_expr(value, Some(&local.ty))?;
                            self.expect_type(&local.ty, &got, value.span(), "in assignment")?;
                            self.emit_op_u32(OP_STORE_LOCAL, local.slot);
                            self.next_slot = mark;
                            return Ok(());
                        }
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
                if tty.is_struct() {
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
                if t.is_struct() {
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
                    if l.ty.is_struct() {
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
                            // A struct is already represented by its address.
                            if !inner.is_struct() {
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
                if !t.is_struct() {
                    self.emit(OP_LOAD_PTR);
                }
                Ok(t)
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
            if decl.ty.is_struct() {
                let n = self.size_of(&decl.ty);
                self.emit_op_u32(OP_COPY, n);
            } else {
                self.emit(OP_STORE_PTR);
            }
        }
        self.emit_op_u32(OP_ADDR_LOCAL, slot);
        Ok(Type::Struct(id))
    }

    fn compile_call(&mut self, callee: &Expr, args: &[Expr], span: Span) -> CResult<Type> {
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
        if ret.is_struct() {
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
