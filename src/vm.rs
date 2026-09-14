//! Stack virtual machine for binZ bytecode.
//!
//! Two stacks: `mem` holds call frames and is byte-addressable at slot
//! granularity (that is what a `*T` points at), `stack` holds operands.

use std::rc::Rc;

use crate::bytecode::*;
use crate::obj::{self, Handle};

#[derive(Debug, Clone)]
pub enum Value {
    I32(i32),
    I64(i64),
    F64(f64),
    Bool(bool),
    Str(Rc<String>),
    Ptr(usize),
    Fn(u32),
    Native(u32),
    /// A handle to a `Vector` / `LinkedList` / `Set` / `SortedSet` /
    /// `HashMap`.
    Obj(Handle),
    Void,
}

#[derive(Debug)]
pub struct RuntimeError {
    pub msg: String,
    pub func: String,
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "runtime error in `{}`: {}", self.func, self.msg)
    }
}

struct Frame {
    func: usize,
    pc: usize,
    fp: usize,
    sret: usize,
}

pub fn format_f64(v: f64) -> String {
    let s = format!("{}", v);
    if v.is_finite() && !s.contains('.') && !s.contains('e') && !s.contains("NaN") {
        format!("{}.0", s)
    } else {
        s
    }
}

pub struct Vm {
    mem: Vec<Value>,
    stack: Vec<Value>,
    frames: Vec<Frame>,
}

macro_rules! rt {
    ($vm:expr, $m:expr, $($arg:tt)*) => {
        return Err(RuntimeError {
            msg: format!($($arg)*),
            func: $vm
                .frames
                .last()
                .map(|f| $m.funcs[f.func].name.clone())
                .unwrap_or_else(|| "?".to_string()),
        })
    };
}

impl Vm {
    pub fn run(m: &Module) -> Result<i32, RuntimeError> {
        let mut vm = Vm { mem: Vec::new(), stack: Vec::new(), frames: Vec::new() };
        let entry = m.entry as usize;
        vm.mem.resize(m.funcs[entry].n_slots as usize, Value::Void);
        vm.frames.push(Frame { func: entry, pc: 0, fp: 0, sret: 0 });
        vm.exec(m)
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().unwrap_or(Value::Void)
    }

