# MEMORY.md

Curated long-term memory. Raw per-day logs live in `memory/YYYY-MM-DD.md`.

## What binZ is

A strongly typed, compiled language with C/C++/JS-shaped syntax. Rust frontend
and backend, emitting a `.binzc` bytecode artifact executed by a stack VM in
`src/vm.rs`. Source files are `.binz`. Started 2026-09-10 from an empty
directory; v0.1 works end to end, containers landed 2026-09-11, and `HashMap`,
the standard library, `binz/map` and `SortedMap` all landed 2026-09-14, and
local modules (`import @root/...`) on 2026-09-15, and the test library
(`@test`, `binz test`, `stub`) on 2026-09-17.

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
- **Local modules, 2026-09-15**: `import @root/utils/math.binz;` binds
  `math`. `@root` is the directory of the file handed to `binz`, so a path
  means the same thing wherever it is written — **no `../`, no path relative
  to the importing file, no reaching outside the root**. Everything else is
  the stdlib rule already in force: the binding is the last segment, always,
  **so a module file is named with one lowercase word** (the name is the
  identifier the importer types) and `.binz` is required. A module is an
  ordinary file: it **exports every function it defines and nothing else**
  (no `export`, no `pub`), its **structs stay inside it** (struct ids are per
  file, so the importer cannot write the type), it **imports what it uses**
  (the importing file is not a scope), and it **does not define `main`**.
  **Names are per file** — two files may both define `add`. A module function
  is a value, by the 2026-09-14 rule. No bytecode change: the VM never learns
  that modules exist. Rationale in `memory/2026-09-15.md`.
- **`as`, 2026-09-15 — his rule, and a sharp one.** A plain alias is what the
  one-way rule forbids: it gives a module two spellings and you pick. His
  version removes the choice. **`as` is legal only where the default binding
  is unavailable** — two or more imports in one file whose files are named
  the same — **and then it is required on every one of them**, so a name is
  never the default for one import and a rename for another. His words: "this
  only is allowed when there are two modules with the same name, BOTH need to
  renamed", then "two or more, for sure". Corollaries, all forced rather than
  invented: `as` on a **stdlib import is always an error** (no two stdlib
  modules are named the same, so `io.print` reads identically everywhere);
  **a clash is per file**; two imports of the *same* file are a duplicate, not
  a clash. The whole rule is one four-way match on `(alias, clash)` in
  `check_rename`.
- **The rename itself is derived, not chosen — his call, same day.** I left it
  free-form and flagged that as the weak half of the rule; he closed it with
  `as textFormat` / `as numberFormat` and "I want it strict, in this way".
  **A rename is the module's directory and its own name joined in camelCase**:
  `text/format.binz` is `textFormat` and nothing else, every other spelling is
  refused naming the right one, and a file directly under the anchor uses
  `root` (`@root/format.binz` -> `rootFormat`). So there is **no free choice
  anywhere in an import**. It stays local — two contested imports always
  differ in the directory, since two files of the same name in one directory
  *are* one file — so adding an import never changes another's rename.
  **This is the first camelCase name that is not a member**: modules, files
  and directories are one lowercase word, but a rename is two module names
  *joined*, and camelCase is how binZ joins words.
- **Tests, 2026-09-17**: a test lives in the file it tests, tagged
  `@test function sumsPositives(): void`, takes nothing and answers `void`.
  **A test name may not start with `test`** — his rule, the tag already says
  it. `binz test <file>` compiles that file *and everything it imports* and
  runs every test in the graph; **`main` is neither required nor run**, so a
  module is testable on its own. `binz build`/`binz run` **do not compile a
  test at all**: it is not in the artifact and nothing can call one.
  Asserting is `test.equal(actual, expected)` — the only assertion, since
  `notEqual`/`assert`/`check` all overlap it — plus `test.fail(msg)` for what
  equality cannot state, and `test.calls(math.add)` for how many times a
  function ran. `binz/test` is reachable only from a test or stub body.
