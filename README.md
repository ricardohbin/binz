# binZ

**DISCLAIMER**: only HUMANS.md and SOUL.md contains stuff created by human. You can see some real thoughts there. Ah, all the PR and commits are made by me too (the human, in the case :P)

A small, strongly typed, compiled language. C/C++/JavaScript-shaped syntax,
Rust backend, bytecode artifact plus a stack VM that executes it.

Source files are `.binz`; compiled artifacts are `.binzc`.

This is the v0.1 scratch: primitives, functions as first-class values, C-like
structs and pointers, fixed arrays, four heap containers, two maps, and a
standard library reached through `import binz/<module>`.

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
| `c[i]` indexes every container | one spelling for "the element at i", by position for a sequence and by key for a map |
| `m[key] = value` is the only way into a map | it inserts when the key is new and overwrites when it is not |
| Modules are lowercase, members are `camelCase`, types are `PascalCase` | one convention per kind of name, and a test in `src/stdlib.rs` holds the line |
| An `as` rename is `camelCase` | it is two module names joined, and `camelCase` is how binZ joins words |
| A module name is always `module.member` | no bare import, no wildcard — `io.print` reads the same in every file |
| The binding is the last path segment, always | `binz/io` is `io`, `@root/utils/math.binz` is `math` |
| `as` only when two imports in a file are named the same — and then on every one of them | a rename is never a second spelling; it exists only where there is no first one |
| A rename is the directory plus the file name — `text/format.binz` is `textFormat` | not a choice: two people renaming the same clash write the same line |
| A file of the project is imported from `@root` | one path for one file, wherever it is written — no `../` |
| `container.size(c)` / `string.size(s)` / `map.size(m)` | one `size` per type family, rather than one name resolving three ways |
| Sequences use `find` / `erase`, sets use `contains` / `remove` | positions and keys are different questions, so they get different verbs |
| `[T; N]` copies, `Vector<T>` aliases | one rule: frame storage is a value, heap storage is a handle |
| A test is tagged `@test` and named for what it asserts | the tag says it is a test, so the name may not say it again |
| `test.equal` is the one assertion | `equal` states what was expected; `test.fail` covers what equality cannot say |
| A stub replaces a module's function, written at the top of the test | mocking without interfaces or an argument threaded through three layers |

## Install & use

```sh
cargo build --release

binz run   examples/tour.binz         # compile and execute
binz run   examples/containers.binz   # the container tour
binz run   examples/maps.binz         # the map tour
binz run   examples/stdlib.binz       # the standard library tour
binz run   examples/modules.binz      # a program split over three files
binz test  examples/orders.binz       # run the `@test` functions it can reach
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
`[T; N]` (fixed array), the heap containers `Vector<T>`, `LinkedList<T>`,
`Set<T>` and `SortedSet<T>`, and the maps `HashMap<K, V>` and
`SortedMap<K, V>`.

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

Five of them, split by one question: does the storage live in the frame or on
the heap? (The two maps are keyed rather than positional, and get their own
section [below](#maps).)

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
```

A heap container may sit in a struct field or an array element, where it is
still just a handle: copying the struct shares the container. For the same
reason `[Vector<i32>{}; 2]` repeats *one* handle into both slots — write the
elements out to get two containers.

`c[i]` reads an element of any of the five; it is assignable on the three
sequences and rejected on a set, whose elements are its keys. An index is
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
| `container.remove(s, x)` | `Set`, `SortedSet` | `true` if it was there |
| `container.contains(s, x)` | `Set`, `SortedSet` | `bool` |
| `container.clear(c)` | the four heap containers | `void` |
| `container.copy(c)` | the four heap containers | a deep copy |

`erase` takes a position and `remove` takes a key, so there is never a choice
about which one a container wants. Applying the wrong one says so:

```
error: `container.push` is not defined for `Set<i32>`; use `container.add`
error: `container.contains` is not defined for `Vector<i32>`; use `container.find`
error: `container.push` is not defined for `[i32; 1]`; a fixed array never changes size, use `Vector<i32>`
error: `container.add` is not defined for `HashMap<str, i32>`; write `m[key] = value`
error: `container.size` is not defined for `HashMap<str, i32>`; a map answers `map.size`
error: `container.size` needs a container, found `str`; a `str` answers `string.size`
```

