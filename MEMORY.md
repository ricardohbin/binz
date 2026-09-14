# MEMORY.md

Curated long-term memory. Raw per-day logs live in `memory/YYYY-MM-DD.md`.

## What binZ is

A strongly typed, compiled language with C/C++/JS-shaped syntax. Rust frontend
and backend, emitting a `.binzc` bytecode artifact executed by a stack VM in
`src/vm.rs`. Source files are `.binz`. Started 2026-09-10 from an empty
directory; v0.1 works end to end, containers landed 2026-09-11, `HashMap`
2026-09-14, the standard library 2026-09-14.

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
- **Standard library, 2026-09-14**: `import binz/io;` and every member is
  reached as `io.print`. The binding is the last path segment, **always** —
  no alias, no wildcard, no bare import — so two modules may both define
  `find` and there is no resolution rule to learn. An import owns its binding
  program-wide (nothing else may be named `io`); imports come first in a file.
  Modules: `io` `string` `int` `float` `container`. **Naming, his call
  2026-09-14: modules lowercase, members `camelCase` (`startsWith`,
  `canParse`), types `PascalCase`** — a test in `src/stdlib.rs` enforces it.
  **`size`, `find` and
  `contains` are per type family**, so `container.size(c)` and
  `string.size(s)` — a `str` is not a container. **A stdlib function is a
  first-class value exactly when its type is writable in binZ**: `io.print`
  is, `container.size` (six types) and `int.max` (two) are not. `int.parse`
  answers `i64` and traps; `int.canParse` guards it, as `contains` guards a
  map read. `cast<str>` is still the only formatter. Rationale in
  `memory/2026-09-14.md`.
- `HashMap<K, V>`, 2026-09-14: the only two-argument type, so it is its own
  `Type::Map` rather than a fifth `Kind`. **`m[key] = value` is the only way
  in** (inserts or overwrites), reading an absent key traps because there is
  no `null`, `container.keys(m)` returns a `Vector<K>` in insertion order and there is
  deliberately no `values`. It answers `remove` / `contains` with the sets,
  never `erase` / `find`. Rationale in `memory/2026-09-14.md`.

## Resolved, previously flagged

- The **`print` vs `len` shadowing inconsistency** (flagged 2026-09-11) is
  gone: module members are a separate namespace, so `container.add` never
  takes the word `add` from the program and nothing resolves "last".

## Open, not yet decided by him

- **No `null`** — my call, not his. Un-C. He may want it back.
- **`[x; N]` next to `[a, b, c]`** — two array literal forms that overlap for
  `[0; 3]`. A real bend of the one-way rule; I took it because enumerating 64
  zeros is untenable. Flagged to him.
- **`HashMap` next to `Set`** — `Set` is a hash set spelled bare, so the
  naming is asymmetric. Either the map becomes `Map` or the set becomes
  `HashSet`; one line in the lexer. I used his word. Flagged 2026-09-14.
- **Contextual int literals** vs suffixes (`10i64`) — my call.
- Whether the `fn` / `->` reserved-token diagnostics stay forever.
- **`binz/` buys nothing yet** — every path is `binz/<one segment>`. It is his
  word, and it only pays off once user modules exist.
- `.binzc` artifact extension was my extrapolation from his `.binz` request.

## Not in v0.1

Slices, closures, user-written generics, **user modules** (`import` reaches
`binz/*` only), methods, enums, an ordered map, bitwise ops, unsigned ints.
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
