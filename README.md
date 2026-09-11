# binZ

**DISCLAIMER**: only HUMANS.md and SOUL.md contains stuff created by human. 

A small, strongly typed, compiled language. C/C++/JavaScript-shaped syntax,
Rust backend, bytecode artifact plus a stack VM that executes it.

Source files are `.binz`; compiled artifacts are `.binzc`.

This is the v0.1 scratch: primitives, functions as first-class values, C-like
structs and pointers, fixed arrays, and four heap containers.

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
| `c[i]` indexes every container | one spelling for "the element at i" |
| `len(c)` for every length | arrays, containers and `str` answer the same call |
| Sequences use `find` / `erase`, sets use `contains` / `remove` | positions and keys are different questions, so they get different verbs |
| `[T; N]` copies, `Vector<T>` aliases | one rule: frame storage is a value, heap storage is a handle |

## Install & use

```sh
cargo build --release

binz run   examples/tour.binz         # compile and execute
binz run   examples/containers.binz   # the container tour
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
`Set<T>` and `SortedSet<T>`.

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
print(cast<str>(apply(f, 2, 3)));
```

The type of a function is written the same way, so `function(i64, i64): i64`
is a type as well as the start of a declaration. `fn` and `->` are reserved
purely so that writing them produces a diagnostic pointing at the one spelling:

```
error: binZ declares functions with `function`, not `fn`
error: binZ writes the return type after `:`, not `->`
```

`print` is itself just a value of type `function(str): void`, so
`const say: function(str): void = print;` works. It is the only builtin with a
type you can write down — the container builtins below are generic, so they
can only be called.

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

Five of them, split by one question: does the storage live in the frame or on
the heap?

| Type | Storage | Assignment | Element type |
| --- | --- | --- | --- |
| `[T; N]` | frame, flat, `N` known at compile time | copies | anything, including structs and arrays |
| `Vector<T>` | heap, contiguous | aliases | any one-slot value |
| `LinkedList<T>` | heap, doubly linked nodes | aliases | any one-slot value |
| `Set<T>` | heap, hashed, insertion order | aliases | `i32` `i64` `f64` `bool` `str` |
| `SortedSet<T>` | heap, sorted, ascending order | aliases | `i32` `i64` `f64` `bool` `str` |

A fixed array is a value like a struct: assigning, passing or returning one
copies every element, and `&a[i]` is a real pointer into it. A heap container
is a *handle*: copying it shares the storage, the way a `*T` would, so a
function can fill a `Vector<T>` it was handed. `const` freezes the handle, not
its contents. `copy(c)` is the one way to get independent storage, and it is
deep — nested containers are copied too.

```c
var scores: [i32; 5] = [7, 3, 9, 1, 5];   // every element, like a struct literal
var grid:   [[i32; 3]; 2] = [[0; 3]; 2];  // `[x; N]` repeats one value
scores[0] = 10;

var v: Vector<str> = Vector<str>{};       // Name<T>{ ... }, like a struct literal
push(v, "one");
v[0] = "ONE";

var seen: Set<i32> = Set<i32>{3, 1, 3};   // two elements
var ranked: SortedSet<i32> = SortedSet<i32>{40, 10};
```

A heap container may sit in a struct field or an array element, where it is
still just a handle: copying the struct shares the container. For the same
reason `[Vector<i32>{}; 2]` repeats *one* handle into both slots — write the
elements out to get two containers.

`c[i]` reads an element of any of the five; it is assignable on the three
sequences, and rejected on a set, whose elements are its keys. An index is
always an `i32`. Iteration is a `while` over `len`, and it is deterministic
everywhere — a `Set` keeps insertion order, a `SortedSet` stays ascending:

```c
var i: i32 = 0;
while (i < len(seen)) {
    print(cast<str>(seen[i]));
    i = i + 1;
}
```

Operations are free functions, not methods, and each one belongs to exactly
one shape:

| Call | Works on | Result |
| --- | --- | --- |
| `len(c)` | every container, and `str` | `i32` |
| `find(c, x)` | `[T; N]`, `Vector`, `LinkedList` | index, or `-1` |
| `push(c, x)` | `Vector`, `LinkedList` | `void` |
| `pop(c)` | `Vector`, `LinkedList` | the last element |
| `insert(c, i, x)` | `Vector`, `LinkedList` | `void` |
| `erase(c, i)` | `Vector`, `LinkedList` | the removed element |
| `add(s, x)` | `Set`, `SortedSet` | `true` if it was new |
| `remove(s, x)` | `Set`, `SortedSet` | `true` if it was there |
| `contains(s, x)` | `Set`, `SortedSet` | `bool` |
| `clear(c)` | the four heap containers | `void` |
| `copy(c)` | the four heap containers | a deep copy |

`erase` takes a position and `remove` takes a key, so there is never a choice
about which one a container wants. Applying the wrong one says so:

```
error: `push` is not defined for `Set<i32>`; use `add`
error: `contains` is not defined for `Vector<i32>`; use `find`
error: `push` is not defined for `[i32; 1]`; a fixed array never changes size, use `Vector<i32>`
```

These builtins are generic over the element type, which binZ has no way to
write in a signature, so unlike `print` they are not first-class values. They
resolve last, so a program that declares its own `add` or `find` shadows them.

Out-of-range indexes, `pop` on an empty container and NaN used as a set key
all trap at runtime.

### Operators

`+ - * /` on numbers, `%` on integers, `+` also concatenates `str`.
`== != < <= > >=` compare two values of the same type and yield `bool`.
`&& || !` are boolean-only; `&&` and `||` short-circuit.
Both sides of a binary operator must already have the same type — there is no
promotion.

### Casting

```c
cast<i64>(x)     // between i32 / i64 / f64 (f64 -> int truncates)
cast<str>(x)     // any primitive to str; this is how you format for print
```

Anything else — `str` to a number, pointer casts, struct casts — is rejected.

### Control flow

```c
if (cond) { ... } else if (other) { ... } else { ... }
while (cond) { ... }
return expr;   // `return;` in a void function
```

A function that does not return `void` must return on every path.

## Implementation

```
src/lexer.rs      source -> tokens
src/parser.rs     tokens -> AST
src/compiler.rs   AST -> type check + bytecode, in a single pass
src/bytecode.rs   instruction set, .binzc serialization, disassembler
src/vm.rs         stack VM
src/obj.rs        heap containers behind Vector / LinkedList / Set / SortedSet
```

The compiler is one pass: because binZ has no inference beyond literal typing,
a type hint threaded downwards is enough to check and emit at the same time.

The VM has two stacks. `mem` holds call frames and is addressable at slot
granularity — that is what a `*T` actually points at, which is why `&local`
costs nothing. `stack` holds operands. Struct values are represented by the
address of their storage; assignment, argument passing and returns emit an
explicit `copy` of N slots, which is what gives structs value semantics. A
fixed array is the same kind of value, so `[T; N]` costs `N * sizeof(T)` slots
in the frame and indexing it is one bounds-checked address computation.

The four heap containers are the only things outside that model: each is one
slot holding a reference-counted handle, and storage is freed when the last
handle goes. `LinkedList<T>` really is a doubly linked list, held in a node
arena, and `l[i]` walks from whichever end is closer — indexing a list is
O(n), and that is the honest cost. `Set<T>` is a hash index over an
insertion-ordered vector; `SortedSet<T>` is a sorted vector searched by
bisection.

Runtime traps: division/remainder by zero, integer overflow, an index out of
range, `pop` on an empty container, NaN used as a set key, and dereferencing a
pointer into a frame that has already been popped.

### Artifact format

Little-endian: `"BINZ"` magic, version, string pool, function table (name,
parameter slot sizes, frame size, sret flag, code), entry index.

## Not in v0.1

Slices, user-written generics, closures, modules/imports, methods,
enums/unions, maps, a real standard library, bitwise operators, and unsigned
integers. `cast<str>` formats scalars only, so a container is printed by
iterating it. Heap containers cannot hold structs, and their elements have no
address. Dangling pointers are detectable but not prevented — pointer
lifetimes are C-like, not borrow-checked.
