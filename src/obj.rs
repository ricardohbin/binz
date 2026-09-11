//! Heap storage behind `Vector<T>`, `LinkedList<T>`, `Set<T>` and
//! `SortedSet<T>`.
//!
//! Every one of them is reached through a reference-counted handle, so a
//! container value is a single slot and copying it aliases the same storage.
//! `copy(c)` is the one way to get an independent container.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;

use crate::vm::Value;

pub type Handle = Rc<RefCell<Obj>>;

#[derive(Debug)]
pub enum Obj {
    Vector(Vec<Value>),
    List(List),
    Set(SetData),
    SortedSet(Vec<Value>),
}

pub type OResult<T> = Result<T, String>;

fn handle(o: Obj) -> Value {
    Value::Obj(Rc::new(RefCell::new(o)))
}

// ------------------------------------------------------------------- values

/// Structural equality, used by `find`. Containers and pointers compare by
/// identity, scalars by value.
pub fn value_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::I32(x), Value::I32(y)) => x == y,
        (Value::I64(x), Value::I64(y)) => x == y,
        (Value::F64(x), Value::F64(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Ptr(x), Value::Ptr(y)) => x == y,
        (Value::Fn(x), Value::Fn(y)) => x == y,
        (Value::Native(x), Value::Native(y)) => x == y,
        (Value::Obj(x), Value::Obj(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

/// Hashable identity of a set element. The compiler already restricts set
/// elements to scalars, so the error arms here are defence in depth.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    I32(i32),
    I64(i64),
    F64(u64),
    Bool(bool),
    Str(String),
}

pub fn key_of(v: &Value) -> OResult<Key> {
    Ok(match v {
        Value::I32(x) => Key::I32(*x),
        Value::I64(x) => Key::I64(*x),
        Value::F64(x) => {
            if x.is_nan() {
                return Err("a set cannot hold NaN: it is not equal to itself".into());
            }
            // -0.0 and 0.0 are the same element.
            Key::F64(if *x == 0.0 { 0.0f64.to_bits() } else { x.to_bits() })
        }
        Value::Bool(x) => Key::Bool(*x),
        Value::Str(x) => Key::Str((**x).clone()),
        _ => return Err("this value cannot be a set element".into()),
    })
}

pub fn cmp_values(a: &Value, b: &Value) -> OResult<Ordering> {
    let o = match (a, b) {
        (Value::I32(x), Value::I32(y)) => x.cmp(y),
        (Value::I64(x), Value::I64(y)) => x.cmp(y),
        (Value::F64(x), Value::F64(y)) => match x.partial_cmp(y) {
            Some(o) => o,
            None => return Err("a SortedSet cannot hold NaN: it has no place in the order".into()),
        },
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Str(x), Value::Str(y)) => x.cmp(y),
        _ => return Err("this value cannot be a set element".into()),
    };
    Ok(o)
}

/// The deep copy behind `copy(c)`: nested containers are copied too, so no
/// part of the result is shared with the original.
pub fn deep_copy(v: &Value) -> Value {
    match v {
        Value::Obj(h) => handle(match &*h.borrow() {
            Obj::Vector(items) => Obj::Vector(items.iter().map(deep_copy).collect()),
            Obj::List(l) => Obj::List(List::from_vec(l.to_vec().iter().map(deep_copy).collect())),
            Obj::Set(s) => {
                let mut out = SetData::default();
                for item in &s.items {
                    // Keys are scalars, so this cannot fail for a live set.
                    let _ = out.add(deep_copy(item));
                }
                Obj::Set(out)
            }
            Obj::SortedSet(items) => Obj::SortedSet(items.iter().map(deep_copy).collect()),
        }),
        other => other.clone(),
    }
}

// -------------------------------------------------------------- linked list

#[derive(Debug, Clone)]
struct Node {
    val: Value,
    prev: i32,
    next: i32,
}

/// A real doubly linked list, held in an arena so the nodes can live in one
/// allocation. Positions are reached by walking, from whichever end is
/// closer -- indexing a list is O(n) and that is the honest cost.
#[derive(Debug, Default)]
pub struct List {
    nodes: Vec<Node>,
    free: Vec<u32>,
    head: i32,
    tail: i32,
    len: usize,
}

impl List {
    pub fn new() -> List {
        List { nodes: Vec::new(), free: Vec::new(), head: -1, tail: -1, len: 0 }
    }

    pub fn from_vec(values: Vec<Value>) -> List {
        let mut l = List::new();
        for v in values {
            l.push_back(v);
        }
        l
    }

    pub fn len(&self) -> usize {
        self.len
    }

    fn alloc(&mut self, val: Value) -> i32 {
        match self.free.pop() {
            Some(i) => {
                self.nodes[i as usize] = Node { val, prev: -1, next: -1 };
                i as i32
            }
            None => {
                self.nodes.push(Node { val, prev: -1, next: -1 });
                (self.nodes.len() - 1) as i32
            }
        }
    }

    fn at(&self, i: usize) -> OResult<i32> {
        if i >= self.len {
            return Err(format!("index {} is out of range for a LinkedList of length {}", i, self.len));
        }
        // Walk from the closer end.
        if i <= self.len / 2 {
            let mut cur = self.head;
            for _ in 0..i {
                cur = self.nodes[cur as usize].next;
            }
            Ok(cur)
        } else {
            let mut cur = self.tail;
            for _ in 0..(self.len - 1 - i) {
                cur = self.nodes[cur as usize].prev;
            }
            Ok(cur)
        }
    }

    pub fn get(&self, i: usize) -> OResult<Value> {
        Ok(self.nodes[self.at(i)? as usize].val.clone())
    }

    pub fn set(&mut self, i: usize, v: Value) -> OResult<()> {
        let n = self.at(i)?;
        self.nodes[n as usize].val = v;
        Ok(())
    }

    pub fn push_back(&mut self, v: Value) {
        let n = self.alloc(v);
        self.nodes[n as usize].prev = self.tail;
        if self.tail >= 0 {
            self.nodes[self.tail as usize].next = n;
        } else {
            self.head = n;
        }
        self.tail = n;
        self.len += 1;
    }

    fn unlink(&mut self, n: i32) -> Value {
        let (prev, next) = {
            let node = &self.nodes[n as usize];
            (node.prev, node.next)
        };
        if prev >= 0 {
            self.nodes[prev as usize].next = next;
        } else {
            self.head = next;
        }
        if next >= 0 {
            self.nodes[next as usize].prev = prev;
        } else {
            self.tail = prev;
        }
        self.len -= 1;
        self.free.push(n as u32);
        std::mem::replace(&mut self.nodes[n as usize].val, Value::Void)
    }

    pub fn pop_back(&mut self) -> OResult<Value> {
        if self.tail < 0 {
            return Err("pop on an empty LinkedList".into());
        }
        let n = self.tail;
        Ok(self.unlink(n))
    }

    pub fn insert(&mut self, i: usize, v: Value) -> OResult<()> {
        if i == self.len {
            self.push_back(v);
            return Ok(());
        }
        let at = self.at(i)?;
        let n = self.alloc(v);
        let prev = self.nodes[at as usize].prev;
        self.nodes[n as usize].prev = prev;
        self.nodes[n as usize].next = at;
        self.nodes[at as usize].prev = n;
        if prev >= 0 {
            self.nodes[prev as usize].next = n;
        } else {
            self.head = n;
        }
        self.len += 1;
        Ok(())
    }

    pub fn erase(&mut self, i: usize) -> OResult<Value> {
        let n = self.at(i)?;
        Ok(self.unlink(n))
    }

    pub fn find(&self, v: &Value) -> i32 {
        let mut cur = self.head;
        let mut i = 0i32;
        while cur >= 0 {
            if value_eq(&self.nodes[cur as usize].val, v) {
                return i;
            }
            cur = self.nodes[cur as usize].next;
            i += 1;
        }
        -1
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
        self.free.clear();
        self.head = -1;
        self.tail = -1;
        self.len = 0;
    }

    pub fn to_vec(&self) -> Vec<Value> {
        let mut out = Vec::with_capacity(self.len);
        let mut cur = self.head;
        while cur >= 0 {
            out.push(self.nodes[cur as usize].val.clone());
            cur = self.nodes[cur as usize].next;
        }
        out
    }
}

// ---------------------------------------------------------------------- set

/// A hash set that keeps insertion order, so `s[i]` and iteration are
/// deterministic across runs.
#[derive(Debug, Default)]
pub struct SetData {
    pub items: Vec<Value>,
    index: HashMap<Key, usize>,
}

impl SetData {
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn contains(&self, v: &Value) -> OResult<bool> {
        Ok(self.index.contains_key(&key_of(v)?))
    }

    pub fn add(&mut self, v: Value) -> OResult<bool> {
        let k = key_of(&v)?;
        if self.index.contains_key(&k) {
            return Ok(false);
        }
        self.index.insert(k, self.items.len());
        self.items.push(v);
        Ok(true)
    }

    pub fn remove(&mut self, v: &Value) -> OResult<bool> {
        let k = key_of(v)?;
        let pos = match self.index.remove(&k) {
            Some(p) => p,
            None => return Ok(false),
        };
        self.items.remove(pos);
        for slot in self.index.values_mut() {
            if *slot > pos {
                *slot -= 1;
            }
        }
        Ok(true)
    }

    pub fn get(&self, i: usize) -> OResult<Value> {
        match self.items.get(i) {
            Some(v) => Ok(v.clone()),
            None => Err(format!("index {} is out of range for a Set of length {}", i, self.items.len())),
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.index.clear();
    }
}

// --------------------------------------------------------------- sorted set

/// Kept as a sorted vector: membership and insertion are a binary search,
/// and `s[i]` yields elements in ascending order.
pub fn sorted_search(items: &[Value], v: &Value) -> OResult<Result<usize, usize>> {
    // An empty set never reaches a comparison, so NaN is rejected up front.
    if let Value::F64(x) = v {
        if x.is_nan() {
            return Err("a SortedSet cannot hold NaN: it has no place in the order".into());
        }
    }
    let mut lo = 0usize;
    let mut hi = items.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        match cmp_values(&items[mid], v)? {
            Ordering::Less => lo = mid + 1,
            Ordering::Greater => hi = mid,
            Ordering::Equal => return Ok(Ok(mid)),
        }
    }
    Ok(Err(lo))
}

pub fn sorted_add(items: &mut Vec<Value>, v: Value) -> OResult<bool> {
    match sorted_search(items, &v)? {
        Ok(_) => Ok(false),
        Err(at) => {
            items.insert(at, v);
            Ok(true)
        }
    }
}

pub fn sorted_remove(items: &mut Vec<Value>, v: &Value) -> OResult<bool> {
    match sorted_search(items, v)? {
        Ok(at) => {
            items.remove(at);
            Ok(true)
        }
        Err(_) => Ok(false),
    }
}

// ------------------------------------------------------------- entry points
//
// The compiler has already checked which builtin may be applied to which
// container, so the mismatched arms below are defence in depth.

use crate::bytecode::{KIND_LIST, KIND_SET, KIND_SORTED_SET, KIND_VECTOR};

pub fn new_container(kind: u8, values: Vec<Value>) -> OResult<Value> {
    Ok(handle(match kind {
        KIND_VECTOR => Obj::Vector(values),
        KIND_LIST => Obj::List(List::from_vec(values)),
        KIND_SET => {
            let mut s = SetData::default();
            for v in values {
                s.add(v)?;
            }
            Obj::Set(s)
        }
        KIND_SORTED_SET => {
            let mut items = Vec::new();
            for v in values {
                sorted_add(&mut items, v)?;
            }
            Obj::SortedSet(items)
        }
        other => return Err(format!("unknown container kind {}", other)),
    }))
}

pub fn kind_name(o: &Obj) -> &'static str {
    match o {
        Obj::Vector(_) => "Vector",
        Obj::List(_) => "LinkedList",
        Obj::Set(_) => "Set",
        Obj::SortedSet(_) => "SortedSet",
    }
}