    fn exec(&mut self, m: &Module) -> Result<i32, RuntimeError> {
        loop {
            let (fi, mut pc) = {
                let f = self.frames.last().unwrap();
                (f.func, f.pc)
            };
            let code = &m.funcs[fi].code;
            if pc >= code.len() {
                rt!(self, m, "fell off the end of the function");
            }
            let op = code[pc];
            pc += 1;

            let mut u32_operand = 0u32;
            let mut u32_second = 0u32;
            let mut u8_operand = 0u8;
            match op {
                OP_PUSH_I32 | OP_PUSH_STR | OP_PUSH_FN | OP_PUSH_NATIVE | OP_LOAD_LOCAL
                | OP_STORE_LOCAL | OP_ADDR_LOCAL | OP_FIELD | OP_COPY | OP_COPY_SRET | OP_CALL
                | OP_JMP | OP_JMP_IF_FALSE | OP_JMP_IF_TRUE | OP_ARR_FIND => {
                    u32_operand =
                        u32::from_le_bytes([code[pc], code[pc + 1], code[pc + 2], code[pc + 3]]);
                    pc += 4;
                }
                OP_ELEM => {
                    u32_operand =
                        u32::from_le_bytes([code[pc], code[pc + 1], code[pc + 2], code[pc + 3]]);
                    u32_second = u32::from_le_bytes([
                        code[pc + 4],
                        code[pc + 5],
                        code[pc + 6],
                        code[pc + 7],
                    ]);
                    pc += 8;
                }
                OP_NEW => {
                    u8_operand = code[pc];
                    u32_operand = u32::from_le_bytes([
                        code[pc + 1],
                        code[pc + 2],
                        code[pc + 3],
                        code[pc + 4],
                    ]);
                    pc += 5;
                }
                OP_PUSH_BOOL | OP_CAST | OP_BUILTIN => {
                    u8_operand = code[pc];
                    pc += 1;
                }
                OP_PUSH_I64 | OP_PUSH_F64 => {}
                _ => {}
            }

            match op {
                OP_PUSH_I32 => self.stack.push(Value::I32(u32_operand as i32)),
                OP_PUSH_I64 => {
                    let v = i64::from_le_bytes(code[pc..pc + 8].try_into().unwrap());
                    pc += 8;
                    self.stack.push(Value::I64(v));
                }
                OP_PUSH_F64 => {
                    let v = f64::from_le_bytes(code[pc..pc + 8].try_into().unwrap());
                    pc += 8;
                    self.stack.push(Value::F64(v));
                }
                OP_PUSH_BOOL => self.stack.push(Value::Bool(u8_operand != 0)),
                OP_PUSH_STR => self
                    .stack
                    .push(Value::Str(Rc::new(m.strings[u32_operand as usize].clone()))),
                OP_PUSH_FN => self.stack.push(Value::Fn(u32_operand)),
                OP_PUSH_NATIVE => self.stack.push(Value::Native(u32_operand)),
                OP_PUSH_VOID => self.stack.push(Value::Void),

                OP_LOAD_LOCAL => {
                    let fp = self.frames.last().unwrap().fp;
                    self.stack.push(self.mem[fp + u32_operand as usize].clone());
                }
                OP_STORE_LOCAL => {
                    let fp = self.frames.last().unwrap().fp;
                    let v = self.pop();
                    self.mem[fp + u32_operand as usize] = v;
                }
                OP_ADDR_LOCAL => {
                    let fp = self.frames.last().unwrap().fp;
                    self.stack.push(Value::Ptr(fp + u32_operand as usize));
                }
                OP_LOAD_PTR => {
                    let a = self.as_ptr(self.stack.last().cloned().unwrap_or(Value::Void), m)?;
                    self.pop();
                    self.stack.push(self.mem[a].clone());
                }
                OP_STORE_PTR => {
                    let v = self.pop();
                    let p = self.pop();
                    let a = self.as_ptr(p, m)?;
                    self.mem[a] = v;
                }
                OP_FIELD => {
                    let p = self.pop();
                    let a = self.as_ptr(p, m)?;
                    self.stack.push(Value::Ptr(a + u32_operand as usize));
                }
                OP_COPY => {
                    let src = self.pop();
                    let dst = self.pop();
                    let (s, d) = (self.as_ptr(src, m)?, self.as_ptr(dst, m)?);
                    for k in 0..u32_operand as usize {
                        self.mem[d + k] = self.mem[s + k].clone();
                    }
                }
                OP_COPY_SRET => {
                    let src = self.pop();
                    let s = self.as_ptr(src, m)?;
                    let d = self.frames.last().unwrap().sret;
                    for k in 0..u32_operand as usize {
                        self.mem[d + k] = self.mem[s + k].clone();
                    }
                    self.stack.push(Value::Ptr(d));
                }

                OP_ELEM => {
                    let idx = self.pop();
                    let base = self.pop();
                    let i = self.index_of(&idx, m)?;
                    let len = u32_second as usize;
                    if i >= len {
                        rt!(self, m, "index {} is out of range for an array of length {}", i, len);
                    }
                    let a = self.as_ptr(base, m)?;
                    self.stack.push(Value::Ptr(a + i * u32_operand as usize));
                }
                OP_ARR_FIND => {
                    let needle = self.pop();
                    let base = self.pop();
                    let a = self.as_ptr(base, m)?;
                    let mut found = -1i32;
                    for k in 0..u32_operand as usize {
                        if obj::value_eq(&self.mem[a + k], &needle) {
                            found = k as i32;
                            break;
                        }
                    }
                    self.stack.push(Value::I32(found));
                }
                OP_NEW => {
                    let at = self.stack.len() - u32_operand as usize;
                    let values: Vec<Value> = self.stack.split_off(at);
                    let v = self.checked(obj::new_container(u8_operand, values), m)?;
                    self.stack.push(v);
                }
                // `c[i]` and `m[key]` share one opcode: what is on the stack
                // is a position for a sequence and a key for a map.
                OP_GET => {
                    let idx = self.pop();
                    let c = self.pop();
                    let h = self.as_obj(c, m)?;
                    let v = if obj::is_map(&h) {
                        self.checked(obj::map_get(&h, &idx), m)?
                    } else {
                        let i = self.index_of(&idx, m)?;
                        self.checked(obj::obj_get(&h, i), m)?
                    };
                    self.stack.push(v);
                }
                OP_SET => {
                    let v = self.pop();
                    let idx = self.pop();
                    let c = self.pop();
                    let h = self.as_obj(c, m)?;
                    if obj::is_map(&h) {
                        self.checked(obj::map_set(&h, idx, v), m)?;
                    } else {
                        let i = self.index_of(&idx, m)?;
                        self.checked(obj::obj_set(&h, i, v), m)?;
                    }
                }
                OP_BUILTIN => {
                    let v = self.builtin(u8_operand, m)?;
                    self.stack.push(v);
                }

                OP_ADD | OP_SUB | OP_MUL | OP_DIV | OP_REM => {
                    let b = self.pop();
                    let a = self.pop();
                    let v = self.arith(op, a, b, m)?;
                    self.stack.push(v);
                }
                OP_NEG => {
                    let a = self.pop();
                    let v = match a {
                        Value::I32(x) => Value::I32(x.wrapping_neg()),
                        Value::I64(x) => Value::I64(x.wrapping_neg()),
                        Value::F64(x) => Value::F64(-x),
                        _ => rt!(self, m, "`-` applied to a non-numeric value"),
                    };
                    self.stack.push(v);
                }
                OP_EQ | OP_NE | OP_LT | OP_LE | OP_GT | OP_GE => {
                    let b = self.pop();
                    let a = self.pop();
                    let v = self.compare(op, a, b, m)?;
                    self.stack.push(Value::Bool(v));
                }
                OP_NOT => {
                    let a = self.pop();
                    match a {
                        Value::Bool(x) => self.stack.push(Value::Bool(!x)),
                        _ => rt!(self, m, "`!` applied to a non-boolean value"),
                    }
                }

                OP_JMP => {
                    pc = (pc as i64 + u32_operand as i32 as i64) as usize;
                }
                OP_JMP_IF_FALSE | OP_JMP_IF_TRUE => {
                    let c = match self.pop() {
                        Value::Bool(b) => b,
                        _ => rt!(self, m, "branch on a non-boolean value"),
                    };
                    let take = if op == OP_JMP_IF_FALSE { !c } else { c };
                    if take {
                        pc = (pc as i64 + u32_operand as i32 as i64) as usize;
                    }
                }
                OP_POP => {
                    self.pop();
                }

                OP_CAST => {
                    let a = self.pop();
                    let v = self.cast(u8_operand, a, m)?;
                    self.stack.push(v);
                }

                OP_CALL => {
                    self.frames.last_mut().unwrap().pc = pc;
                    let argc = u32_operand as usize;
                    let at = self.stack.len() - argc;
                    let args: Vec<Value> = self.stack.split_off(at);
                    let callee = self.pop();
                    match callee {
                        Value::Native(idx) => {
                            let v = self.call_native(idx, &args, m)?;
                            self.stack.push(v);
                        }
                        Value::Fn(idx) => {
                            let meta = &m.funcs[idx as usize];
                            let fp = self.mem.len();
                            self.mem.resize(fp + meta.n_slots as usize, Value::Void);
                            let mut ai = 0usize;
                            let mut sret = 0usize;
                            if meta.sret {
                                sret = self.as_ptr(args[0].clone(), m)?;
                                ai = 1;
                            }
                            let mut slot = 0usize;
                            for (i, size) in meta.param_sizes.iter().enumerate() {
                                let v = args[ai + i].clone();
                                if *size == 1 {
                                    self.mem[fp + slot] = v;
                                } else {
                                    let s = self.as_ptr(v, m)?;
                                    for k in 0..*size as usize {
                                        self.mem[fp + slot + k] = self.mem[s + k].clone();
                                    }
                                }
                                slot += *size as usize;
                            }
                            self.frames.push(Frame { func: idx as usize, pc: 0, fp, sret });
                        }
                        _ => rt!(self, m, "attempted to call a non-function value"),
                    }
                    continue;
                }

                OP_RET => {
                    let v = self.pop();
                    let frame = self.frames.pop().unwrap();
                    self.mem.truncate(frame.fp);
                    if self.frames.is_empty() {
                        return Ok(match v {
                            Value::I32(x) => x,
                            _ => 0,
                        });
                    }
                    self.stack.push(v);
                    continue;
                }

                other => rt!(self, m, "unknown opcode 0x{:02x}", other),
            }

            self.frames.last_mut().unwrap().pc = pc;
        }
    }

