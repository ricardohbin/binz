# binZ

**DISCLAIMER**: only HUMANS.md and SOUL.md contains stuff created by human. You can see some real thoughts there. Ah, all the PR and commits are made by me too (the human, in the case :P)

A small, strongly typed, compiled language. C/C++/JavaScript-shaped syntax,
Rust backend, bytecode artifact plus a stack VM that executes it.

Source files are `.binz`; compiled artifacts are `.binzc`.

This is the v0.1 scratch: primitives, functions as first-class values, C-like
structs and pointers, fixed arrays, five heap containers, and a standard
library reached through `import binz/<module>`.

## Guiding rule: one way to do one thing

Every design decision below falls out of that rule.

| Decision | Why |
| --- | --- |
| Types are always written out — no inference | one spelling for a declaration |
| No implicit conversions, ever (not even `i32` → `i64`) | `cast<T>(x)` is the only converter |
| `cast<T>(x)` is the only cast syntax | no C casts, no constructor casts, no `as` |
| `function` declares a function — `fn` is rejected | one keyword, spelled out |
| `:` introduces every type, including return types | no `->` in declarations |
| No `->` operator; write `(*p).field` | one way to reach through a pointer |
| Only `while` for loops | `for`/`do`/`foreach` are the same loop |
| No `+=`, `++`, `--` | `x = x + 1` |
| Conditions are parenthesised and must be `bool` | no truthiness, no `if x = 0` trap |
| Struct literals list every field, in declaration order | one shape per struct |
| `const` / `var` — no third form | immutable by default in spirit |
| No `null` | a `*T` always points at something |
| `c[i]` indexes every container | one spelling for "the element at i", by position for a sequence and by key for a `HashMap` |
| `m[key] = value` is the only way into a `HashMap` | it inserts when the key is new and overwrites when it is not |
| Modules are lowercase, members are `camelCase`, types are `PascalCase` | one convention per kind of name, and a test in `src/stdlib.rs` holds the line |
| A stdlib name is always `module.member` | no bare import, no alias, no wildcard — `io.print` reads the same in every file |
| The binding is the last path segment, always | `binz/io` is `io`, and there is no way to rename it |
| `container.size(c)` / `string.size(s)` | one `size` per type family, rather than one name resolving two ways |
| Sequences use `find` / `erase`, sets use `contains` / `remove` | positions and keys are different questions, so they get different verbs |
| `[T; N]` copies, `Vector<T>` aliases | one rule: frame storage is a value, heap storage is a handle |

## Install & use

```sh
cargo build --release

binz run   examples/tour.binz         # compile and execute
binz run   examples/containers.binz   # the container tour
binz run   examples/stdlib.binz       # the standard library tour
binz build examples/tour.binz         # emit examples/tour.binzc
binz exec  examples/tour.binzc        # execute an artifact
binz dump  examples/tour.binzc        # disassemble
cargo test                            # end-to-end language tests
```

`main` must be `function main(): i32`, and its return value is the process exit code.

## The language

### Types

Primitives: `i32`, `i64`, `f64`, `bool`, `str`, and `void` (return type only).

Composites: `*T` (pointer), `function(A, B): R` (function), `struct`s,
`[T; N]` (fixed array), and the heap containers `Vector<T>`, `LinkedList<T>`,
`Set<T>`, `SortedSet<T>` and `HashMap<K, V>`.

### Variables

```c
const limit: i64 = 10;   // immutable
var   count: i64 = 0;    // mutable
```

The type annotation is mandatory. An integer literal takes the type it is
assigned to (`i32` when there is nothing to go on); a float literal is always
`f64`, so `const x: f64 = 3;` is an error — write `3.0`.

### Functions

Declared at the top level, in any order (forward and mutual recursion work).

```c
function add(a: i64, b: i64): i64 {
    return a + b;
}
```

Functions are values, so they have a type and can be stored and passed:

```c
function apply(op: function(i64, i64): i64, a: i64, b: i64): i64 {
    return op(a, b);
}

const f: function(i64, i64): i64 = add;
io.print(cast<str>(apply(f, 2, 3)));
```

The type of a function is written the same way, so `function(i64, i64): i64`
is a type as well as the start of a declaration. `fn` and `->` are reserved
purely so that writing them produces a diagnostic pointing at the one spelling:

```
error: binZ declares functions with `function`, not `fn`
error: binZ writes the return type after `:`, not `->`
```