A `str` is not a container, and neither is a map: a `str` is sized and searched
through `binz/string`, a map through `binz/map`. So `size`, `find` and
`contains` each mean one thing per type family instead of one name covering
three.

Out-of-range indexes, `pop` on an empty container and NaN used as a key all
trap at runtime.

### Maps

Two of them. Both are keyed by value, both are handles, and both answer the
same six verbs in `binz/map` — they differ in one thing, the order
`map.keys` hands back:

| Type | Storage | `map.keys` order | Cost |
| --- | --- | --- | --- |
| `HashMap<K, V>` | heap, hash index over an insertion-ordered vector | insertion | read and insert `O(1)`; `map.remove` is `O(n)` |
| `SortedMap<K, V>` | heap, red-black tree | ascending by key | `O(log n)`, including `map.remove` |

A `HashMap` pays for its insertion order: the hash index stores *positions*
into a vector, so erasing a key has to close the gap and reindex everything
after it. If a workload removes keys in bulk, `SortedMap` is the faster of
the two today, despite the `O(log n)`.

A key is `i32`, `i64`, `f64`, `bool` or `str`; a value is any one-slot value.
These are the only two types with two type arguments, and the only two
subscripted by something other than an `i32`.

```c
var ages: HashMap<str, i32> = HashMap<str, i32>{"ana": 31};  // key: value
ages["bruno"] = 27;                       // inserts
ages["ana"] = 32;                         // overwrites

var stock: SortedMap<str, i32> = SortedMap<str, i32>{"pear": 3, "apple": 12};
stock["fig"] = 7;                         // same spelling, ordered storage
```

`m[key] = value` is the only way in, so there is no `add` / `insert` / `put`
to choose between. Reading a key that is not there traps — binZ has no
`null`, so `map.contains(m, k)` is the guard. Walking a map is `map.keys(m)`,
which hands back a `Vector<K>`; there is deliberately no `values`, because
`m[k]` already answers that:

```c
const who: Vector<str> = map.keys(ages);
var i: i32 = 0;
while (i < container.size(who)) {       // the Vector is a container again
    io.print(who[i] + " = " + cast<str>(ages[who[i]]));
    i = i + 1;
}
```

| Call | Result |
| --- | --- |
| `map.size(m)` | `i32` |
| `map.contains(m, k)` | `bool` |
| `map.remove(m, k)` | `true` if the key was there |
| `map.keys(m)` | a `Vector<K>` |
| `map.clear(m)` | `void` |
| `map.copy(m)` | a deep copy |

A map answers `remove` and never `erase`, the same split the sets use: `erase`
takes a position and a map has none. Every other verb names the line that
works:

```
error: `binz/map` has no `add`: write `m[key] = value`
error: `binz/map` has no `find`: a map is keyed by value; use `map.contains`
error: `binz/map` has no `values`: read `m[key]` while walking `map.keys(m)`
error: `map.size` needs a map, found `Vector<i32>`; a container answers `container.size`
```

Reading an absent key traps, and so does NaN used as a key — a `SortedMap`
has to compare its keys, so it rejects NaN for exactly the reason a
`SortedSet` does.

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

That is the whole mechanism. There is no bare import, no wildcard, no second
spelling — `io.print` reads identically in every file that uses it,
and a reader never has to look up where a name came from. Every `import` goes
at the top of the file, one module per line, before the first `struct` or
`function`.

An import owns its binding for the whole file: once `binz/io` is in scope,
nothing else in that file may be called `io`. Conversely, a module member
never takes a name away from the program — `add` and `find` are ordinary
words, and `container.add` does not stop you writing your own
`function add(...)`.