    /// Wraps a container-layer failure as a runtime error in the current
    /// function.
    fn checked<T>(&self, r: Result<T, String>, m: &Module) -> Result<T, RuntimeError> {
        match r {
            Ok(v) => Ok(v),
            Err(msg) => rt!(self, m, "{}", msg),
        }
    }

    fn index_of(&self, v: &Value, m: &Module) -> Result<usize, RuntimeError> {
        match v {
            Value::I32(i) if *i >= 0 => Ok(*i as usize),
            Value::I32(i) => rt!(self, m, "index {} is negative", i),
            _ => rt!(self, m, "an index must be an i32"),
        }
    }

    fn as_obj(&self, v: Value, m: &Module) -> Result<Handle, RuntimeError> {
        match v {
            Value::Obj(h) => Ok(h),
            _ => rt!(self, m, "expected a container"),
        }
    }

    fn builtin(&mut self, id: u8, m: &Module) -> Result<Value, RuntimeError> {
        Ok(match id {
            B_SIZE => {
                let c = self.pop();
                Value::I32(obj::obj_len(&self.as_obj(c, m)?) as i32)
            }
            B_INT_ABS => {
                let a = self.pop();
                match a {
                    Value::I32(x) => match x.checked_abs() {
                        Some(v) => Value::I32(v),
                        None => rt!(self, m, "`int.abs` overflowed an i32"),
                    },
                    Value::I64(x) => match x.checked_abs() {
                        Some(v) => Value::I64(v),
                        None => rt!(self, m, "`int.abs` overflowed an i64"),
                    },
                    _ => rt!(self, m, "`int.abs` expects an integer"),
                }
            }
            B_INT_MIN | B_INT_MAX => {
                let b = self.pop();
                let a = self.pop();
                let lo = id == B_INT_MIN;
                match (a, b) {
                    (Value::I32(x), Value::I32(y)) => {
                        Value::I32(if lo { x.min(y) } else { x.max(y) })
                    }
                    (Value::I64(x), Value::I64(y)) => {
                        Value::I64(if lo { x.min(y) } else { x.max(y) })
                    }
                    _ => rt!(self, m, "`int.min` and `int.max` expect two integers of the same width"),
                }
            }
            B_FIND => {
                let x = self.pop();
                let c = self.pop();
                let h = self.as_obj(c, m)?;
                Value::I32(self.checked(obj::obj_find(&h, &x), m)?)
            }
            B_PUSH => {
                let x = self.pop();
                let c = self.pop();
                let h = self.as_obj(c, m)?;
                self.checked(obj::obj_push(&h, x), m)?;
                Value::Void
            }
            B_POP => {
                let c = self.pop();
                let h = self.as_obj(c, m)?;
                self.checked(obj::obj_pop(&h), m)?
            }
            B_INSERT => {
                let x = self.pop();
                let idx = self.pop();
                let c = self.pop();
                let i = self.index_of(&idx, m)?;
                let h = self.as_obj(c, m)?;
                self.checked(obj::obj_insert(&h, i, x), m)?;
                Value::Void
            }
            B_ERASE => {
                let idx = self.pop();
                let c = self.pop();
                let i = self.index_of(&idx, m)?;
                let h = self.as_obj(c, m)?;
                self.checked(obj::obj_erase(&h, i), m)?
            }
            B_ADD => {
                let x = self.pop();
                let c = self.pop();
                let h = self.as_obj(c, m)?;
                Value::Bool(self.checked(obj::obj_add(&h, x), m)?)
            }
            B_REMOVE => {
                let x = self.pop();
                let c = self.pop();
                let h = self.as_obj(c, m)?;
                Value::Bool(self.checked(obj::obj_remove(&h, &x), m)?)
            }
            B_CONTAINS => {
                let x = self.pop();
                let c = self.pop();
                let h = self.as_obj(c, m)?;
                Value::Bool(self.checked(obj::obj_contains(&h, &x), m)?)
            }
            B_CLEAR => {
                let c = self.pop();
                let h = self.as_obj(c, m)?;
                obj::obj_clear(&h);
                Value::Void
            }
            B_COPY => {
                let c = self.pop();
                obj::deep_copy(&c)
            }
            B_KEYS => {
                let c = self.pop();
                let h = self.as_obj(c, m)?;
                self.checked(obj::map_keys(&h), m)?
            }
            other => rt!(self, m, "unknown builtin #{}", other),
        })
    }

