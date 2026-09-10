# binZ

**DISCLAIMER**: only HUMANS.md and SOUL.md contains stuff created by human. 

A small, strongly typed, compiled language. C/C++/JavaScript-shaped syntax,
Rust backend, bytecode artifact plus a stack VM that executes it.

Source files are `.binz`; compiled artifacts are `.binzc`.

This is the v0.1 scratch: primitives, functions as first-class values, C-like
structs and pointers.

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

## Install & use

```sh
cargo build --release

binz run   examples/tour.binz    # compile and execute
binz build examples/tour.binz    # emit examples/tour.binzc
binz exec  examples/tour.binzc   # execute an artifact
binz dump  examples/tour.binzc   # disassemble
cargo test                       # end-to-end language tests
```

`main` must be `function main(): i32`, and its return value is the process exit code.

## The language

### Types

Primitives: `i32`, `i64`, `f64`, `bool`, `str`, and `void` (return type only).

Composites: `*T` (pointer), `function(A, B): R` (function), and `struct`s.

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
`const say: function(str): void = print;` works. It is the only builtin.

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
```

The compiler is one pass: because binZ has no inference beyond literal typing,
a type hint threaded downwards is enough to check and emit at the same time.

The VM has two stacks. `mem` holds call frames and is addressable at slot
granularity — that is what a `*T` actually points at, which is why `&local`
costs nothing. `stack` holds operands. Struct values are represented by the
address of their storage; assignment, argument passing and returns emit an
explicit `copy` of N slots, which is what gives structs value semantics.

Runtime traps: division/remainder by zero, integer overflow, and dereferencing
a pointer into a frame that has already been popped.

### Artifact format

Little-endian: `"BINZ"` magic, version, string pool, function table (name,
parameter slot sizes, frame size, sret flag, code), entry index.

## Not in v0.1

Arrays and slices, heap allocation, closures, generics, modules/imports,
methods, enums/unions, a real standard library, bitwise operators, and unsigned
integers. Dangling pointers are detectable but not prevented — pointer
lifetimes are C-like, not borrow-checked.
