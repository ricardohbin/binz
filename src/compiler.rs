//! Single pass over the AST that type checks and emits bytecode at the same
//! time. binZ has no type inference beyond literal typing, so one pass with a
//! type hint threaded downwards is enough.

use std::collections::HashMap;

use crate::ast::*;
use crate::bytecode::*;
use crate::error::{CResult, CompileError};
use crate::lexer::Span;
use crate::loader::{Program, SourceFile};
use crate::types::*;
use crate::stdlib;

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

/// What one file can see. Names are per file: `math.binz` and `main.binz`
/// may both define `add`, and an `import` is visible only in the file that
/// writes it. Only the *ids* are shared -- a struct id and a function id
/// index the whole program, because they are what the bytecode holds.
#[derive(Default)]
struct FileScope {
    /// `binz/<name>` modules imported here, by binding.
    std_imports: Vec<String>,
    /// `@root/...` modules imported here, as `(binding, file index)`.
    mod_imports: Vec<(String, usize)>,
    struct_ids: HashMap<String, usize>,
    fn_ids: HashMap<String, usize>,
}

/// A qualified name's left-hand side: `io` in `io.print`, `math` in
/// `math.add`.
enum ModRef {
    Std(String),
    /// Index into `Compiler::files`.
    Local(usize),
}

pub struct Compiler {
    /// One scope per file of the program, indexed as `Program::files`.
    files: Vec<FileScope>,
    /// The `@root/...` spelling of each file, for diagnostics.
    displays: Vec<String>,
    /// The file being compiled right now.
    cur: usize,
    structs: Vec<StructInfo>,
    sigs: Vec<FnSig>,
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

pub fn compile(prog: &Program) -> CResult<Module> {
    let mut c = Compiler {
        files: prog.files.iter().map(|_| FileScope::default()).collect(),
        displays: prog.files.iter().map(|f| f.display.clone()).collect(),
        cur: prog.entry,
        structs: Vec::new(),
        sigs: Vec::new(),
        strings: Vec::new(),
        string_ids: HashMap::new(),
        code: Vec::new(),
        scopes: Vec::new(),
        next_slot: 0,
        max_slots: 0,
        cur_ret: Type::Void,
        cur_sret: false,
    };
    c.run(prog)
}

impl Compiler {
    fn run(&mut self, prog: &Program) -> CResult<Module> {
        // Every phase runs over every file before the next one begins, so a
        // module and the file importing it are indistinguishable: either may
        // be written first, and neither has to be compiled twice.
        self.each_file(prog, Self::declare_imports)?;
        self.each_file(prog, Self::declare_structs)?;
        self.each_file(prog, Self::declare_fields)?;
        self.each_file(prog, Self::lay_out_structs)?;
        self.each_file(prog, Self::declare_fns)?;

        // `main` is the program, so it belongs to the entry file and to no
        // other. A module that grew one is almost certainly a file that was
        // meant to be run.
        for (i, f) in prog.files.iter().enumerate() {
            if i == prog.entry {
                continue;
            }
            if let Some(fd) = fn_named(f, "main") {
                return Err(CompileError::new(
                    format!(
                        "`{}` defines `main`, but only the file passed to `binz` does; \
                         an imported module is a library",
                        f.display
                    ),
                    fd.span,
                )
                .at_file(&f.path));
            }
        }

        let entry = match self.files[prog.entry].fn_ids.get("main") {
            Some(i) => *i,
            None => {
                return Err(CompileError::new(
                    "every program needs `function main(): i32`",
                    Span { line: 1, col: 1 },
                )
                .at_file(&prog.files[prog.entry].path))
            }
        };
        {
            let m = &self.sigs[entry];
            if !m.params.is_empty() || m.ret != Type::I32 {
                return Err(CompileError::new(
                    "`main` must be declared `function main(): i32`",
                    m.span,
                )
                .at_file(&prog.files[prog.entry].path));
            }
        }

        // Bodies last, so any file may call into any file it imported.
        // `funcs` is indexed by function id rather than appended to, because
        // the ids were handed out per file in `declare_fns`.
        let mut funcs: Vec<Option<FnMeta>> = (0..self.sigs.len()).map(|_| None).collect();
        for (i, f) in prog.files.iter().enumerate() {
            self.cur = i;
            for item in &f.items {
                if let Item::Fn(fd) = item {
                    let idx = self.files[i].fn_ids[&fd.name];
                    funcs[idx] = Some(self.compile_fn(fd, idx).map_err(|e| e.at_file(&f.path))?);
                }
            }
        }

        Ok(Module {
            strings: std::mem::take(&mut self.strings),
            funcs: funcs.into_iter().map(|f| f.expect("every signature got a body")).collect(),
            entry: entry as u32,
        })
    }