    fn as_ptr(&self, v: Value, m: &Module) -> Result<usize, RuntimeError> {
        match v {
            Value::Ptr(a) => {
                if a < self.mem.len() {
                    Ok(a)
                } else {
                    rt!(self, m, "pointer {} points outside live memory (dangling)", a)
                }
            }
            _ => rt!(self, m, "expected a pointer"),
        }
    }

    fn arith(&self, op: u8, a: Value, b: Value, m: &Module) -> Result<Value, RuntimeError> {
        macro_rules! int_op {
            ($x:expr, $y:expr, $ctor:ident, $ty:literal) => {
                match op {
                    OP_ADD => match $x.checked_add($y) {
                        Some(v) => Value::$ctor(v),
                        None => rt!(self, m, concat!($ty, " addition overflowed")),
                    },
                    OP_SUB => match $x.checked_sub($y) {
                        Some(v) => Value::$ctor(v),
                        None => rt!(self, m, concat!($ty, " subtraction overflowed")),
                    },
                    OP_MUL => match $x.checked_mul($y) {
                        Some(v) => Value::$ctor(v),
                        None => rt!(self, m, concat!($ty, " multiplication overflowed")),
                    },
                    OP_DIV => {
                        if $y == 0 {
                            rt!(self, m, "division by zero")
                        }
                        match $x.checked_div($y) {
                            Some(v) => Value::$ctor(v),
                            None => rt!(self, m, concat!($ty, " division overflowed")),
                        }
                    }
                    _ => {
                        if $y == 0 {
                            rt!(self, m, "remainder by zero")
                        }
                        match $x.checked_rem($y) {
                            Some(v) => Value::$ctor(v),
                            None => rt!(self, m, concat!($ty, " remainder overflowed")),
                        }
                    }
                }
            };
        }
        Ok(match (a, b) {
            (Value::I32(x), Value::I32(y)) => int_op!(x, y, I32, "i32"),
            (Value::I64(x), Value::I64(y)) => int_op!(x, y, I64, "i64"),
            (Value::F64(x), Value::F64(y)) => Value::F64(match op {
                OP_ADD => x + y,
                OP_SUB => x - y,
                OP_MUL => x * y,
                OP_DIV => x / y,
                _ => rt!(self, m, "`%` is not defined for f64"),
            }),
            (Value::Str(x), Value::Str(y)) => {
                if op != OP_ADD {
                    rt!(self, m, "only `+` is defined for str")
                }
                Value::Str(Rc::new(format!("{}{}", x, y)))
            }
            _ => rt!(self, m, "mismatched operand types in arithmetic"),
        })
    }

