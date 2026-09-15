//! The binZ bytecode artifact (`.bzc`) and its instruction set.
//!
//! Layout is little-endian throughout:
//!   magic "BINZ" | version u32 | strings | functions | entry u32

pub const MAGIC: &[u8; 4] = b"BINZ";
pub const VERSION: u32 = 3;

pub const OP_PUSH_I32: u8 = 0x01;
pub const OP_PUSH_I64: u8 = 0x02;
pub const OP_PUSH_F64: u8 = 0x03;
pub const OP_PUSH_BOOL: u8 = 0x04;
pub const OP_PUSH_STR: u8 = 0x05;
pub const OP_PUSH_FN: u8 = 0x06;
pub const OP_PUSH_NATIVE: u8 = 0x07;
pub const OP_PUSH_VOID: u8 = 0x08;

pub const OP_LOAD_LOCAL: u8 = 0x10;
pub const OP_STORE_LOCAL: u8 = 0x11;
pub const OP_ADDR_LOCAL: u8 = 0x12;
pub const OP_LOAD_PTR: u8 = 0x13;
pub const OP_STORE_PTR: u8 = 0x14;
pub const OP_FIELD: u8 = 0x15;
pub const OP_COPY: u8 = 0x16;
pub const OP_COPY_SRET: u8 = 0x17;
/// `[elem_size: u32, len: u32]` -- pops an index and a base address, pushes
/// the address of that element after a bounds check.
pub const OP_ELEM: u8 = 0x18;
/// `[len: u32]` -- linear search of a fixed array of one-slot elements.
pub const OP_ARR_FIND: u8 = 0x19;

pub const OP_ADD: u8 = 0x20;
pub const OP_SUB: u8 = 0x21;
pub const OP_MUL: u8 = 0x22;
pub const OP_DIV: u8 = 0x23;
pub const OP_REM: u8 = 0x24;
pub const OP_NEG: u8 = 0x25;

pub const OP_EQ: u8 = 0x30;
pub const OP_NE: u8 = 0x31;
pub const OP_LT: u8 = 0x32;
pub const OP_LE: u8 = 0x33;
pub const OP_GT: u8 = 0x34;
pub const OP_GE: u8 = 0x35;
pub const OP_NOT: u8 = 0x36;

pub const OP_JMP: u8 = 0x40;
pub const OP_JMP_IF_FALSE: u8 = 0x41;
pub const OP_JMP_IF_TRUE: u8 = 0x42;
pub const OP_POP: u8 = 0x43;

pub const OP_CALL: u8 = 0x50;
pub const OP_RET: u8 = 0x51;

pub const OP_CAST: u8 = 0x60;

/// `[kind: u8, count: u32]` -- pops `count` values, pushes a new container.
/// A map pushes two values per entry, so `count` is twice its length.
pub const OP_NEW: u8 = 0x70;
/// Pops an index and a container, pushes the element.
pub const OP_GET: u8 = 0x71;
/// Pops a value, an index and a container, and stores.
pub const OP_SET: u8 = 0x72;
/// `[builtin: u8]` -- pops that builtin's arguments, pushes its result.
pub const OP_BUILTIN: u8 = 0x73;

pub const KIND_VECTOR: u8 = 0;
pub const KIND_LIST: u8 = 1;
pub const KIND_SET: u8 = 2;
pub const KIND_SORTED_SET: u8 = 3;
pub const KIND_MAP: u8 = 4;
pub const KIND_SORTED_MAP: u8 = 5;

pub const B_SIZE: u8 = 0;
pub const B_FIND: u8 = 1;
pub const B_PUSH: u8 = 2;
pub const B_POP: u8 = 3;
pub const B_INSERT: u8 = 4;
pub const B_ERASE: u8 = 5;
pub const B_ADD: u8 = 6;
pub const B_REMOVE: u8 = 7;
pub const B_CONTAINS: u8 = 8;
pub const B_CLEAR: u8 = 9;
pub const B_COPY: u8 = 10;
pub const B_KEYS: u8 = 11;
pub const B_INT_ABS: u8 = 12;
pub const B_INT_MIN: u8 = 13;
pub const B_INT_MAX: u8 = 14;

pub fn builtin_name(id: u8) -> &'static str {
    crate::stdlib::form_name(id)
}

pub const CAST_I32: u8 = 0;
pub const CAST_I64: u8 = 1;
pub const CAST_F64: u8 = 2;
pub const CAST_STR: u8 = 3;