A standard-library function is a value under exactly the same rule — see
[The standard library](#the-standard-library).

Parameters behave like `var`: they are mutable copies inside the function.

### Structs and pointers

Structs are value types, laid out flat like in C. Nested structs are inlined;
a struct that contains itself by value is rejected (use a pointer).

```c
struct Point { x: i64, y: i64 }

var a: Point = Point { x: 1, y: 2 };
var b: Point = a;        // full copy, not an alias
```

They are passed and returned by value too — a struct return is compiled with a
hidden destination pointer, so no copy is left dangling.

`&place` takes an address, `*p` dereferences. You can only take the address of
a `var` place, and there is no `->`:

```c
function shift(p: *Point, dx: i64): void {
    (*p).x = (*p).x + dx;
    return;
}

shift(&a, 10);

var pn: *i64 = &a.y;     // pointers to fields work
*pn = 99;
```

### Containers

Six of them, split by one question: does the storage live in the frame or on
the heap?

| Type | Storage | Assignment | Element type |
| --- | --- | --- | --- |
| `[T; N]` | frame, flat, `N` known at compile time | copies | anything, including structs and arrays |
| `Vector<T>` | heap, contiguous | aliases | any one-slot value |
| `LinkedList<T>` | heap, doubly linked nodes | aliases | any one-slot value |
| `Set<T>` | heap, hashed, insertion order | aliases | `i32` `i64` `f64` `bool` `str` |
| `SortedSet<T>` | heap, sorted, ascending order | aliases | `i32` `i64` `f64` `bool` `str` |
| `HashMap<K, V>` | heap, hashed, insertion order | aliases | key: `i32` `i64` `f64` `bool` `str`; value: any one-slot value |

A fixed array is a value like a struct: assigning, passing or returning one
copies every element, and `&a[i]` is a real pointer into it. A heap container
is a *handle*: copying it shares the storage, the way a `*T` would, so a
function can fill a `Vector<T>` it was handed. `const` freezes the handle, not
its contents. `container.copy(c)` is the one way to get independent storage,
and it is deep — nested containers are copied too.

```c
var scores: [i32; 5] = [7, 3, 9, 1, 5];   // every element, like a struct literal
var grid:   [[i32; 3]; 2] = [[0; 3]; 2];  // `[x; N]` repeats one value
scores[0] = 10;

var v: Vector<str> = Vector<str>{};       // Name<T>{ ... }, like a struct literal
container.push(v, "one");
v[0] = "ONE";

var seen: Set<i32> = Set<i32>{3, 1, 3};   // two elements
var ranked: SortedSet<i32> = SortedSet<i32>{40, 10};

var ages: HashMap<str, i32> = HashMap<str, i32>{"ana": 31};  // key: value
ages["bruno"] = 27;                       // inserts
ages["ana"] = 32;                         // overwrites
```

`HashMap<K, V>` is the one type with two arguments, and the one container
subscripted by something other than an `i32`. Reading a key that is not there
traps — binZ has no `null`, so `container.contains(m, k)` is the guard.
Walking a map is `container.keys(m)`, which hands back a `Vector<K>` in
insertion order; there is no `values`, because `m[k]` already answers that:

```c
const who: Vector<str> = container.keys(ages);
var i: i32 = 0;
while (i < container.size(who)) {
    io.print(who[i] + " = " + cast<str>(ages[who[i]]));
    i = i + 1;
}
```

A heap container may sit in a struct field or an array element, where it is
still just a handle: copying the struct shares the container. For the same
reason `[Vector<i32>{}; 2]` repeats *one* handle into both slots — write the
elements out to get two containers.

`c[i]` reads an element of any of the six; it is assignable on the three
sequences and on a `HashMap`, and rejected on a set, whose elements are its
keys. An index is
always an `i32`. Iteration is a `while` over `container.size`, and it is
deterministic everywhere — a `Set` keeps insertion order, a `SortedSet` stays
ascending:

```c
var i: i32 = 0;
while (i < container.size(seen)) {
    io.print(cast<str>(seen[i]));
    i = i + 1;
}
```

The verbs live in `binz/container`, and each one belongs to exactly one shape:

| Call | Works on | Result |
| --- | --- | --- |
| `container.size(c)` | every container | `i32` |
| `container.find(c, x)` | `[T; N]`, `Vector`, `LinkedList` | index, or `-1` |
| `container.push(c, x)` | `Vector`, `LinkedList` | `void` |
| `container.pop(c)` | `Vector`, `LinkedList` | the last element |
| `container.insert(c, i, x)` | `Vector`, `LinkedList` | `void` |
| `container.erase(c, i)` | `Vector`, `LinkedList` | the removed element |
| `container.add(s, x)` | `Set`, `SortedSet` | `true` if it was new |
| `container.remove(s, x)` | `Set`, `SortedSet`, `HashMap` | `true` if it was there |
| `container.contains(s, x)` | `Set`, `SortedSet`, `HashMap` | `bool` |
| `container.keys(m)` | `HashMap` | a `Vector<K>`, in insertion order |
| `container.clear(c)` | the five heap containers | `void` |
| `container.copy(c)` | the five heap containers | a deep copy |

`erase` takes a position and `remove` takes a key, so there is never a choice
about which one a container wants. Applying the wrong one says so:

```
error: `container.push` is not defined for `Set<i32>`; use `container.add`
error: `container.contains` is not defined for `Vector<i32>`; use `container.find`
error: `container.push` is not defined for `[i32; 1]`; a fixed array never changes size, use `Vector<i32>`
error: `container.add` is not defined for `HashMap<str, i32>`; write `m[key] = value`
error: `container.size` needs a container, found `str`; a `str` answers `string.size`
```

A `str` is not a container: it is sized and searched through `binz/string`, so
`size`, `find` and `contains` each mean one thing per type instead of one name
covering two.

Out-of-range indexes, a `HashMap` key that is not present, `pop` on an empty
container and NaN used as a key all trap at runtime.

### Operators

`+ - * /` on numbers, `%` on integers, `+` also concatenates `str`.
`== != < <= > >=` compare two values of the same type and yield `bool`.
`&& || !` are boolean-only; `&&` and `||` short-circuit.
Both sides of a binary operator must already have the same type — there is no
promotion.

### Casting

```c
cast<i64>(x)     // between i32 / i64 / f64 (f64 -> int truncates)
cast<str>(x)     // any primitive to str; this is how you format for io.print
```

Anything else — `str` to a number, pointer casts, struct casts — is rejected.

### Control flow

```c
if (cond) { ... } else if (other) { ... } else { ... }
while (cond) { ... }
return expr;   // `return;` in a void function
```

A function that does not return `void` must return on every path.

## The standard library

Nothing is in scope by default. A module is made visible with `import`, and
every one of its members is then reached through the **last segment of the
path** — always, with no way to rename it:

```c
import binz/io;
import binz/string;
import binz/container;

function main(): i32 {
    io.print(string.upper("binz"));
    return 0;
}
```

A module name is one lowercase word, a member is `camelCase` (`startsWith`,
`canParse`), and a type is `PascalCase` (`Vector<T>`, `HashMap<K, V>`) — one
convention per kind of name, so nothing has to be looked up.

That is the whole mechanism. There is no bare import, no alias, no wildcard,
no second spelling — `io.print` reads identically in every file that uses it,
and a reader never has to look up where a name came from. Every `import` goes
at the top of the file, one module per line, before the first `struct` or
`function`.

An import owns its binding for the whole program: once `binz/io` is in scope,
nothing else may be called `io`. Conversely, a module member never takes a
name away from the program — `add` and `find` are ordinary words, and
`container.add` does not stop you writing your own `function add(...)`.

| Module | Covers |
| --- | --- |
| `binz/io` | `print` |
| `binz/string` | `size` `at` `slice` `find` `contains` `startsWith` `endsWith` `upper` `lower` `trim` `repeat` `replace` `split` `join` |
| `binz/int` | `parse` `canParse` `abs` `min` `max` |
| `binz/float` | `parse` `canParse` `abs` `min` `max` `floor` `ceil` `round` `sqrt` `pow` `isNan` |
| `binz/container` | the twelve container verbs, [above](#containers) |

### Which members are values

A standard-library function is a first-class value **exactly when its type can
be written down in binZ**. Nothing else distinguishes the two groups:

```c
const shout: function(str): str = string.upper;   // fine
const say:   function(str): void = io.print;      // fine
const n:     function(str): i32  = container.size;
// error: `container.size` is generic over the type it is given, which binZ
// cannot write in a signature, so it is not a value; call it directly
```

`container.size` answers for six different types and `int.max` for two, and
binZ has no user-written generics yet, so those have no type to hold. They are
resolved at the call site instead.

### binz/string

`str` is a sequence of characters, and every position the module hands out is
a **character** index, so `size`, `at`, `slice` and `find` agree on text that
is not ASCII:

```c
const s: str = "maçã";
string.size(s);            // 4
string.at(s, 2);           // "ç"  -- there is no char type, so a 1-char str
string.slice(s, 0, 2);     // "ma" -- half-open, like every range in binZ
string.find(s, "çã");      // 2, or -1 -- same convention as container.find
```

`string.split` hands back a `Vector<str>` and `string.join` takes one, so the
container module takes over the moment there is more than one string:

```c
const parts: Vector<str> = string.split("a,b,c", ",");
string.join(parts, " | ");                 // "a | b | c"
container.size(parts);                     // 3
```

Formatting a value as text is still `cast<str>(x)`, not a `string` function —
`cast` is the one converter and this module does not duplicate it.

### binz/int and binz/float

There is no `null` and no optional, so `parse` **traps** on input it cannot
read and `canParse` is the guard — the same shape as `container.contains` in
front of a `HashMap` read:

```c
if (int.canParse(text)) {
    const n: i32 = cast<i32>(int.parse(text));
}
```

`int.parse` answers in the widest integer, `i64`, and `cast<i32>` narrows it,
because `cast` is already the one converter. `abs`, `min` and `max` are
generic over `i32` and `i64` and answer in the width they were handed, so
neither width is pushed through a cast to use them. Handing one an `f64` says
where to go instead:

```
error: `int.abs` needs an `i32` or an `i64`, found `f64`; an `f64` answers `float.abs`
```

### When a name is missing

Every diagnostic carries the line the program should have written:

```
error: `print` is in `binz/io`; write `io.print` after `import binz/io;`
error: `io` is not imported; add `import binz/io;` at the top of the file
error: `binz/string` has no `push`; it is in `binz/container`
error: there is no module `binz/json`; binZ has io, string, int, float, container
```

`len` was the spelling before the standard library had modules, so it gets its
own:

```
error: `len` is now `size`, in `binz/string` or `binz/container`; write
`string.size` or `container.size` after `import binz/string; import binz/container;`
```

## Implementation

```
src/lexer.rs      source -> tokens
src/parser.rs     tokens -> AST
src/compiler.rs   AST -> type check + bytecode, in a single pass
src/stdlib.rs     the module table: what binz/io, binz/string, ... contain
src/bytecode.rs   instruction set, .binzc serialization, disassembler
src/vm.rs         stack VM
src/obj.rs        heap containers behind Vector / LinkedList / Set / SortedSet / HashMap
```

The compiler is one pass: because binZ has no inference beyond literal typing,
a type hint threaded downwards is enough to check and emit at the same time.

The standard library is compiled in, not written in binZ, and `src/stdlib.rs`
is its whole table. A monomorphic member is a *native*: its index is a
bytecode operand, it is pushed as an ordinary function value, and the VM
executes it in `call_native`. A generic member is a *form*: the compiler
resolves it against the type of its first argument and emits `OP_BUILTIN`.
That split is why `io.print` is a value and `container.size` is not — it is
the one difference between the two tables.

The VM has two stacks. `mem` holds call frames and is addressable at slot
granularity — that is what a `*T` actually points at, which is why `&local`
costs nothing. `stack` holds operands. Struct values are represented by the
address of their storage; assignment, argument passing and returns emit an
explicit `copy` of N slots, which is what gives structs value semantics. A
fixed array is the same kind of value, so `[T; N]` costs `N * sizeof(T)` slots
in the frame and indexing it is one bounds-checked address computation.

The five heap containers are the only things outside that model: each is one
slot holding a reference-counted handle, and storage is freed when the last
handle goes. `LinkedList<T>` really is a doubly linked list, held in a node
arena, and `l[i]` walks from whichever end is closer — indexing a list is
O(n), and that is the honest cost. `Set<T>` is a hash index over an
insertion-ordered vector; `SortedSet<T>` is a sorted vector searched by
bisection; `HashMap<K, V>` is the same shape as `Set<T>`, a hash index over an
insertion-ordered vector of pairs, which is what makes `container.keys(m)` deterministic
across runs. `get` and `set` share the `c[i]` opcodes: the VM reads the value
on the stack as a position for a sequence and as a key for a map.

Runtime traps: division/remainder by zero, integer overflow, an index out of
range, a `HashMap` key that is not present, `pop` on an empty container, NaN
used as a key, and dereferencing a pointer into a frame that has already been
popped.

### Artifact format

Little-endian: `"BINZ"` magic, version, string pool, function table (name,
parameter slot sizes, frame size, sret flag, code), entry index.

## Not in v0.1

Slices, user-written generics, closures, user-defined modules (`import` reaches
`binz/*` only), methods, enums/unions, an ordered map, bitwise operators, and
unsigned integers. The standard library is a scratch: no file or process I/O
beyond `io.print`, no time, no random. `cast<str>` formats scalars only, so a
container is printed by iterating it. Heap containers cannot hold structs, and their elements have no
address. Dangling pointers are detectable but not prevented — pointer
lifetimes are C-like, not borrow-checked.