    fn compare(&self, op: u8, a: Value, b: Value, m: &Module) -> Result<bool, RuntimeError> {
        let ord = match (&a, &b) {
            (Value::I32(x), Value::I32(y)) => x.partial_cmp(y),
            (Value::I64(x), Value::I64(y)) => x.partial_cmp(y),
            (Value::F64(x), Value::F64(y)) => x.partial_cmp(y),
            (Value::Str(x), Value::Str(y)) => x.partial_cmp(y),
            (Value::Bool(x), Value::Bool(y)) => x.partial_cmp(y),
            (Value::Ptr(x), Value::Ptr(y)) => x.partial_cmp(y),
            _ => rt!(self, m, "mismatched operand types in comparison"),
        };
        Ok(match ord {
            None => op == OP_NE, // NaN: only `!=` holds
            Some(o) => match op {
                OP_EQ => o.is_eq(),
                OP_NE => o.is_ne(),
                OP_LT => o.is_lt(),
                OP_LE => o.is_le(),
                OP_GT => o.is_gt(),
                _ => o.is_ge(),
            },
        })
    }

    fn cast(&self, kind: u8, v: Value, m: &Module) -> Result<Value, RuntimeError> {
        Ok(match kind {
            CAST_I32 => match v {
                Value::I32(x) => Value::I32(x),
                Value::I64(x) => Value::I32(x as i32),
                Value::F64(x) => Value::I32(x as i32),
                _ => rt!(self, m, "cannot cast this value to i32"),
            },
            CAST_I64 => match v {
                Value::I32(x) => Value::I64(x as i64),
                Value::I64(x) => Value::I64(x),
                Value::F64(x) => Value::I64(x as i64),
                _ => rt!(self, m, "cannot cast this value to i64"),
            },
            CAST_F64 => match v {
                Value::I32(x) => Value::F64(x as f64),
                Value::I64(x) => Value::F64(x as f64),
                Value::F64(x) => Value::F64(x),
                _ => rt!(self, m, "cannot cast this value to f64"),
            },
            _ => Value::Str(Rc::new(match v {
                Value::I32(x) => x.to_string(),
                Value::I64(x) => x.to_string(),
                Value::F64(x) => format_f64(x),
                Value::Bool(x) => x.to_string(),
                Value::Str(x) => (*x).clone(),
                _ => rt!(self, m, "cannot cast this value to str"),
            })),
        })
    }