#[derive(Debug, Clone)]
pub struct FnMeta {
    pub name: String,
    /// Slot footprint of each parameter (1 for scalars, N for structs passed
    /// by value). Their frame offsets are the running sum.
    pub param_sizes: Vec<u32>,
    /// Total frame size: locals + struct temporaries.
    pub n_slots: u32,
    /// True when the function returns a struct by value and therefore takes a
    /// hidden destination address as its first argument.
    pub sret: bool,
    pub ret_size: u32,
    pub code: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Module {
    pub strings: Vec<String>,
    pub funcs: Vec<FnMeta>,
    pub entry: u32,
}

// ------------------------------------------------------------- serialization

struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.buf.extend_from_slice(s.as_bytes());
    }
}

pub fn serialize(m: &Module) -> Vec<u8> {
    let mut w = Writer { buf: Vec::new() };
    w.buf.extend_from_slice(MAGIC);
    w.u32(VERSION);
    w.u32(m.strings.len() as u32);
    for s in &m.strings {
        w.str(s);
    }
    w.u32(m.funcs.len() as u32);
    for f in &m.funcs {
        w.str(&f.name);
        w.u32(f.param_sizes.len() as u32);
        for p in &f.param_sizes {
            w.u32(*p);
        }
        w.u32(f.n_slots);
        w.u8(if f.sret { 1 } else { 0 });
        w.u32(f.ret_size);
        w.u32(f.code.len() as u32);
        w.buf.extend_from_slice(&f.code);
    }
    w.u32(m.entry);
    w.buf
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.pos + n > self.buf.len() {
            return Err("truncated bytecode artifact".into());
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn str(&mut self) -> Result<String, String> {
        let n = self.u32()? as usize;
        let b = self.take(n)?;
        String::from_utf8(b.to_vec()).map_err(|_| "invalid utf-8 in artifact".to_string())
    }
}

pub fn deserialize(buf: &[u8]) -> Result<Module, String> {
    let mut r = Reader { buf, pos: 0 };
    if r.take(4)? != MAGIC {
        return Err("not a binZ artifact (bad magic)".into());
    }
    let version = r.u32()?;
    if version != VERSION {
        return Err(format!("unsupported artifact version {} (expected {})", version, VERSION));
    }
    let n_strings = r.u32()? as usize;
    let mut strings = Vec::with_capacity(n_strings);
    for _ in 0..n_strings {
        strings.push(r.str()?);
    }
    let n_funcs = r.u32()? as usize;
    let mut funcs = Vec::with_capacity(n_funcs);
    for _ in 0..n_funcs {
        let name = r.str()?;
        let n_params = r.u32()? as usize;
        let mut param_sizes = Vec::with_capacity(n_params);
        for _ in 0..n_params {
            param_sizes.push(r.u32()?);
        }
        let n_slots = r.u32()?;
        let sret = r.u8()? != 0;
        let ret_size = r.u32()?;
        let code_len = r.u32()? as usize;
        let code = r.take(code_len)?.to_vec();
        funcs.push(FnMeta { name, param_sizes, n_slots, sret, ret_size, code });
    }
    let entry = r.u32()?;
    Ok(Module { strings, funcs, entry })
}

// ------------------------------------------------------------ disassembler

fn rd_u32(code: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([code[at], code[at + 1], code[at + 2], code[at + 3]])
}

pub fn disassemble(m: &Module) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "; binZ artifact v{}  ({} functions, {} strings, entry = {})\n",
        VERSION,
        m.funcs.len(),
        m.strings.len(),
        m.funcs.get(m.entry as usize).map(|f| f.name.as_str()).unwrap_or("?")
    ));
    for (i, s) in m.strings.iter().enumerate() {
        out.push_str(&format!("; str[{}] = {:?}\n", i, s));
    }
    for (fi, f) in m.funcs.iter().enumerate() {
        out.push_str(&format!(
            "\nfunction #{} {} (params: {:?}, slots: {}, sret: {}, ret_size: {})\n",
            fi, f.name, f.param_sizes, f.n_slots, f.sret, f.ret_size
        ));
        let code = &f.code;
        let mut pc = 0usize;
        while pc < code.len() {
            let at = pc;
            let op = code[pc];
            pc += 1;
            let text = match op {
                OP_PUSH_I32 => {
                    let v = rd_u32(code, pc) as i32;
                    pc += 4;
                    format!("push.i32 {}", v)
                }
                OP_PUSH_I64 => {
                    let b = &code[pc..pc + 8];
                    let v = i64::from_le_bytes(b.try_into().unwrap());
                    pc += 8;
                    format!("push.i64 {}", v)
                }
                OP_PUSH_F64 => {
                    let b = &code[pc..pc + 8];
                    let v = f64::from_le_bytes(b.try_into().unwrap());
                    pc += 8;
                    format!("push.f64 {}", v)
                }
                OP_PUSH_BOOL => {
                    let v = code[pc];
                    pc += 1;
                    format!("push.bool {}", v != 0)
                }
                OP_PUSH_STR => {
                    let v = rd_u32(code, pc);
                    pc += 4;
                    format!("push.str {} ; {:?}", v, m.strings[v as usize])
                }
                OP_PUSH_FN => {
                    let v = rd_u32(code, pc);
                    pc += 4;
                    format!("push.fn {} ; {}", v, m.funcs[v as usize].name)
                }
                OP_PUSH_NATIVE => {
                    let v = rd_u32(code, pc);
                    pc += 4;
                    format!("push.native {} ; {}", v, crate::stdlib::native_name(v))
                }
                OP_PUSH_VOID => "push.void".to_string(),
                OP_LOAD_LOCAL | OP_STORE_LOCAL | OP_ADDR_LOCAL | OP_FIELD | OP_COPY
                | OP_COPY_SRET | OP_CALL => {
                    let v = rd_u32(code, pc);
                    pc += 4;
                    let name = match op {
                        OP_LOAD_LOCAL => "load.local",
                        OP_STORE_LOCAL => "store.local",
                        OP_ADDR_LOCAL => "addr.local",
                        OP_FIELD => "field",
                        OP_COPY => "copy",
                        OP_COPY_SRET => "copy.sret",
                        _ => "call",
                    };
                    format!("{} {}", name, v)
                }
                OP_JMP | OP_JMP_IF_FALSE | OP_JMP_IF_TRUE => {
                    let rel = rd_u32(code, pc) as i32;
                    pc += 4;
                    let name = match op {
                        OP_JMP => "jmp",
                        OP_JMP_IF_FALSE => "jmp.false",
                        _ => "jmp.true",
                    };
                    format!("{} {} ; -> {}", name, rel, (pc as i64 + rel as i64))
                }
                OP_CAST => {
                    let k = code[pc];
                    pc += 1;
                    let name = match k {
                        CAST_I32 => "i32",
                        CAST_I64 => "i64",
                        CAST_F64 => "f64",
                        _ => "str",
                    };
                    format!("cast {}", name)
                }
                OP_ELEM => {
                    let size = rd_u32(code, pc);
                    let len = rd_u32(code, pc + 4);
                    pc += 8;
                    format!("elem {} {}", size, len)
                }
                OP_ARR_FIND => {
                    let len = rd_u32(code, pc);
                    pc += 4;
                    format!("arr.find {}", len)
                }
                OP_NEW => {
                    let kind = code[pc];
                    let count = rd_u32(code, pc + 1);
                    pc += 5;
                    let name = match kind {
                        KIND_VECTOR => "Vector",
                        KIND_LIST => "LinkedList",
                        KIND_SET => "Set",
                        KIND_SORTED_SET => "SortedSet",
                        KIND_MAP => "HashMap",
                        _ => "SortedMap",
                    };
                    format!("new {} {}", name, count)
                }
                OP_BUILTIN => {
                    let id = code[pc];
                    pc += 1;
                    format!("builtin {}", builtin_name(id))
                }
                OP_GET => "get".into(),
                OP_SET => "set".into(),
                OP_LOAD_PTR => "load.ptr".into(),
                OP_STORE_PTR => "store.ptr".into(),
                OP_ADD => "add".into(),
                OP_SUB => "sub".into(),
                OP_MUL => "mul".into(),
                OP_DIV => "div".into(),
                OP_REM => "rem".into(),
                OP_NEG => "neg".into(),
                OP_EQ => "eq".into(),
                OP_NE => "ne".into(),
                OP_LT => "lt".into(),
                OP_LE => "le".into(),
                OP_GT => "gt".into(),
                OP_GE => "ge".into(),
                OP_NOT => "not".into(),
                OP_POP => "pop".into(),
                OP_RET => "ret".into(),
                other => format!("<unknown 0x{:02x}>", other),
            };
            out.push_str(&format!("  {:>5}  {}\n", at, text));
        }
    }
    out
}