- **`stub`, 2026-09-17 — mocking with no interfaces and no DI.** His ask;
  the answer was already in the VM, because every call is `OP_PUSH_FN <id>` +
  `OP_CALL` and so dispatches by function id. `stub rates.lookup(country:
  str): f64 { ... }` at the **top of a test** replaces the *function*, not
  the call: every path that reaches it lands in the stub, however deep in the
  import graph. One `OP_STUB` and a redirect table allocated only when used.
  Rules, each earning its keep: stubs come first in the test body (no
  temporal rule); the **signature is written out and must match** (a drifted
  stub is the one bug a test lib must not hide); one function one stub;
  **a module of this project only** — never `binz/io`; **a stub cannot call
  what it replaces** (it would land back in the stub). Counting exists
  because binZ has no globals and no closures, so a stub body **cannot
  record anything**. Each test runs in a fresh VM, so a stub dies with it.
  Artifact `VERSION` 3 -> 4: `Module.entry` is now optional and the module
  carries its test table. Rationale in `memory/2026-09-17.md`.
- **Standard library, 2026-09-14**: `import binz/io;` and every member is
  reached as `io.print`. The binding is the last path segment, **always** —
  no alias, no wildcard, no bare import — so two modules may both define
  `find` and there is no resolution rule to learn. An import owns its binding for the
  whole *file* (nothing else in it may be named `io`); imports come first in
  a file.
  Modules: `io` `string` `int` `float` `container` `map` `test`. **Naming, his call
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

- **A module could not be run on its own to test it** (flagged 2026-09-15) —
  gone 2026-09-17: `binz test` needs no `main`, so `binz test math.binz`
  works.

- **No user modules** (flagged 2026-09-14) — done 2026-09-15, see above. That
  is also when **`binz/` started paying for itself**: `binz/...` and
  `@root/...` are now two namespaces that have to be told apart.

- The **`print` vs `len` shadowing inconsistency** (flagged 2026-09-11) is
  gone: module members are a separate namespace, so `container.add` never
  takes the word `add` from the program and nothing resolves "last".

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
- **`some-module.binz` is an error**, one lowercase word only, because the
  file name is the binding. That is the name *he typed* in the request.
  Flagged 2026-09-15.
- **`@root` is the entry file's directory**, so moving the entry file moves
  the root. A manifest would pin it; there isn't one and I did not invent one.
- **No exported types** — a struct cannot cross a module boundary, so two
  files cannot share a data type. The next thing to decide about modules.
- **A name clash is per file**, so one module can be `math` in one file and
  `geometryMath` in another. Per-file is the only rule that does not make adding an
  import to one file break another, but a reader moving between files sees two
  names for one module. Flagged 2026-09-15.
- **`as` cannot resolve a clash with the standard library**: a file named
  `io.binz` is refused outright rather than being renameable, since his rule
  would otherwise demand renaming `binz/io` too. The one place the rule is not
  mechanical. Flagged 2026-09-15.
- **Two contested files that share a parent directory name** (`a/x/format` and
  `b/x/format`) both derive `xFormat` and collide, reported as a plain
  "already imported". Rare; the fixes are a longer prefix (non-local) or a
  dedicated diagnostic. Flagged 2026-09-15.
- **A test is not type checked by `binz build`/`binz run`**, since neither
  compiles one. The price of a test weighing nothing in the artifact, but a
  stale test stays invisible until `binz test` runs. Flagged 2026-09-17.
- **The standard library cannot be stubbed**, so `io.print` output cannot be
  captured in a test. Natives are not bytecode functions and would need a
  second redirect table. The likeliest next ask. Flagged 2026-09-17.
- **A function of the file under test cannot be stubbed** — only a module's.
  Flagged 2026-09-17.
- **No setup, teardown or filter**: `binz test <file>` runs everything it can
  reach, and there is no way to run one test. Flagged 2026-09-17.
- **`import binz/test;` reserves `test` in the file**, the same bite as `map`.
- **`test.calls` counts every call to the function**, including the ones the
  module makes to itself. Flagged 2026-09-17.
- Whether the `fn` / `->` reserved-token diagnostics stay forever.
- `.binzc` artifact extension was my extrapolation from his `.binz` request.

## Not in v0.1

Slices, closures, user-written generics, **exported types** (a module exports
its functions only), methods, enums, bitwise ops, unsigned ints.
The stdlib is a scratch: `io` has only `print` — no input, no stderr, no
files, no time, no random. A test run has no setup, teardown or filter, and
cannot stub the standard library. `cast<str>` still formats
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