fn oob(o: &Obj, i: usize, len: usize) -> String {
    format!("index {} is out of range for a {} of length {}", i, kind_name(o), len)
}

pub fn obj_len(h: &Handle) -> usize {
    match &*h.borrow() {
        Obj::Vector(items) | Obj::SortedSet(items) => items.len(),
        Obj::List(l) => l.len(),
        Obj::Set(s) => s.len(),
    }
}

pub fn obj_get(h: &Handle, i: usize) -> OResult<Value> {
    let o = h.borrow();
    match &*o {
        Obj::Vector(items) | Obj::SortedSet(items) => match items.get(i) {
            Some(v) => Ok(v.clone()),
            None => Err(oob(&o, i, items.len())),
        },
        Obj::List(l) => l.get(i),
        Obj::Set(s) => s.get(i),
    }
}

pub fn obj_set(h: &Handle, i: usize, v: Value) -> OResult<()> {
    let mut o = h.borrow_mut();
    match &mut *o {
        Obj::Vector(items) => {
            if i >= items.len() {
                let len = items.len();
                return Err(format!("index {} is out of range for a Vector of length {}", i, len));
            }
            items[i] = v;
            Ok(())
        }
        Obj::List(l) => l.set(i, v),
        _ => Err("elements of a set are its keys and cannot be replaced".into()),
    }
}

