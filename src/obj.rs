//! Heap storage behind `Vector<T>`, `LinkedList<T>`, `Set<T>`,
//! `SortedSet<T>`, `HashMap<K, V>` and `SortedMap<K, V>`.
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
    Map(MapData),
    SortedMap(SortedMapData),
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

/// Hashable identity of a set element or a map key. The compiler already
/// restricts both to scalars, so the error arms here are defence in depth.
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
                return Err("NaN cannot be a key: it is not equal to itself".into());
            }
            // -0.0 and 0.0 are the same element.
            Key::F64(if *x == 0.0 { 0.0f64.to_bits() } else { x.to_bits() })
        }
        Value::Bool(x) => Key::Bool(*x),
        Value::Str(x) => Key::Str((**x).clone()),
        _ => return Err("this value cannot be a key".into()),
    })
}

/// A key as it would be written in source, for the "no such key" message.
fn show_key(v: &Value) -> String {
    match v {
        Value::I32(x) => x.to_string(),
        Value::I64(x) => x.to_string(),
        Value::F64(x) => crate::vm::format_f64(*x),
        Value::Bool(x) => x.to_string(),
        Value::Str(s) => format!("{:?}", s),
        _ => "that value".into(),
    }
}

/// NaN is rejected before the first comparison, because an empty sorted
/// container would never reach one and would otherwise accept it.
fn reject_nan(v: &Value, what: &str) -> OResult<()> {
    if let Value::F64(x) = v {
        if x.is_nan() {
            return Err(format!("a {} cannot hold NaN: it has no place in the order", what));
        }
    }
    Ok(())
}