    /// Run one phase over every file, with `cur` set and every diagnostic
    /// labelled with the file it came from.
    fn each_file(
        &mut self,
        prog: &Program,
        phase: fn(&mut Self, &SourceFile) -> CResult<()>,
    ) -> CResult<()> {
        for (i, f) in prog.files.iter().enumerate() {
            self.cur = i;
            phase(self, f).map_err(|e| e.at_file(&f.path))?;
        }
        Ok(())
    }

    // 0. imports. A module is bound to the last segment of its path and to
    // nothing else, so `binz/io` is always reached as `io.` and
    // `@root/utils/math.binz` as `math.`.
    fn declare_imports(&mut self, f: &SourceFile) -> CResult<()> {
        // Two directories may hold two files of the same name. Find those
        // names first: they are the only place `as` is legal, and the only
        // place it is required.
        let contested = contested_names(f);

        // `deps` holds one file index per local import, in source order.
        let mut dep = 0;
        for item in &f.items {
            let im = match item {
                Item::Import(im) => im,
                _ => continue,
            };
            let name = im.binding().to_string();
            if im.local {
                let target = f.deps[dep];
                dep += 1;
                if self.files[self.cur].mod_imports.iter().any(|(_, t)| *t == target) {
                    return Err(CompileError::new(
                        format!("`{}` is already imported in this file", im.text()),
                        im.span,
                    ));
                }
                self.check_rename(im, &contested)?;
                // Only the file's own name can collide with the standard
                // library: a rename always carries a capital at the join, so
                // it can never be a lowercase module name.
                if stdlib::is_module(im.own_name()) {
                    return Err(CompileError::new(
                        format!(
                            "`{}` is the standard library module `binz/{}`; a local module \
                             cannot take its name",
                            im.own_name(),
                            im.own_name()
                        ),
                        im.span,
                    ));
                }
                self.check_unbound(&name, im.span)?;
                self.files[self.cur].mod_imports.push((name, target));
            } else {
                if let Some(a) = &im.alias {
                    return Err(CompileError::new(
                        format!(
                            "`{}` is always reached as `{}`; `as` renames a module only when \
                             two or more imports in a file are named the same, and no two \
                             standard library modules are",
                            im.text(),
                            im.own_name()
                        ),
                        a.span,
                    ));
                }
                if im.path.len() != 2 || im.path[0] != "binz" {
                    return Err(CompileError::new(
                        format!(
                            "`{}` is not a module path; a standard library module is spelled \
                             `binz/<name>` ({}), and a file of this project `@root/<path>.binz`",
                            im.text(),
                            stdlib::MODULES.join(", ")
                        ),
                        im.span,
                    ));
                }
                if !stdlib::is_module(&name) {
                    return Err(CompileError::new(
                        format!(
                            "there is no module `binz/{}`; binZ has {}",
                            name,
                            stdlib::MODULES.join(", ")
                        ),
                        im.span,
                    ));
                }
                self.check_unbound(&name, im.span)?;
                self.files[self.cur].std_imports.push(name);
            }
        }
        Ok(())
    }