| Module | Covers |
| --- | --- |
| `binz/io` | `print` |
| `binz/string` | `size` `at` `slice` `find` `contains` `startsWith` `endsWith` `upper` `lower` `trim` `repeat` `replace` `split` `join` |
| `binz/int` | `parse` `canParse` `abs` `min` `max` |
| `binz/float` | `parse` `canParse` `abs` `min` `max` `floor` `ceil` `round` `sqrt` `pow` `isNan` |
| `binz/container` | the eleven container verbs, [above](#containers) |
| `binz/map` | the six map verbs, [above](#maps) |
| `binz/random` | `f64` `i32` — [below](#binzrandom) |
| `binz/test` | `equal` `calls` `fail`, reachable only from a test — [below](#tests) |

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

`container.size` answers for five different types, `map.keys` for two and
`int.max` for two, and binZ has no user-written generics yet, so those have no
type to hold. They are resolved at the call site instead.

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

### binz/random

A member is named for the type it answers, because that is the whole of what
it is:

```c
import binz/random;

const r: f64 = random.f64();       // 0.0 <= r < 1.0, never 1.0
const d: i32 = random.i32(1, 6);   // a die -- both ends included
```

`random.i32` traps on a backwards range, since an empty range has no value to
answer with and binZ has no `null`. Both members are monomorphic, so both are
ordinary function values.

The generator is seeded from the operating system once per process and
**there is no way to set the seed**. Code that has to be predictable does not
replay a random number — it [stubs](#stubbing-a-module) the module function
that reads one. The standard library is not stubbable, so the seam is always
your own function:

```c
// rates.binz -- the dependency a test cannot predict
function lookup(country: str): f64 {
    return random.f64();
}
```

```c
@test function appliesTheRate(): void {
    stub rates.lookup(country: str): f64 { return 0.5; }

    test.equal(total(100.0, "br"), 50.0);
}
```

Without that stub, all a test can say about `total(100.0, "br")` is that it
lands between `0.0` and `100.0` — which is what `examples/orders.binz` shows
side by side.

### When a name is missing

Every diagnostic carries the line the program should have written:

```
error: `print` is in `binz/io`; write `io.print` after `import binz/io;`
error: `io` is not imported; add `import binz/io;` at the top of the file
error: `binz/string` has no `push`; it is in `binz/container`
error: `binz/map` has no `values`: read `m[key]` while walking `map.keys(m)`
error: there is no module `binz/json`; binZ has io, string, int, float, container, map
error: cannot read `@root/nope.binz`: No such file or directory
error: `@root/math.binz` has no function `cube`
error: `math` is the imported module `@root/math.binz`, so it cannot also be a variable
```

A verb a module refuses on purpose gets the line to write instead, rather
than a pointer at another module that would refuse it too. A program is a
graph of files, so every diagnostic names the file its span belongs to — which
is not always the one on the command line.

`len` was the spelling before the standard library had modules, so it gets its
own:

```
error: `len` is now `size`, in `binz/string`, `binz/container` or `binz/map`;
write `string.size`, `container.size` or `map.size` after
`import binz/string; import binz/container; import binz/map;`
```

## Local modules

A program can be more than one file. A file of this project is imported from
`@root`, the directory of the file handed to `binz`:

```c
// examples/modules.binz
import binz/io;
import @root/modules/geometry.binz;

function main(): i32 {
    io.print(cast<str>(geometry.square(7)));
    return 0;
}
```

`@root` is the anchor because a path should mean the same thing wherever it is
written. There is no `../`, no path relative to the importing file, and no way
to reach outside the root — one file, one spelling.

Everything else is the rule already in force for `binz/io`. **The binding is
the file's own name**, always, with no way to rename it: `geometry.binz` binds
`geometry`. That is why a module file is named with one lowercase word — the
name is not decoration, it is the identifier the importing file will type, so
`some-module.binz` is refused where it is written:

```
error: `-` cannot appear in a module file name: the name is the binding, the
`math` you would write in `math.square`, so it is one lowercase word
```

A module is an ordinary `.binz` file, with three consequences:

- **It exports every function it defines, and nothing else.** There is no
  `export` keyword and no `pub` — a module's surface is what you can read at
  the top of it. Its structs stay inside it, including in the signature of an
  exported function, which the importing file would have no way to write down.
- **It imports what it uses.** The file that imports it is not a scope, so a
  module that calls `io.print` writes `import binz/io;` itself. A module may
  import other local modules, including ones it shares with the entry file: a
  file reached twice through the graph is still compiled once.
- **It does not define `main`.** `main` is the program; a module is a library.

Names are per file. Two files may both define `add`, and neither shadows the
other — `add` is this file's, `alpha.add` is the other's. A module function is
an ordinary value, by the same rule that makes `io.print` one: its type is
writable in binZ.

```c
const f: function(i32): i32 = geometry.square;
```

An import cycle is an error, and names the whole chain:

```
error: import cycle: @root/a.binz -> @root/b.binz -> @root/a.binz
```

### `as`, only where it is forced

Two directories may hold two files of the same name. That is the one situation
`as` exists for — and then **every one of them is renamed**, so a name is never
the default for one import and a rename for another:

```c
import @root/modules/text/format.binz   as textFormat;
import @root/modules/number/format.binz as numberFormat;
```

**The rename is not a choice either.** It is the module's directory and its own
name, joined the way binZ joins words everywhere else — `text/format.binz` is
`textFormat` and nothing else. Two people renaming the same clash write the
same line, and the diagnostic names the spelling rather than asking you to
invent one:

```
error: `@root/modules/text/format.binz` and `@root/modules/number/format.binz`
are named `format`; when two or more imports in a file are named the same,
every one of them is renamed -- write
`import @root/modules/text/format.binz as textFormat;`

error: the rename of `@root/modules/text/format.binz` is `textFormat`, not
`textformat1`: a rename is the directory and the file name joined, so it is
not a choice either
```

Two contested imports always differ in the directory — two files of the same
name in one directory *are* one file — so the rename is unique without looking
at what else the file imports. A file directly under the anchor borrows the
anchor's name: `@root/format.binz` is `rootFormat`.

Outside that situation `as` is refused, because a module would then have two
spellings — the bare name and the rename — which is the thing the whole import
design exists to avoid:

```
error: `as` renames a module only when two or more imports in a file are named
the same; `@root/utils/math.binz` is the only `math` in this file, so it is
imported as `math`
```

No two standard library modules are named the same, so `as` is never legal on
one: `io.print` reads identically in every file of every program. A rename
always carries a capital at the join, so it can never collide with a module
name — those are one lowercase word.

A clash is per file. The same module is `math` in a file that imports only it,
and renamed in a file that imports both.

## Tests

A test lives in the file it tests, tagged `@test`, and `binz test` is the only
thing that runs one:

```c
import binz/test;

function double(n: i32): i32 {
    return n + n;
}

@test function doublesPositives(): void {
    test.equal(double(21), 42);
}
```

```sh
binz test examples/orders.binz
```

`binz test` compiles the file it is given **and everything that file imports**,
and runs every `@test` in the graph — so pointing it at the entry file runs the
project. `main` is neither required nor run, which is also what lets a module
be tested on its own: `binz test src/math.binz` works, and a module still may
not define `main`.

`binz build` and `binz run` do not compile a test at all. A test is not in the
artifact, weighs nothing, and cannot be called — by its own file or by one
importing it. The cost is the other half of that: a test that no longer
compiles stops `binz test`, and nothing else.

A test takes no arguments and answers `void`. It reports by failing, and there
is no caller to read a result. **A test name may not start with `test`** — the
tag above it already says that:

```
error: a test name cannot start with `test`: the `@test` tag above it already
says that, so write `sumsPositives`
```

### Asserting

| Written | Means |
| --- | --- |
| `test.equal(actual, expected)` | fails unless the two are `==`, printing both |
| `test.fail(message)` | fails outright — for what equality cannot state |
| `test.calls(math.add)` | how many times that function was called in this test |

`test.equal` compares whatever `==` compares, and both sides are one type:
binZ converts nothing on its own here either. A failure prints both sides
with `cast<str>`, the one formatter the language has:

```
  FAIL  doublesNegatives
        expected `0`, found `-2`
```

There is deliberately no `notEqual`, no `assert(condition)` and no `check` —
`test.equal(x > 10, true)` states an expectation in the one spelling there is.
`binz/test` is reachable only from a test body or a stub body; a program never
runs one of these, so outside a test there is nothing for them to answer to.

### Stubbing a module

`stub` replaces a function of an imported module for the length of one test:

```c
import binz/test;
import @root/rates.binz;

function total(amount: f64, country: str): f64 {
    return amount * rates.lookup(country);
}

@test function appliesTheRate(): void {
    stub rates.lookup(country: str): f64 { return 0.5; }

    test.equal(total(100.0, "br"), 50.0);
    test.equal(test.calls(rates.lookup), 1);
}
```

It replaces the **function**, not the call. Every path that reaches
`rates.lookup` lands in the body written here, however deep in the import graph
the call was written — which is the point: no interface, no dependency passed
in, nothing threaded through three layers to reach the one place that has to
lie. It is undone when the test ends, because each test runs in a machine of
its own.

The rules, and what each one is for:

| Rule | Why |
| --- | --- |
| Every `stub` goes at the top of the test, before its first statement | what a test replaced is read once, not hunted for |
| The signature is written out and must match the real one | a stub that has drifted from what it fakes is the one bug a test library must not hide |
| One function, one stub per test | no guessing which body wins |
| Only a module of this project — never `binz/io` | `io.print` means the same thing in every program |
| Only inside an `@test` function | outside a test there is no length to replace it for |
| A stub cannot call the function it replaces | the call would land back in the stub — there is no calling through |

Since binZ has no globals and no closures, a stub body cannot record anything
— which is why counting is the library's job. `test.calls` answers how many
times a function of the program was called during this test, stubbed or not,
and it is asserted with the same `test.equal` as everything else. A standard
library call has no function id to count, so `test.calls(io.print)` is refused
rather than answering zero forever.

## Implementation

```
src/lexer.rs      source -> tokens
src/loader.rs     entry file -> the graph of files it imports
src/parser.rs     tokens -> AST
src/compiler.rs   AST -> type check + bytecode, in a single pass
src/stdlib.rs     the module table: what binz/io, binz/string, ... contain
src/bytecode.rs   instruction set, .binzc serialization, disassembler
src/vm.rs         stack VM
src/obj.rs        heap storage: Vector / LinkedList / Set / SortedSet / HashMap / SortedMap
```

The compiler is one pass: because binZ has no inference beyond literal typing,
a type hint threaded downwards is enough to check and emit at the same time.

The standard library is compiled in, not written in binZ, and `src/stdlib.rs`
is its whole table. `binz/random` is the one member with state: an xorshift64*
word in the VM, seeded from the platform through `RandomState`, so the
language still has no dependencies. A monomorphic member is a *native*: its index is a
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
insertion-ordered vector of pairs; and `SortedMap<K, V>` is a genuine
red-black tree over a node arena, with `-1` for nil rather than a sentinel
node, so deletion carries the parent explicitly where CLRS reads
`nil.parent`. Both maps therefore make `map.keys(m)` deterministic across
runs. `get` and `set` share the `c[i]` opcodes: the VM reads the value on the
stack as a position for a sequence and as a key for a map.

The red-black invariants are checked directly rather than trusted: the unit
tests in `src/obj.rs` walk the tree after every step of 3000 randomised
inserts and deletes, asserting the root is black, no red node has a red
child, every path carries the same black height, parent links agree, and the
in-order walk is ascending — against a plain `Vec` kept as the oracle.

`binz test` is the same compiler in a second mode. `@test` functions and the
stub bodies inside them are the only difference: in run mode they are never
declared, so they cannot be reached and cannot be emitted. A stub costs one
instruction, `OP_STUB`, which pops a function value and records a redirect —
every call already goes through a function id, so redirecting one is what
makes a stub reach calls written anywhere in the graph. The redirect table is
allocated only when a stub installs one, and the call counts only in a test
run, so an ordinary program pays nothing for either existing.

Runtime traps: division/remainder by zero, integer overflow, an index out of
range, a map key that is not present, `pop` on an empty container, NaN
used as a key, and dereferencing a pointer into a frame that has already been
popped.

### Artifact format

Little-endian: `"BINZ"` magic, version, string pool, function table (name,
parameter slot sizes, frame size, sret flag, code), entry index, test table.
The entry index is optional and the test table is empty in everything
`binz build` writes: both exist because a `binz test` compile has no `main`
and carries the tests it found.

## Not in v0.1

Slices, user-written generics, closures, exported types (a module exports its
functions only), methods, enums/unions, bitwise operators, and
unsigned integers. The standard library is a scratch: no file or process I/O
beyond `io.print`, no time. `binz/random` cannot be seeded, so a
failing random case cannot be replayed. A test run has no setup, no teardown
and no filter — `binz test <file>` runs everything it can reach. `cast<str>` formats
scalars only, so a
container is printed by iterating it. Heap containers cannot hold structs, and their elements have no
address. Dangling pointers are detectable but not prevented — pointer
lifetimes are C-like, not borrow-checked.
