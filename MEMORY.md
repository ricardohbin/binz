# MEMORY.md

Curated long-term memory. Raw per-day logs live in `memory/YYYY-MM-DD.md`.

## What binZ is

A strongly typed, compiled language with C/C++/JS-shaped syntax. Rust frontend
and backend, emitting a `.binzc` bytecode artifact executed by a stack VM in
`src/vm.rs`. Source files are `.binz`. Started 2026-09-10 from an empty
directory; v0.1 works end to end, containers landed 2026-09-11, and `HashMap`,
the standard library, `binz/map` and `SortedMap` all landed 2026-09-14.

**The one invariant: there is exactly ONE way to do one thing.** Every syntax
question gets settled by that rule first, before taste. When proposing anything
new, say which existing spelling it would duplicate — if it duplicates one, it
does not go in.

## Design decisions that are settled

- `function name(params): T` — the keyword is spelled out, and `:` introduces
  every type, including return types. `function(A, B): R` is the function type.
- `fn` and `->` are reserved tokens *only* to produce migration diagnostics.
- `const` / `var`, always with an explicit type annotation. No inference.
- No implicit conversions at all. `cast<T>(x)` is the only converter and the
  only way to format a value for `io.print`.
- Structs are C-like value types, flat layout, passed and returned by value
  (struct returns compile to a hidden destination pointer, so nothing dangles).
- Pointers: `&place` (only on `var` places) and `*p`. No `->`, write `(*p).x`.
- `while` is the only loop; no `+=`/`++`; conditions must be `bool`.
- Containers, 2026-09-11: `[T; N]` is a frame value and copies; `Vector<T>`,
  `LinkedList<T>`, `Set<T>`, `SortedSet<T>` are heap handles and alias.
  `c[i]` indexes all five, `container.size(c)` measures all five (a `str`
  answers `string.size`), and the verbs are split so no container has two
  ways to do one thing — **`erase` takes a position, `remove` takes a key**;
  `find` for sequences, `contains` for sets. `container.copy(c)` is the one
  deep copy. (Verbs were bare names until the stdlib landed, same day.) Full rationale in
  `memory/2026-09-11.md`.
- **Container/map names follow Redis, not Java — his word, 2026-09-14**:
  "the concept I am using to Set/SortedSet is likely one used in redis,
  forget about java Set Interface", then "redis has Hashes (that are
  HashMaps) right. keep this way". So `Set` is **not** "the hash
  implementation of an abstract Set interface" — that is the Java frame, and
  it is wrong here. binZ takes its collection names from Redis's data types:
  SET -> `Set`, ZSET -> `SortedSet`, HASH -> `HashMap`, and `SortedMap` for
  the ordered map Redis lacks. **This closes the `Set`-vs-`HashMap`
  asymmetry I flagged twice: there was never one.** Do not reopen it, in
  either direction — not `Set` -> `HashSet`, not `HashMap` -> `Map`.
- **Standard library, 2026-09-14**: `import binz/io;` and every member is
  reached as `io.print`. The binding is the last path segment, **always** —
  no alias, no wildcard, no bare import — so two modules may both define
  `find` and there is no resolution rule to learn. An import owns its binding
  program-wide (nothing else may be named `io`); imports come first in a file.
  Modules: `io` `string` `int` `float` `container` `map`. **Naming, his call
  2026-09-14: modules lowercase, members `camelCase` (`startsWith`,
  `canParse`), types `PascalCase`** — a test in `src/stdlib.rs` enforces it.
  **`size`, `find` and
  `contains` are per type family**, so `container.size(c)`,
  `string.size(s)` and `map.size(m)` — a `str` is not a container, and
  neither is a map. **A stdlib function is a
  first-class value exactly when its type is writable in binZ**: `io.print`
  is, `container.size` (six types) and `int.max` (two) are not. `int.parse`
  answers `i64` and traps; `int.canParse` guards it, as `contains` guards a
  map read. `cast<str>` is still the only formatter. Rationale in
  `memory/2026-09-14.md`.