    /// `as` exists for exactly one situation: this file imports two or more
    /// modules that are named the same. Then **every one of them** is
    /// renamed, so a name is never the default for one import and a rename
    /// for another -- and outside that situation `as` is an error, because a
    /// module would otherwise have two spellings.
    ///
    /// The rename itself is not a choice either: it is the directory and the
    /// file name joined, so two people renaming the same clash write the same
    /// line.
    fn check_rename(
        &self,
        im: &ImportDef,
        contested: &[(String, Vec<String>)],
    ) -> CResult<()> {
        let clash = contested.iter().find(|(n, _)| n == im.own_name());
        match (&im.alias, clash) {
            (None, None) => Ok(()),
            (None, Some((name, files))) => Err(CompileError::new(
                format!(
                    "{} are named `{}`; when two or more imports in a file are named the \
                     same, every one of them is renamed -- write `import {} as {};`",
                    stdlib::join_and(files),
                    name,
                    im.text(),
                    derived_alias(im)
                ),
                im.span,
            )),
            (Some(a), None) => Err(CompileError::new(
                format!(
                    "`as` renames a module only when two or more imports in a file are named \
                     the same; `{}` is the only `{}` in this file, so it is imported as `{}`",
                    im.text(),
                    im.own_name(),
                    im.own_name()
                ),
                a.span,
            )),
            (Some(a), Some(_)) => {
                let want = derived_alias(im);
                if a.name != want {
                    return Err(CompileError::new(
                        format!(
                            "the rename of `{}` is `{}`, not `{}`: a rename is the directory \
                             and the file name joined, so it is not a choice either",
                            im.text(),
                            want,
                            a.name
                        ),
                        a.span,
                    ));
                }
                Ok(())
            }
        }
    }

    /// One binding per name per file, whichever kind of import claimed it.
    fn check_unbound(&self, name: &str, span: Span) -> CResult<()> {
        let sc = &self.files[self.cur];
        let taken = sc.std_imports.iter().any(|m| m == name)
            || sc.mod_imports.iter().any(|(b, _)| b == name);
        if taken {
            return Err(CompileError::new(
                format!("`{}` is already imported in this file", name),
                span,
            ));
        }
        Ok(())
    }

    // 1. struct names
    fn declare_structs(&mut self, f: &SourceFile) -> CResult<()> {
        for item in &f.items {
            if let Item::Struct(sd) = item {
                self.check_free(&sd.name, sd.span, "the name of a struct")?;
                if self.files[self.cur].struct_ids.contains_key(&sd.name) {
                    return Err(CompileError::new(
                        format!("struct `{}` is already defined", sd.name),
                        sd.span,
                    ));
                }
                let id = self.structs.len();
                self.files[self.cur].struct_ids.insert(sd.name.clone(), id);
                self.structs.push(StructInfo {
                    name: sd.name.clone(),
                    fields: Vec::new(),
                    size: 0,
                    laid_out: false,
                });
            }
        }
        Ok(())
    }

    // 2. field types
    fn declare_fields(&mut self, f: &SourceFile) -> CResult<()> {
        for item in &f.items {
            if let Item::Struct(sd) = item {
                let id = self.files[self.cur].struct_ids[&sd.name];
                let mut fields = Vec::new();
                for fl in &sd.fields {
                    if fields.iter().any(|x: &FieldInfo| x.name == fl.name) {
                        return Err(CompileError::new(
                            format!("duplicate field `{}` in struct `{}`", fl.name, sd.name),
                            fl.span,
                        ));
                    }
                    let ty = self.resolve_type(&fl.ty)?;
                    if ty == Type::Void {
                        return Err(CompileError::new(
                            "a field cannot have type `void`",
                            fl.ty.span(),
                        ));
                    }
                    fields.push(FieldInfo { name: fl.name.clone(), ty, offset: 0 });
                }
                self.structs[id].fields = fields;
            }
        }
        Ok(())
    }

    // 3. layouts (detects value-recursive structs)
    fn lay_out_structs(&mut self, f: &SourceFile) -> CResult<()> {
        for item in &f.items {
            if let Item::Struct(sd) = item {
                let id = self.files[self.cur].struct_ids[&sd.name];
                self.layout(id, sd.span, &mut vec![false; self.structs.len()])?;
            }
        }
        Ok(())
    }