pub fn cmp_values(a: &Value, b: &Value) -> OResult<Ordering> {
    let o = match (a, b) {
        (Value::I32(x), Value::I32(y)) => x.cmp(y),
        (Value::I64(x), Value::I64(y)) => x.cmp(y),
        (Value::F64(x), Value::F64(y)) => match x.partial_cmp(y) {
            Some(o) => o,
            // Both sorted containers reject NaN before they get here.
            None => return Err("NaN has no place in an order".into()),
        },
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Str(x), Value::Str(y)) => x.cmp(y),
        _ => return Err("these values cannot be compared".into()),
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
            Obj::Map(mp) => {
                let mut out = MapData::default();
                for (k, v) in &mp.entries {
                    // Keys are scalars, so this cannot fail for a live map.
                    let _ = out.set(k.clone(), deep_copy(v));
                }
                Obj::Map(out)
            }
            Obj::SortedMap(sm) => {
                let mut out = SortedMapData::new();
                // Rebuilding in ascending order would make the tree a right
                // spine in a plain BST; the red-black fixup rebalances it,
                // so the copy costs the same shape as the original.
                for (k, v) in sm.entries() {
                    let _ = out.set(k, deep_copy(&v));
                }
                Obj::SortedMap(out)
            }
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
    reject_nan(v, "SortedSet")?;
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

// ---------------------------------------------------------------- hash map

/// A hash map that keeps insertion order, so `keys(m)` is deterministic
/// across runs. Same shape as `SetData`, with a value beside every key.
#[derive(Debug, Default)]
pub struct MapData {
    entries: Vec<(Value, Value)>,
    index: HashMap<Key, usize>,
}

impl MapData {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn contains(&self, k: &Value) -> OResult<bool> {
        Ok(self.index.contains_key(&key_of(k)?))
    }

    /// binZ has no `null`, so reading an absent key is a trap rather than a
    /// value the program has to test for.
    pub fn get(&self, k: &Value) -> OResult<Value> {
        match self.index.get(&key_of(k)?) {
            Some(at) => Ok(self.entries[*at].1.clone()),
            None => Err(format!(
                "key {} is not in the HashMap; test with `contains` first",
                show_key(k)
            )),
        }
    }

    /// Inserts or overwrites. A key keeps the position of its first insert.
    pub fn set(&mut self, k: Value, v: Value) -> OResult<()> {
        let key = key_of(&k)?;
        match self.index.get(&key) {
            Some(at) => self.entries[*at].1 = v,
            None => {
                self.index.insert(key, self.entries.len());
                self.entries.push((k, v));
            }
        }
        Ok(())
    }

    pub fn remove(&mut self, k: &Value) -> OResult<bool> {
        let key = key_of(k)?;
        let pos = match self.index.remove(&key) {
            Some(p) => p,
            None => return Ok(false),
        };
        self.entries.remove(pos);
        for slot in self.index.values_mut() {
            if *slot > pos {
                *slot -= 1;
            }
        }
        Ok(true)
    }

    pub fn keys(&self) -> Vec<Value> {
        self.entries.iter().map(|e| e.0.clone()).collect()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.index.clear();
    }
}


// ----------------------------------------------------------- sorted map

/// A genuine red-black tree, held in a node arena so the whole map is one
/// allocation and a node index is stable while it lives. `NIL` is `-1` and
/// counts as black; there is no sentinel node, so every place that would
/// have read `nil.parent` carries the parent explicitly instead.
///
/// This is what makes `SortedMap` different from `HashMap`: keys come back
/// in ascending order, and the cost of every operation is `O(log n)` rather
/// than `O(1)` amortised.
const NIL: i32 = -1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Color {
    Red,
    Black,
}

#[derive(Debug)]
struct RbNode {
    key: Value,
    val: Value,
    left: i32,
    right: i32,
    parent: i32,
    color: Color,
}

#[derive(Debug, Default)]
pub struct SortedMapData {
    nodes: Vec<RbNode>,
    free: Vec<u32>,
    root: i32,
    len: usize,
}

impl SortedMapData {
    pub fn new() -> SortedMapData {
        SortedMapData { nodes: Vec::new(), free: Vec::new(), root: NIL, len: 0 }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    // ------------------------------------------------------------ plumbing

    fn color(&self, n: i32) -> Color {
        if n == NIL {
            Color::Black
        } else {
            self.nodes[n as usize].color
        }
    }

    fn paint(&mut self, n: i32, c: Color) {
        if n != NIL {
            self.nodes[n as usize].color = c;
        }
    }

    fn left(&self, n: i32) -> i32 {
        if n == NIL { NIL } else { self.nodes[n as usize].left }
    }

    fn right(&self, n: i32) -> i32 {
        if n == NIL { NIL } else { self.nodes[n as usize].right }
    }

    fn parent(&self, n: i32) -> i32 {
        if n == NIL { NIL } else { self.nodes[n as usize].parent }
    }

    fn set_left(&mut self, n: i32, c: i32) {
        self.nodes[n as usize].left = c;
    }

    fn set_right(&mut self, n: i32, c: i32) {
        self.nodes[n as usize].right = c;
    }

    fn set_parent(&mut self, n: i32, p: i32) {
        if n != NIL {
            self.nodes[n as usize].parent = p;
        }
    }

    fn alloc(&mut self, key: Value, val: Value) -> i32 {
        let node = RbNode { key, val, left: NIL, right: NIL, parent: NIL, color: Color::Red };
        match self.free.pop() {
            Some(i) => {
                self.nodes[i as usize] = node;
                i as i32
            }
            None => {
                self.nodes.push(node);
                (self.nodes.len() - 1) as i32
            }
        }
    }

    /// Frees a node's index and drops the values it held, so a long-lived
    /// map that churns keys does not pin their storage.
    fn release(&mut self, n: i32) {
        let slot = &mut self.nodes[n as usize];
        slot.key = Value::Void;
        slot.val = Value::Void;
        slot.left = NIL;
        slot.right = NIL;
        slot.parent = NIL;
        self.free.push(n as u32);
    }

    fn rotate_left(&mut self, x: i32) {
        let y = self.right(x);
        let yl = self.left(y);
        self.set_right(x, yl);
        self.set_parent(yl, x);
        let p = self.parent(x);
        self.set_parent(y, p);
        if p == NIL {
            self.root = y;
        } else if self.left(p) == x {
            self.set_left(p, y);
        } else {
            self.set_right(p, y);
        }
        self.set_left(y, x);
        self.set_parent(x, y);
    }

    fn rotate_right(&mut self, x: i32) {
        let y = self.left(x);
        let yr = self.right(y);
        self.set_left(x, yr);
        self.set_parent(yr, x);
        let p = self.parent(x);
        self.set_parent(y, p);
        if p == NIL {
            self.root = y;
        } else if self.right(p) == x {
            self.set_right(p, y);
        } else {
            self.set_left(p, y);
        }
        self.set_right(y, x);
        self.set_parent(x, y);
    }

    // -------------------------------------------------------------- search

    /// The node holding `k`, or `NIL`. Comparison can fail, so this answers
    /// an `OResult` -- the same reason `SortedSet` rejects NaN up front.
    fn find(&self, k: &Value) -> OResult<i32> {
        reject_nan(k, "SortedMap")?;
        let mut cur = self.root;
        while cur != NIL {
            cur = match cmp_values(k, &self.nodes[cur as usize].key)? {
                Ordering::Less => self.left(cur),
                Ordering::Greater => self.right(cur),
                Ordering::Equal => return Ok(cur),
            };
        }
        Ok(NIL)
    }

    pub fn contains(&self, k: &Value) -> OResult<bool> {
        Ok(self.find(k)? != NIL)
    }

    /// binZ has no `null`, so reading an absent key is a trap rather than a
    /// value the program has to test for -- same rule as `HashMap`.
    pub fn get(&self, k: &Value) -> OResult<Value> {
        match self.find(k)? {
            NIL => Err(format!(
                "key {} is not in the SortedMap; test with `map.contains` first",
                show_key(k)
            )),
            n => Ok(self.nodes[n as usize].val.clone()),
        }
    }

    // -------------------------------------------------------------- insert

    /// Inserts or overwrites, so `m[key] = value` stays the one way in.
    pub fn set(&mut self, k: Value, v: Value) -> OResult<()> {
        reject_nan(&k, "SortedMap")?;
        let mut parent = NIL;
        let mut cur = self.root;
        let mut went_left = false;
        while cur != NIL {
            parent = cur;
            match cmp_values(&k, &self.nodes[cur as usize].key)? {
                Ordering::Less => {
                    went_left = true;
                    cur = self.left(cur);
                }
                Ordering::Greater => {
                    went_left = false;
                    cur = self.right(cur);
                }
                Ordering::Equal => {
                    self.nodes[cur as usize].val = v;
                    return Ok(());
                }
            }
        }
        let n = self.alloc(k, v);
        self.set_parent(n, parent);
        if parent == NIL {
            self.root = n;
        } else if went_left {
            self.set_left(parent, n);
        } else {
            self.set_right(parent, n);
        }
        self.len += 1;
        self.insert_fixup(n);
        Ok(())
    }

    fn insert_fixup(&mut self, mut z: i32) {
        while self.color(self.parent(z)) == Color::Red {
            let p = self.parent(z);
            let g = self.parent(p);
            // `g` is black and therefore real: a red `p` cannot be the root.
            if p == self.left(g) {
                let uncle = self.right(g);
                if self.color(uncle) == Color::Red {
                    self.paint(p, Color::Black);
                    self.paint(uncle, Color::Black);
                    self.paint(g, Color::Red);
                    z = g;
                } else {
                    if z == self.right(p) {
                        z = p;
                        self.rotate_left(z);
                    }
                    let p = self.parent(z);
                    let g = self.parent(p);
                    self.paint(p, Color::Black);
                    self.paint(g, Color::Red);
                    self.rotate_right(g);
                }
            } else {
                let uncle = self.left(g);
                if self.color(uncle) == Color::Red {
                    self.paint(p, Color::Black);
                    self.paint(uncle, Color::Black);
                    self.paint(g, Color::Red);
                    z = g;
                } else {
                    if z == self.left(p) {
                        z = p;
                        self.rotate_right(z);
                    }
                    let p = self.parent(z);
                    let g = self.parent(p);
                    self.paint(p, Color::Black);
                    self.paint(g, Color::Red);
                    self.rotate_left(g);
                }
            }
        }
        let root = self.root;
        self.paint(root, Color::Black);
    }

    // -------------------------------------------------------------- remove

    /// Puts `v` where `u` was. `v` may be `NIL`, which is why the caller has
    /// to remember `u`'s parent before calling this.
    fn transplant(&mut self, u: i32, v: i32) {
        let p = self.parent(u);
        if p == NIL {
            self.root = v;
        } else if self.left(p) == u {
            self.set_left(p, v);
        } else {
            self.set_right(p, v);
        }
        self.set_parent(v, p);
    }

    fn minimum(&self, mut n: i32) -> i32 {
        while self.left(n) != NIL {
            n = self.left(n);
        }
        n
    }

    pub fn remove(&mut self, k: &Value) -> OResult<bool> {
        let z = self.find(k)?;
        if z == NIL {
            return Ok(false);
        }
        let mut removed_color = self.color(z);
        let x;
        let x_parent;
        if self.left(z) == NIL {
            x = self.right(z);
            x_parent = self.parent(z);
            self.transplant(z, x);
        } else if self.right(z) == NIL {
            x = self.left(z);
            x_parent = self.parent(z);
            self.transplant(z, x);
        } else {
            // `y` is `z`'s successor, and takes `z`'s place links and all,
            // so no key or value ever moves between nodes.
            let y = self.minimum(self.right(z));
            removed_color = self.color(y);
            x = self.right(y);
            if self.parent(y) == z {
                x_parent = y;
            } else {
                x_parent = self.parent(y);
                let yr = self.right(y);
                self.transplant(y, yr);
                let zr = self.right(z);
                self.set_right(y, zr);
                self.set_parent(zr, y);
            }
            self.transplant(z, y);
            let zl = self.left(z);
            self.set_left(y, zl);
            self.set_parent(zl, y);
            let zc = self.color(z);
            self.paint(y, zc);
        }
        self.release(z);
        self.len -= 1;
        if removed_color == Color::Black {
            self.remove_fixup(x, x_parent);
        }
        Ok(true)
    }

    fn remove_fixup(&mut self, mut x: i32, mut xp: i32) {
        while x != self.root && self.color(x) == Color::Black {
            if xp == NIL {
                break;
            }
            let on_left = self.left(xp) == x;
            let mut w = if on_left { self.right(xp) } else { self.left(xp) };
            // A doubly-black `x` always has a real sibling in a well formed
            // tree; bailing out here is defence in depth, not a case.
            if w == NIL {
                break;
            }
            if self.color(w) == Color::Red {
                self.paint(w, Color::Black);
                self.paint(xp, Color::Red);
                if on_left {
                    self.rotate_left(xp);
                    w = self.right(xp);
                } else {
                    self.rotate_right(xp);
                    w = self.left(xp);
                }
                if w == NIL {
                    break;
                }
            }
            let (near, far) = if on_left {
                (self.left(w), self.right(w))
            } else {
                (self.right(w), self.left(w))
            };
            if self.color(near) == Color::Black && self.color(far) == Color::Black {
                self.paint(w, Color::Red);
                x = xp;
                xp = self.parent(x);
            } else {
                if self.color(far) == Color::Black {
                    self.paint(near, Color::Black);
                    self.paint(w, Color::Red);
                    if on_left {
                        self.rotate_right(w);
                        w = self.right(xp);
                    } else {
                        self.rotate_left(w);
                        w = self.left(xp);
                    }
                }
                let pc = self.color(xp);
                self.paint(w, pc);
                self.paint(xp, Color::Black);
                let far = if on_left { self.right(w) } else { self.left(w) };
                self.paint(far, Color::Black);
                if on_left {
                    self.rotate_left(xp);
                } else {
                    self.rotate_right(xp);
                }
                x = self.root;
                xp = NIL;
            }
        }
        self.paint(x, Color::Black);
    }

    // ------------------------------------------------------------- walking

    /// In-order, so ascending by key. Iterative, because a degenerate tree
    /// is only `O(log n)` deep but recursion would still be a stack risk in
    /// a VM that already owns one.
    fn in_order(&self) -> Vec<i32> {
        let mut out = Vec::with_capacity(self.len);
        let mut stack: Vec<i32> = Vec::new();
        let mut cur = self.root;
        while cur != NIL || !stack.is_empty() {
            while cur != NIL {
                stack.push(cur);
                cur = self.left(cur);
            }
            let n = stack.pop().unwrap();
            out.push(n);
            cur = self.right(n);
        }
        out
    }

    pub fn keys(&self) -> Vec<Value> {
        self.in_order().into_iter().map(|n| self.nodes[n as usize].key.clone()).collect()
    }

    pub fn entries(&self) -> Vec<(Value, Value)> {
        self.in_order()
            .into_iter()
            .map(|n| (self.nodes[n as usize].key.clone(), self.nodes[n as usize].val.clone()))
            .collect()
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
        self.free.clear();
        self.root = NIL;
        self.len = 0;
    }
}

// ------------------------------------------------------------- entry points
//
// The compiler has already checked which builtin may be applied to which
// container, so the mismatched arms below are defence in depth.

use crate::bytecode::{
    KIND_LIST, KIND_MAP, KIND_SET, KIND_SORTED_MAP, KIND_SORTED_SET, KIND_VECTOR,
};

/// The `Vector<T>` that `keys(m)` hands back.
pub fn new_vector(values: Vec<Value>) -> Value {
    handle(Obj::Vector(values))
}

/// A map literal arrives as a flat key, value, key, value run, so both
/// maps pair it up the same way.
fn map_entries(values: Vec<Value>, what: &str) -> OResult<Vec<(Value, Value)>> {
    let mut out = Vec::with_capacity(values.len() / 2);
    let mut it = values.into_iter();
    while let Some(k) = it.next() {
        match it.next() {
            Some(v) => out.push((k, v)),
            None => return Err(format!("a {} literal ended without a value", what)),
        }
    }
    Ok(out)
}

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
        KIND_MAP => {
            // A map literal pushes a key and a value per entry, in order.
            let mut mp = MapData::default();
            for (k, v) in map_entries(values, "HashMap")? {
                mp.set(k, v)?;
            }
            Obj::Map(mp)
        }
        KIND_SORTED_MAP => {
            let mut sm = SortedMapData::new();
            for (k, v) in map_entries(values, "SortedMap")? {
                sm.set(k, v)?;
            }
            Obj::SortedMap(sm)
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
        Obj::Map(_) => "HashMap",
        Obj::SortedMap(_) => "SortedMap",
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
        Obj::Map(mp) => mp.len(),
        Obj::SortedMap(sm) => sm.len(),
    }
}

/// A map is reached by key, so `m[k]` takes a different path than `c[i]`.
pub fn is_map(h: &Handle) -> bool {
    matches!(&*h.borrow(), Obj::Map(_) | Obj::SortedMap(_))
}

pub fn map_get(h: &Handle, k: &Value) -> OResult<Value> {
    match &*h.borrow() {
        Obj::Map(mp) => mp.get(k),
        Obj::SortedMap(sm) => sm.get(k),
        other => Err(format!("a {} is indexed by position, not by key", kind_name(other))),
    }
}

pub fn map_set(h: &Handle, k: Value, v: Value) -> OResult<()> {
    match &mut *h.borrow_mut() {
        Obj::Map(mp) => mp.set(k, v),
        Obj::SortedMap(sm) => sm.set(k, v),
        other => Err(format!("a {} is indexed by position, not by key", kind_name(other))),
    }
}

/// Insertion order for a `HashMap`, ascending for a `SortedMap` -- the one
/// thing that tells the two apart.
pub fn map_keys(h: &Handle) -> OResult<Value> {
    match &*h.borrow() {
        Obj::Map(mp) => Ok(new_vector(mp.keys())),
        Obj::SortedMap(sm) => Ok(new_vector(sm.keys())),
        other => Err(format!("`keys` is not defined for a {}", kind_name(other))),
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
        Obj::Map(_) | Obj::SortedMap(_) => {
            Err(format!("a {} is indexed by key, not by position", kind_name(&o)))
        }
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
        Obj::Map(_) | Obj::SortedMap(_) => {
            Err("a map is indexed by key, not by position".into())
        }
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
        Obj::Map(_) | Obj::SortedMap(_) => {
            Err("`add` is not defined for a map; write `m[key] = value`".into())
        }
        other => Err(format!("`add` is not defined for a {}; use `push`", kind_name(other))),
    }
}

pub fn obj_remove(h: &Handle, v: &Value) -> OResult<bool> {
    match &mut *h.borrow_mut() {
        Obj::Set(s) => s.remove(v),
        Obj::SortedSet(items) => sorted_remove(items, v),
        Obj::Map(mp) => mp.remove(v),
        Obj::SortedMap(sm) => sm.remove(v),
        other => Err(format!("`remove` is not defined for a {}; use `erase`", kind_name(other))),
    }
}

pub fn obj_contains(h: &Handle, v: &Value) -> OResult<bool> {
    match &*h.borrow() {
        Obj::Set(s) => s.contains(v),
        Obj::SortedSet(items) => Ok(sorted_search(items, v)?.is_ok()),
        Obj::Map(mp) => mp.contains(v),
        Obj::SortedMap(sm) => sm.contains(v),
        other => Err(format!("`contains` is not defined for a {}; use `find`", kind_name(other))),
    }
}

pub fn obj_clear(h: &Handle) {
    match &mut *h.borrow_mut() {
        Obj::Vector(items) | Obj::SortedSet(items) => items.clear(),
        Obj::List(l) => l.clear(),
        Obj::Set(s) => s.clear(),
        Obj::Map(mp) => mp.clear(),
        Obj::SortedMap(sm) => sm.clear(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random order, so a failure is reproducible.
    fn lcg(state: &mut u64) -> i32 {
        *state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((*state >> 33) % 1000) as i32
    }

    impl SortedMapData {
        /// Every red-black invariant at once, plus the link and length
        /// bookkeeping the arena adds on top of them. Returns the black
        /// height so the recursion can check it is uniform.
        fn check(&self, n: i32, parent: i32) -> usize {
            if n == NIL {
                return 1;
            }
            assert_eq!(self.parent(n), parent, "node {} has a stale parent", n);
            if self.color(n) == Color::Red {
                assert_eq!(self.color(self.left(n)), Color::Black, "red node {} has a red child", n);
                assert_eq!(self.color(self.right(n)), Color::Black, "red node {} has a red child", n);
            }
            let l = self.check(self.left(n), n);
            let r = self.check(self.right(n), n);
            assert_eq!(l, r, "black height differs under node {}", n);
            l + if self.color(n) == Color::Black { 1 } else { 0 }
        }

        fn assert_valid(&self) {
            assert_eq!(self.color(self.root), Color::Black, "the root must be black");
            self.check(self.root, NIL);
            let keys = self.keys();
            assert_eq!(keys.len(), self.len, "`len` disagrees with the walk");
            for w in keys.windows(2) {
                assert_eq!(
                    cmp_values(&w[0], &w[1]).unwrap(),
                    Ordering::Less,
                    "the in-order walk is not ascending"
                );
            }
        }
    }

    /// Ascending insertion is the worst case for an unbalanced BST: without
    /// the fixup this would be a right spine, and the black-height check
    /// would fail immediately.
    #[test]
    fn stays_balanced_when_keys_arrive_in_order() {
        let mut m = SortedMapData::new();
        for i in 0..200 {
            m.set(Value::I32(i), Value::I32(i * 2)).unwrap();
            m.assert_valid();
        }
        assert_eq!(m.len(), 200);
        for i in 0..200 {
            // `Value` has no `PartialEq` on purpose: `value_eq` is the one
            // structural comparison, and it is what `find` uses.
            assert!(value_eq(&m.get(&Value::I32(i)).unwrap(), &Value::I32(i * 2)));
        }
    }

    /// Insert and delete interleaved, against a `Vec` kept as the oracle.
    /// Deletion is where a red-black tree goes wrong, and the two-children
    /// case only shows up once the tree is deep enough.
    #[test]
    fn matches_a_plain_list_through_random_churn() {
        let mut m = SortedMapData::new();
        let mut oracle: Vec<(i32, i32)> = Vec::new();
        let mut seed = 0x5eed_u64;
        for step in 0..3000 {
            let k = lcg(&mut seed);
            if step % 3 == 2 {
                let had = oracle.iter().position(|e| e.0 == k);
                let removed = m.remove(&Value::I32(k)).unwrap();
                assert_eq!(removed, had.is_some(), "`remove` disagreed on key {}", k);
                if let Some(at) = had {
                    oracle.remove(at);
                }
            } else {
                m.set(Value::I32(k), Value::I32(step)).unwrap();
                match oracle.iter_mut().find(|e| e.0 == k) {
                    Some(e) => e.1 = step,
                    None => oracle.push((k, step)),
                }
            }
            m.assert_valid();
        }
        oracle.sort();
        let got: Vec<(i32, i32)> = m
            .entries()
            .into_iter()
            .map(|(k, v)| match (k, v) {
                (Value::I32(k), Value::I32(v)) => (k, v),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(got, oracle);
    }

    /// A freed node index is reused, so churn must not grow the arena past
    /// the high-water mark of live entries.
    #[test]
    fn reuses_the_arena_after_deletes() {
        let mut m = SortedMapData::new();
        for round in 0..50 {
            for i in 0..20 {
                m.set(Value::I32(i), Value::I32(round)).unwrap();
            }
            for i in 0..20 {
                assert!(m.remove(&Value::I32(i)).unwrap());
            }
            assert_eq!(m.len(), 0);
        }
        assert!(m.nodes.len() <= 20, "the arena grew to {} for 20 live keys", m.nodes.len());
    }
}