pub fn obj_push(h: &Handle, v: Value) -> OResult<()> {
    match &mut *h.borrow_mut() {
        Obj::Vector(items) => {
            items.push(v);
            Ok(())
        }
        Obj::List(l) => {
            l.push_back(v);
            Ok(())
        }
        other => Err(format!("`push` is not defined for a {}", kind_name(other))),
    }
}

pub fn obj_pop(h: &Handle) -> OResult<Value> {
    match &mut *h.borrow_mut() {
        Obj::Vector(items) => items.pop().ok_or_else(|| "pop on an empty Vector".to_string()),
        Obj::List(l) => l.pop_back(),
        other => Err(format!("`pop` is not defined for a {}", kind_name(other))),
    }
}

pub fn obj_insert(h: &Handle, i: usize, v: Value) -> OResult<()> {
    match &mut *h.borrow_mut() {
        Obj::Vector(items) => {
            if i > items.len() {
                let len = items.len();
                return Err(format!("cannot insert at {} in a Vector of length {}", i, len));
            }
            items.insert(i, v);
            Ok(())
        }
        Obj::List(l) => {
            if i > l.len() {
                let len = l.len();
                return Err(format!("cannot insert at {} in a LinkedList of length {}", i, len));
            }
            l.insert(i, v)
        }
        other => Err(format!("`insert` is not defined for a {}", kind_name(other))),
    }
}