    // 4. function signatures
    fn declare_fns(&mut self, f: &SourceFile) -> CResult<()> {
        for item in &f.items {
            if let Item::Fn(fd) = item {
                self.check_free(&fd.name, fd.span, "the name of a function")?;
                if self.files[self.cur].fn_ids.contains_key(&fd.name) {
                    return Err(CompileError::new(
                        format!("function `{}` is already defined", fd.name),
                        fd.span,
                    ));
                }
                if self.files[self.cur].struct_ids.contains_key(&fd.name) {
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
                let id = self.sigs.len();
                self.files[self.cur].fn_ids.insert(fd.name.clone(), id);
                self.sigs.push(FnSig { name: fd.name.clone(), params, ret, span: fd.span });
            }
        }
        Ok(())
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
                other => match self.files[self.cur].struct_ids.get(other) {
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
            TypeExpr::Map(mk, k, v, sp) => {
                let kt = self.resolve_type(k)?;
                let vt = self.resolve_type(v)?;
                if !kt.is_key() {
                    return Err(CompileError::new(
                        format!(
                            "a `{}` is keyed by value, so its key must be i32, i64, f64, bool or str, not `{}`",
                            mk.name(),
                            self.tn(&kt)
                        ),
                        *sp,
                    ));
                }
                if !vt.is_slot() {
                    return Err(CompileError::new(
                        format!(
                            "`{}` holds one-slot values and cannot hold `{}`; put it behind a pointer, or use `[{}; N]`",
                            mk.name(),
                            self.tn(&vt),
                            self.tn(&vt)
                        ),
                        *sp,
                    ));
                }
                Type::Map(*mk, Box::new(kt), Box::new(vt))
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

    /// An imported module owns its binding for the whole file: nothing else
    /// in it may be called `io` once `binz/io` is in scope. That is what lets
    /// `io.print` be read without checking whether `io` is a local struct.
    fn check_free(&self, name: &str, span: Span, what: &str) -> CResult<()> {
        let sc = &self.files[self.cur];
        let module = if sc.std_imports.iter().any(|m| m == name) {
            format!("binz/{}", name)
        } else if let Some((_, f)) = sc.mod_imports.iter().find(|(b, _)| b == name) {
            self.displays[*f].clone()
        } else {
            return Ok(());
        };
        Err(CompileError::new(
            format!("`{}` is the imported module `{}`, so it cannot also be {}", name, module, what),
            span,
        ))
    }

    fn declare(&mut self, name: &str, ty: Type, slot: u32, mutable: bool, span: Span) -> CResult<()> {
        self.check_free(name, span, "a variable")?;
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
                    match bt {
                        Type::Container(kind, elem) => {
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
                        // `m[key] = value` is the one way to put an entry in
                        // a map: it inserts when the key is new and
                        // overwrites when it is not.
                        Type::Map(_, kt, vt) => {
                            self.code.extend_from_slice(&scratch);
                            self.compile_key(index, &kt)?;
                            let got = self.compile_expr(value, Some(&vt))?;
                            self.expect_type(&vt, &got, value.span(), "in assignment")?;
                            self.emit(OP_SET);
                            self.next_slot = mark;
                            return Ok(());
                        }
                        _ => {}
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
                if let Some(m) = self.module_base(base)? {
                    let module = match m {
                        ModRef::Std(m) => format!("binz/{}", m),
                        ModRef::Local(f) => self.displays[f].clone(),
                    };
                    return Err(CompileError::new(
                        format!(
                            "`{}` is a function in `{}`, not a place that can be assigned to",
                            name, module
                        ),
                        *span,
                    ));
                }
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
                    Type::Map(mk, ..) => Err(CompileError::new(
                        format!(
                            "an entry of `{}<...>` lives on the heap and has no address; copy it into a variable first",
                            mk.name()
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
                if let Some(idx) = self.files[self.cur].fn_ids.get(name) {
                    let sig = self.sigs[*idx].clone();
                    self.emit_op_u32(OP_PUSH_FN, *idx as u32);
                    return Ok(Type::Fn(sig.params, Box::new(sig.ret)));
                }
                if let Some(e) = stdlib_hint(name, *span) {
                    return Err(e);
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

            Expr::Field { base, name, span } => {
                match self.module_base(base)? {
                    Some(ModRef::Std(m)) => return self.compile_module_value(&m, name, *span),
                    Some(ModRef::Local(f)) => return self.local_member(f, name, *span),
                    None => {}
                }
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
                    Type::Map(_, kt, vt) => {
                        self.compile_key(index, &kt)?;
                        self.emit(OP_GET);
                        Ok(*vt)
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

            Expr::MapLit { kind, key, val, entries, span } => {
                self.compile_map_lit(*kind, key, val, entries, *span)
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
        let id = match self.files[self.cur].struct_ids.get(name) {
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

    /// A map is subscripted by its key type, the one place where the thing
    /// inside `[...]` is not an `i32`.
    fn compile_key(&mut self, key: &Expr, kt: &Type) -> CResult<()> {
        let got = self.compile_expr(key, Some(kt))?;
        self.expect_type(kt, &got, key.span(), "in a key")?;
        Ok(())
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

    fn compile_map_lit(
        &mut self,
        kind: MapKind,
        key: &TypeExpr,
        val: &TypeExpr,
        entries: &[(Expr, Expr)],
        span: Span,
    ) -> CResult<Type> {
        let ty = self.resolve_type(&TypeExpr::Map(
            kind,
            Box::new(key.clone()),
            Box::new(val.clone()),
            span,
        ))?;
        let (kt, vt) = match &ty {
            Type::Map(_, k, v) => ((**k).clone(), (**v).clone()),
            _ => unreachable!(),
        };
        for (k, v) in entries {
            self.compile_key(k, &kt)?;
            let got = self.compile_expr(v, Some(&vt))?;
            self.expect_type(&vt, &got, v.span(), "in map literal")?;
        }
        self.emit(OP_NEW);
        self.emit(map_kind_code(kind));
        // Two stack values per entry: the key, then the value.
        self.emit_u32((entries.len() * 2) as u32);
        Ok(ty)
    }

    /// The six members of `binz/map`. They read the same for `HashMap` and
    /// `SortedMap`, which differ in one thing only: the order `map.keys`
    /// hands back.
    fn compile_map_form(&mut self, id: u8, args: &[Expr], kt: Type, ct: Type) -> CResult<Type> {
        if id == B_REMOVE || id == B_CONTAINS {
            self.compile_key(&args[1], &kt)?;
        }
        self.emit(OP_BUILTIN);
        self.emit(id);
        Ok(match id {
            B_SIZE => Type::I32,
            B_REMOVE | B_CONTAINS => Type::Bool,
            B_KEYS => Type::Container(Kind::Vector, Box::new(kt)),
            B_COPY => ct,
            _ => Type::Void,
        })
    }

    /// `int.abs` / `int.min` / `int.max`: generic over `i32` and `i64`, and
    /// answering in the width they were handed, so neither one is forced
    /// through a `cast` to use them.
    fn compile_int_form(&mut self, name: &str, id: u8, args: &[Expr], span: Span) -> CResult<Type> {
        let t = self.compile_expr(&args[0], Some(&Type::I32))?;
        if !t.is_integer() {
            let hint = if t == Type::F64 { format!("; an `f64` answers `float.{}`", name) } else { String::new() };
            return Err(CompileError::new(
                format!("`int.{}` needs an `i32` or an `i64`, found `{}`{}", name, self.tn(&t), hint),
                span,
            ));
        }
        if id != B_INT_ABS {
            let got = self.compile_expr(&args[1], Some(&t))?;
            self.expect_type(&t, &got, args[1].span(), "in argument")?;
        }
        self.emit(OP_BUILTIN);
        self.emit(id);
        Ok(t)
    }

    /// The generic half of the standard library: special forms that the
    /// compiler resolves against the type of their first argument, because
    /// binZ has no way yet to write that signature down.
    fn compile_form(
        &mut self,
        module: &str,
        member: &str,
        id: u8,
        arity: usize,
        args: &[Expr],
        span: Span,
    ) -> CResult<Type> {
        let name = format!("{}.{}", module, member);
        let name = name.as_str();
        if args.len() != arity {
            return Err(CompileError::new(
                format!("`{}` takes {} argument(s), found {}", name, arity, args.len()),
                span,
            ));
        }
        if module == "int" {
            return self.compile_int_form(member, id, args, span);
        }
        let ct = self.compile_expr(&args[0], None)?;

        // A map is not a container. It is reached only by key, and every
        // one of its operations lives in `binz/map`, so `binz/container`
        // hands it back with the line that does work -- the same split that
        // sends a `str` to `binz/string`.
        if let Type::Map(_, kt, _) = &ct {
            if module != "map" {
                return Err(CompileError::new(
                    format!(
                        "`{}` is not defined for `{}`; {}",
                        name,
                        self.tn(&ct),
                        map_instead(member)
                    ),
                    span,
                ));
            }
            let kt = (**kt).clone();
            return self.compile_map_form(id, args, kt, ct.clone());
        }
        if module == "map" {
            let hint = if stdlib::find_form("container", member).is_some() {
                format!("; a container answers `container.{}`", member)
            } else {
                String::new()
            };
            return Err(CompileError::new(
                format!("`{}` needs a map, found `{}`{}", name, self.tn(&ct), hint),
                span,
            ));
        }

        // A `str` is not a container: it answers `binz/string` instead, so
        // `size`, `find` and `contains` each have exactly one spelling per
        // type rather than one spelling covering both.
        if let Type::Str = ct {
            let hint = if stdlib::find_native("string", member).is_some() {
                format!("; a `str` answers `string.{}`", member)
            } else {
                "; a `str` is not a container".to_string()
            };
            return Err(CompileError::new(
                format!("`{}` needs a container, found `str`{}", name, hint),
                span,
            ));
        }

        let (elem, kind) = match &ct {
            Type::Array(elem, n) => {
                match id {
                    B_SIZE => {
                        // The size is part of the type; drop the base address.
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
                B_FIND => "use `container.contains`",
                B_PUSH => "use `container.add`",
                B_ERASE => "use `container.remove`",
                _ => "a set has no positions",
            };
            return Err(CompileError::new(
                format!("`{}` is not defined for `{}`; {}", name, self.tn(&ct), instead),
                span,
            ));
        }
        if wants_set && kind.is_sequence() {
            let instead = match id {
                B_ADD => "use `container.push`",
                B_REMOVE => "use `container.erase`",
                _ => "use `container.find`",
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
            B_SIZE | B_FIND => Type::I32,
            B_POP | B_ERASE => elem,
            B_ADD | B_REMOVE | B_CONTAINS => Type::Bool,
            B_COPY => ct,
            _ => Type::Void,
        })
    }

    /// `io` or `math`, when `base` is the bare name of a module imported by
    /// this file. Returns an error instead when it names a standard library
    /// module the file forgot to import.
    fn module_base(&self, base: &Expr) -> CResult<Option<ModRef>> {
        let (name, span) = match base {
            Expr::Ident(n, sp) => (n, *sp),
            _ => return Ok(None),
        };
        if self.lookup(name).is_some() {
            return Ok(None);
        }
        let sc = &self.files[self.cur];
        if sc.std_imports.iter().any(|m| m == name) {
            return Ok(Some(ModRef::Std(name.clone())));
        }
        if let Some((_, f)) = sc.mod_imports.iter().find(|(b, _)| b == name) {
            return Ok(Some(ModRef::Local(*f)));
        }
        if stdlib::is_module(name) {
            return Err(CompileError::new(
                format!(
                    "`{}` is not imported; add `import binz/{};` at the top of the file",
                    name, name
                ),
                span,
            ));
        }
        Ok(None)
    }

    /// `math.add`, where `math` is another file of this project. A module
    /// exports every function it defines and nothing else, so this is just a
    /// lookup in that file's own scope -- and the result is an ordinary
    /// function value, exactly as a bare `add` would be inside `math.binz`.
    fn local_member(&mut self, file: usize, name: &str, span: Span) -> CResult<Type> {
        let binding = self.files[self.cur]
            .mod_imports
            .iter()
            .find(|(_, f)| *f == file)
            .map(|(b, _)| b.clone())
            .unwrap_or_default();
        let idx = match self.files[file].fn_ids.get(name) {
            Some(i) => *i,
            None => {
                let what = if self.files[file].struct_ids.contains_key(name) {
                    format!(
                        "`{}` is a struct in `{}`, and a module exports its functions, not its types",
                        name, self.displays[file]
                    )
                } else {
                    format!("`{}` has no function `{}`", self.displays[file], name)
                };
                return Err(CompileError::new(what, span));
            }
        };
        let sig = self.sigs[idx].clone();
        // A struct belongs to the file that declares it, so a signature
        // mentioning one cannot be written down here -- there is no way to
        // name the type, and no way to pass a value of it.
        for t in sig.params.iter().chain(std::iter::once(&sig.ret)) {
            if let Some(id) = struct_in(t) {
                return Err(CompileError::new(
                    format!(
                        "`{}.{}` is typed with the struct `{}`, which `{}` does not export; \
                         a module exports its functions, not its types",
                        binding,
                        name,
                        self.structs[id].name,
                        self.displays[file]
                    ),
                    span,
                ));
            }
        }
        self.emit_op_u32(OP_PUSH_FN, idx as u32);
        Ok(Type::Fn(sig.params, Box::new(sig.ret)))
    }

    fn no_member(&self, module: &str, name: &str, span: Span) -> CompileError {
        // A verb the module refuses on purpose gets the line to write
        // instead, rather than a pointer at a module that would refuse it
        // too.
        if let Some(instead) = stdlib::misused(module, name) {
            return CompileError::new(
                format!("`binz/{}` has no `{}`: {}", module, name, instead),
                span,
            );
        }
        let elsewhere: Vec<&str> = stdlib::modules_defining(name)
            .into_iter()
            .filter(|m| *m != module)
            .collect();
        let hint = if elsewhere.is_empty() {
            String::new()
        } else {
            format!("; it is in {}", stdlib::describe_modules(&elsewhere))
        };
        CompileError::new(
            format!("`binz/{}` has no `{}`{}", module, name, hint),
            span,
        )
    }

    /// A stdlib member used as a value. It is one exactly when its type can
    /// be written down in binZ; the generic ones have to be called.
    fn compile_module_value(&mut self, module: &str, name: &str, span: Span) -> CResult<Type> {
        if let Some((idx, ty)) = stdlib::find_native(module, name) {
            self.emit_op_u32(OP_PUSH_NATIVE, idx);
            return Ok(ty);
        }
        if stdlib::find_form(module, name).is_some() {
            return Err(CompileError::new(
                format!(
                    "`{}.{}` is generic over the type it is given, which binZ cannot write in a signature, so it is not a value; call it directly",
                    module, name
                ),
                span,
            ));
        }
        Err(self.no_member(module, name, span))
    }

    fn compile_module_call(
        &mut self,
        module: &str,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> CResult<Type> {
        if let Some(form) = stdlib::find_form(module, name) {
            return self.compile_form(module, name, form.id, form.arity, args, span);
        }
        if let Some((idx, ty)) = stdlib::find_native(module, name) {
            self.emit_op_u32(OP_PUSH_NATIVE, idx);
            return self.finish_call(ty, args, span);
        }
        Err(self.no_member(module, name, span))
    }

    fn compile_call(&mut self, callee: &Expr, args: &[Expr], span: Span) -> CResult<Type> {
        if let Expr::Field { base, name, .. } = callee {
            match self.module_base(base)? {
                Some(ModRef::Std(m)) => return self.compile_module_call(&m, name, args, span),
                Some(ModRef::Local(f)) => {
                    // A local module's member is a plain function value, so
                    // the call goes down the ordinary path from here.
                    let ct = self.local_member(f, name, span)?;
                    return self.finish_call(ct, args, span);
                }
                None => {}
            }
        }
        let ct = self.compile_expr(callee, None)?;
        self.finish_call(ct, args, span)
    }

    fn finish_call(&mut self, ct: Type, args: &[Expr], span: Span) -> CResult<Type> {
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

/// What `binz/container` answers when it is handed a map: every map
/// operation lives in `binz/map`, so this always names a line that works.
fn map_instead(member: &str) -> String {
    match member {
        "size" | "contains" | "remove" | "clear" | "copy" => {
            format!("a map answers `map.{}`", member)
        }
        "find" => "a map is keyed by value; use `map.contains`".into(),
        "push" | "add" | "insert" => "write `m[key] = value`".into(),
        "erase" => "a map has no positions; use `map.remove`".into(),
        "pop" => "a map has no positions".into(),
        _ => "every map operation lives in `binz/map`".into(),
    }
}

fn map_kind_code(mk: MapKind) -> u8 {
    match mk {
        MapKind::Hash => KIND_MAP,
        MapKind::Sorted => KIND_SORTED_MAP,
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
/// The one spelling an `as` rename may have: the module's directory and its
/// own name, joined the way binZ joins words everywhere else. `text/format`
/// is `textFormat`. A file directly under the anchor uses `root`.
///
/// Two contested imports always differ in the directory (two files of the
/// same name in one directory *are* one file), so this is unique without
/// having to look at what else the file imports -- adding an import can never
/// change the rename another one has to use.
fn derived_alias(im: &ImportDef) -> String {
    let n = im.path.len();
    let parent = if n >= 2 { im.path[n - 2].as_str() } else { "root" };
    let name = im.own_name();
    format!("{}{}{}", parent, name[..1].to_ascii_uppercase(), &name[1..])
}

/// The module names this file claims more than once, each with the imports
/// that claim it. Only these may be renamed with `as`, and each of them must
/// be. Two imports of the *same* file are a duplicate, not a clash, so the
/// files are counted distinctly.
fn contested_names(f: &SourceFile) -> Vec<(String, Vec<String>)> {
    let mut claims: Vec<(String, Vec<usize>, Vec<String>)> = Vec::new();
    let mut dep = 0;
    for item in &f.items {
        let im = match item {
            Item::Import(im) if im.local => im,
            _ => continue,
        };
        let target = f.deps[dep];
        dep += 1;
        match claims.iter_mut().find(|(n, _, _)| n == im.own_name()) {
            Some((_, targets, texts)) => {
                if !targets.contains(&target) {
                    targets.push(target);
                    texts.push(format!("`{}`", im.text()));
                }
            }
            None => claims.push((im.own_name().to_string(), vec![target], vec![format!("`{}`", im.text())])),
        }
    }
    claims
        .into_iter()
        .filter(|(_, targets, _)| targets.len() > 1)
        .map(|(n, _, texts)| (n, texts))
        .collect()
}

/// The `main` of a file, if it has one.
fn fn_named<'a>(f: &'a SourceFile, name: &str) -> Option<&'a FnDef> {
    f.items.iter().find_map(|i| match i {
        Item::Fn(fd) if fd.name == name => Some(fd),
        _ => None,
    })
}

/// The first struct a type mentions, however deeply. Struct ids are per file,
/// so this is what decides whether a signature can cross a module boundary.
fn struct_in(t: &Type) -> Option<usize> {
    match t {
        Type::Struct(id) => Some(*id),
        Type::Ptr(inner) | Type::Array(inner, _) | Type::Container(_, inner) => struct_in(inner),
        Type::Map(_, k, v) => struct_in(k).or_else(|| struct_in(v)),
        Type::Fn(ps, r) => ps.iter().find_map(struct_in).or_else(|| struct_in(r)),
        _ => None,
    }
}

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

/// Turns a bare `print(...)` or `len(...)` -- how binZ was written before the
/// standard library had modules -- into the import and the spelling that
/// replace it.
fn stdlib_hint(name: &str, span: Span) -> Option<CompileError> {
    let renamed = stdlib::renamed_to(name);
    let target = renamed.unwrap_or(name);
    let mods = stdlib::modules_defining(target);
    if mods.is_empty() {
        return None;
    }
    let calls: Vec<String> = mods.iter().map(|m| format!("`{}.{}`", m, target)).collect();
    let was = match renamed {
        Some(new) => format!("`{}` is now `{}`, in {}", name, new, stdlib::describe_modules(&mods)),
        None => format!("`{}` is in {}", name, stdlib::describe_modules(&mods)),
    };
    let imports: Vec<String> = mods.iter().map(|m| format!("import binz/{};", m)).collect();
    Some(CompileError::new(
        format!("{}; write {} after `{}`", was, stdlib::join_or(&calls), imports.join(" ")),
        span,
    ))
}