    fn str_arg(&self, v: &Value, m: &Module) -> Result<Rc<String>, RuntimeError> {
        match v {
            Value::Str(s) => Ok(s.clone()),
            _ => rt!(self, m, "expected a str"),
        }
    }

    fn f64_arg(&self, v: &Value, m: &Module) -> Result<f64, RuntimeError> {
        match v {
            Value::F64(x) => Ok(*x),
            _ => rt!(self, m, "expected an f64"),
        }
    }

    fn i32_arg(&self, v: &Value, m: &Module) -> Result<i32, RuntimeError> {
        match v {
            Value::I32(x) => Ok(*x),
            _ => rt!(self, m, "expected an i32"),
        }
    }

    /// Character offset of the `n`th character, for the `string` module.
    /// Every position binZ hands out is a character index, so that
    /// `string.size` and `string.slice` agree on non-ASCII text.
    fn char_byte(s: &str, n: usize) -> usize {
        s.char_indices().nth(n).map(|(b, _)| b).unwrap_or(s.len())
    }

    /// `string.find` answers a character index, so a byte offset from
    /// Rust's search has to be converted back.
    fn char_index_of(s: &str, byte: usize) -> i32 {
        s[..byte].chars().count() as i32
    }

    fn call_native(&self, idx: u32, args: &[Value], m: &Module) -> Result<Value, RuntimeError> {
        // The index is the position in `stdlib::NATIVES`.
        Ok(match idx {
            // ------------------------------------------------------ io
            0 => {
                println!("{}", self.str_arg(&args[0], m)?);
                Value::Void
            }

            // -------------------------------------------------- string
            1 => Value::I32(self.str_arg(&args[0], m)?.chars().count() as i32),
            2 => {
                let s = self.str_arg(&args[0], m)?;
                let i = self.i32_arg(&args[1], m)?;
                match if i < 0 { None } else { s.chars().nth(i as usize) } {
                    Some(c) => Value::Str(Rc::new(c.to_string())),
                    None => rt!(
                        self,
                        m,
                        "`string.at` index {} is out of range for a str of size {}",
                        i,
                        s.chars().count()
                    ),
                }
            }
            3 => {
                let s = self.str_arg(&args[0], m)?;
                let (a, b) = (self.i32_arg(&args[1], m)?, self.i32_arg(&args[2], m)?);
                let n = s.chars().count() as i32;
                if a < 0 || b > n || a > b {
                    rt!(self, m, "`string.slice` range {}..{} is not inside 0..{}", a, b, n);
                }
                let (lo, hi) = (Self::char_byte(&s, a as usize), Self::char_byte(&s, b as usize));
                Value::Str(Rc::new(s[lo..hi].to_string()))
            }
            4 => {
                let (s, sub) = (self.str_arg(&args[0], m)?, self.str_arg(&args[1], m)?);
                // Same convention as `container.find`: a position, or -1.
                Value::I32(match s.find(sub.as_str()) {
                    Some(b) => Self::char_index_of(&s, b),
                    None => -1,
                })
            }
            5 => {
                let (s, sub) = (self.str_arg(&args[0], m)?, self.str_arg(&args[1], m)?);
                Value::Bool(s.contains(sub.as_str()))
            }
            6 => {
                let (s, p) = (self.str_arg(&args[0], m)?, self.str_arg(&args[1], m)?);
                Value::Bool(s.starts_with(p.as_str()))
            }
            7 => {
                let (s, p) = (self.str_arg(&args[0], m)?, self.str_arg(&args[1], m)?);
                Value::Bool(s.ends_with(p.as_str()))
            }
            8 => Value::Str(Rc::new(self.str_arg(&args[0], m)?.to_uppercase())),
            9 => Value::Str(Rc::new(self.str_arg(&args[0], m)?.to_lowercase())),
            10 => Value::Str(Rc::new(self.str_arg(&args[0], m)?.trim().to_string())),
            11 => {
                let s = self.str_arg(&args[0], m)?;
                let n = self.i32_arg(&args[1], m)?;
                if n < 0 {
                    rt!(self, m, "`string.repeat` count {} is negative", n);
                }
                Value::Str(Rc::new(s.repeat(n as usize)))
            }
            12 => {
                let s = self.str_arg(&args[0], m)?;
                let from = self.str_arg(&args[1], m)?;
                let to = self.str_arg(&args[2], m)?;
                if from.is_empty() {
                    rt!(self, m, "`string.replace` cannot match an empty str");
                }
                Value::Str(Rc::new(s.replace(from.as_str(), to.as_str())))
            }
            13 => {
                let s = self.str_arg(&args[0], m)?;
                let sep = self.str_arg(&args[1], m)?;
                if sep.is_empty() {
                    rt!(self, m, "`string.split` cannot split on an empty str");
                }
                let parts: Vec<Value> = s
                    .split(sep.as_str())
                    .map(|p| Value::Str(Rc::new(p.to_string())))
                    .collect();
                obj::new_vector(parts)
            }
            14 => {
                let h = self.as_obj(args[0].clone(), m)?;
                let sep = self.str_arg(&args[1], m)?;
                let n = obj::obj_len(&h);
                let mut out = String::new();
                for i in 0..n {
                    if i > 0 {
                        out.push_str(&sep);
                    }
                    let v = self.checked(obj::obj_get(&h, i), m)?;
                    out.push_str(&self.str_arg(&v, m)?);
                }
                Value::Str(Rc::new(out))
            }

            // ----------------------------------------------------- int
            // `parse` answers the widest integer; `cast<i32>` narrows.
            15 => {
                let s = self.str_arg(&args[0], m)?;
                match s.trim().parse::<i64>() {
                    Ok(v) => Value::I64(v),
                    Err(_) => rt!(
                        self,
                        m,
                        "`int.parse` cannot read \"{}\" as an integer; guard it with `int.canParse`",
                        s
                    ),
                }
            }
            16 => Value::Bool(self.str_arg(&args[0], m)?.trim().parse::<i64>().is_ok()),

            // --------------------------------------------------- float
            17 => {
                let s = self.str_arg(&args[0], m)?;
                match s.trim().parse::<f64>() {
                    Ok(v) => Value::F64(v),
                    Err(_) => rt!(
                        self,
                        m,
                        "`float.parse` cannot read \"{}\" as an f64; guard it with `float.canParse`",
                        s
                    ),
                }
            }
            18 => Value::Bool(self.str_arg(&args[0], m)?.trim().parse::<f64>().is_ok()),
            19 => Value::F64(self.f64_arg(&args[0], m)?.abs()),
            20 => Value::F64(self.f64_arg(&args[0], m)?.min(self.f64_arg(&args[1], m)?)),
            21 => Value::F64(self.f64_arg(&args[0], m)?.max(self.f64_arg(&args[1], m)?)),
            22 => Value::F64(self.f64_arg(&args[0], m)?.floor()),
            23 => Value::F64(self.f64_arg(&args[0], m)?.ceil()),
            24 => Value::F64(self.f64_arg(&args[0], m)?.round()),
            25 => Value::F64(self.f64_arg(&args[0], m)?.sqrt()),
            26 => Value::F64(self.f64_arg(&args[0], m)?.powf(self.f64_arg(&args[1], m)?)),
            27 => Value::Bool(self.f64_arg(&args[0], m)?.is_nan()),

            _ => rt!(self, m, "unknown stdlib function #{}", idx),
        })
    }
}