pub fn obj_erase(h: &Handle, i: usize) -> OResult<Value> {
    let mut o = h.borrow_mut();
    match &mut *o {
        Obj::Vector(items) => {
            if i >= items.len() {
                let len = items.len();
                return Err(format!("index {} is out of range for a Vector of length {}", i, len));
            }
            Ok(items.remove(i))
        }
        Obj::List(l) => l.erase(i),
        other => Err(format!("`erase` is not defined for a {}", kind_name(other))),
    }
}

pub fn obj_find(h: &Handle, v: &Value) -> OResult<i32> {
    match &*h.borrow() {
        Obj::Vector(items) => {
            Ok(items.iter().position(|x| value_eq(x, v)).map(|p| p as i32).unwrap_or(-1))
        }
        Obj::List(l) => Ok(l.find(v)),
        other => Err(format!("`find` is not defined for a {}; use `contains`", kind_name(other))),
    }
}

pub fn obj_add(h: &Handle, v: Value) -> OResult<bool> {
    match &mut *h.borrow_mut() {
        Obj::Set(s) => s.add(v),
        Obj::SortedSet(items) => sorted_add(items, v),
        other => Err(format!("`add` is not defined for a {}; use `push`", kind_name(other))),
    }
}

pub fn obj_remove(h: &Handle, v: &Value) -> OResult<bool> {
    match &mut *h.borrow_mut() {
        Obj::Set(s) => s.remove(v),
        Obj::SortedSet(items) => sorted_remove(items, v),
        other => Err(format!("`remove` is not defined for a {}; use `erase`", kind_name(other))),
    }
}

pub fn obj_contains(h: &Handle, v: &Value) -> OResult<bool> {
    match &*h.borrow() {
        Obj::Set(s) => s.contains(v),
        Obj::SortedSet(items) => Ok(sorted_search(items, v)?.is_ok()),
        other => Err(format!("`contains` is not defined for a {}; use `find`", kind_name(other))),
    }
}

pub fn obj_clear(h: &Handle) {
    match &mut *h.borrow_mut() {
        Obj::Vector(items) | Obj::SortedSet(items) => items.clear(),
        Obj::List(l) => l.clear(),
        Obj::Set(s) => s.clear(),
    }
}