- **Maps, 2026-09-14**: `HashMap<K, V>` and `SortedMap<K, V>` are the only
  two-argument types, so they share `Type::Map(MapKind, K, V)` rather than
  being `Kind`s. **`m[key] = value` is the only way in** (inserts or
  overwrites), reading an absent key traps because there is no `null`,
  `map.keys(m)` returns a `Vector<K>` and there is deliberately **no
  `values`**. A map answers `remove` / `contains`, never `erase` / `find`.
  **A map is a third type family, not a container**: all six verbs live in
  `binz/map` (`size` `contains` `remove` `keys` `clear` `copy`) and
  `container.size(m)` is an error naming `map.size`. `SortedMap` differs from
  `HashMap` in **exactly one thing** — `map.keys` answers ascending instead
  of insertion order — which is the only reason it is allowed to exist
  beside it. It is a real CLRS red-black tree over a node arena (`NIL = -1`,
  no sentinel), and its invariants are asserted after every step of 3000
  randomised operations in `src/obj.rs`. Rationale in `memory/2026-09-14.md`.

## Resolved, previously flagged

- The **`print` vs `len` shadowing inconsistency** (flagged 2026-09-11) is
  gone: module members are a separate namespace, so `container.add` never
  takes the word `add` from the program and nothing resolves "last".

## Repo mechanics

- **CI, 2026-09-15**: `.github/workflows/ci.yml`, one job — build, `cargo
  test`, then run every `examples/*.binz`. On push to `main` and every PR.
  He scoped it: "no release or other things". **`cargo fmt --check` and
  `cargo clippy -D warnings` are deliberately absent** — the tree is 141
  rustfmt diffs from default and clippy has 3 lints, one of which
  (`enum_variant_names`) wants a variant renamed, which is his call. Both
  are one step away if he wants them. Detail in `memory/2026-09-15.md`.

## Open, not yet decided by him

- **No `null`** — my call, not his. Un-C. He may want it back.
- **`[x; N]` next to `[a, b, c]`** — two array literal forms that overlap for
  `[0; 3]`. A real bend of the one-way rule; I took it because enumerating 64
  zeros is untenable. Flagged to him.
- **`import binz/map;` reserves the word `map` program-wide**, so `var map:
  i32` is an error. The import rule working as designed, but `map` is a more
  plausible variable name than `container` or `float`. Flagged 2026-09-14.
- **A `SortedMap` answers only the six `HashMap` verbs** — no `first`,
  `last` or range walk, which the tree could give cheaply. I did not invent
  spellings he had not asked for. Flagged 2026-09-14.
- **Contextual int literals** vs suffixes (`10i64`) — my call.
- Whether the `fn` / `->` reserved-token diagnostics stay forever.
- **`binz/` buys nothing yet** — every path is `binz/<one segment>`. It is his
  word, and it only pays off once user modules exist.
- `.binzc` artifact extension was my extrapolation from his `.binz` request.

## Not in v0.1

Slices, closures, user-written generics, **user modules** (`import` reaches
`binz/*` only), methods, enums, bitwise ops, unsigned ints.
The stdlib is a scratch: `io` has only `print` — no input, no stderr, no
files, no time, no random. `cast<str>` still formats
scalars only, so a container is printed by iterating it. Heap containers
cannot hold structs and their elements have no address. Pointer lifetimes are
C-like, not borrow-checked — dangling pointers trap at runtime but are not
prevented.

## How he wants me to work

- Direct answers. No "great question", no flattery, no preamble.
- Be resourceful before asking: check memory first, then act.
- English always, unless he explicitly asks for a resumo in Brazilian
  Portuguese — then switch back to English immediately after.
- Say "I don't know the answer to that question" rather than invent one. He
  would rather have a blunt no than a confident wrong.
- Don't ask permission for the startup routine. Just do it.
- Memories live in this project only. Never read or write memory elsewhere.
- Don't dump directory listings or secrets into chat.
- **Commits and PRs are his** (SOUL.md, 2026-09-11). Leave the work in the
  tree; do not commit and do not open PRs.
