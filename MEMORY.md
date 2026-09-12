# MEMORY.md

Curated long-term memory. Raw per-day logs live in `memory/YYYY-MM-DD.md`.

## What binZ is

A strongly typed, compiled language with C/C++/JS-shaped syntax. Rust frontend
and backend, emitting a `.binzc` bytecode artifact executed by a stack VM in
`src/vm.rs`. Source files are `.binz`. Started 2026-09-10 from an empty
directory; v0.1 works end to end, containers landed 2026-09-11.

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
  only way to format a value for `print`.
- Structs are C-like value types, flat layout, passed and returned by value
  (struct returns compile to a hidden destination pointer, so nothing dangles).
- Pointers: `&place` (only on `var` places) and `*p`. No `->`, write `(*p).x`.
- `while` is the only loop; no `+=`/`++`; conditions must be `bool`.
- Containers, 2026-09-11: `[T; N]` is a frame value and copies; `Vector<T>`,
  `LinkedList<T>`, `Set<T>`, `SortedSet<T>` are heap handles and alias.
  `c[i]` indexes all five, `len(c)` measures all five plus `str`, and the
  verbs are split so no container has two ways to do one thing — **`erase`
  takes a position, `remove` takes a key**; `find` for sequences, `contains`
  for sets. `copy(c)` is the one deep copy. Full rationale in
  `memory/2026-09-11.md`.

## Open, not yet decided by him

- **No `null`** — my call, not his. Un-C. He may want it back.
- **`[x; N]` next to `[a, b, c]`** — two array literal forms that overlap for
  `[0; 3]`. A real bend of the one-way rule; I took it because enumerating 64
  zeros is untenable. Flagged to him.
- **Container builtins resolve last**, so a user `function add(...)` shadows
  the set builtin. `print` still cannot be redefined — inconsistent, and he
  may want one rule for both.
- **Contextual int literals** vs suffixes (`10i64`) — my call.
- Whether the `fn` / `->` reserved-token diagnostics stay forever.
- `.binzc` artifact extension was my extrapolation from his `.binz` request.

## Not in v0.1

Slices, closures, user-written generics, modules, methods, enums, maps,
bitwise ops, unsigned ints, a standard library. `cast<str>` still formats
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
