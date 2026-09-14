//! End-to-end tests: each case is compiled and executed by the real binary.

use std::io::Write;
use std::process::Command;

/// Each test owns one file in the temp directory, so the name has to be
/// unique -- tests run in parallel and would otherwise race on it.
fn write_temp(name: &str, src: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("binz_test_{}.binz", name));
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(src.as_bytes()).unwrap();
    path
}

/// Every test program is compiled with the whole standard library in scope,
/// so a test only has to say what it is about. The import rules themselves
/// are exercised by `run_raw_err`, which writes its own file head.
const PRELUDE: &str = "import binz/io;\nimport binz/string;\nimport binz/int;\n\
                       import binz/float;\nimport binz/container;\n";

/// Runs a program, asserting it exits 0, and returns its stdout.
fn run_ok(name: &str, src: &str) -> String {
    let path = write_temp(name, &format!("{}{}", PRELUDE, src));
    let out = Command::new(env!("CARGO_BIN_EXE_binz")).arg("run").arg(&path).output().unwrap();
    assert!(
        out.status.success(),
        "program failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// Like `run_ok`, but the source is taken exactly as written -- no imports.
fn run_raw_ok(name: &str, src: &str) -> String {
    let path = write_temp(name, src);
    let out = Command::new(env!("CARGO_BIN_EXE_binz")).arg("run").arg(&path).output().unwrap();
    assert!(out.status.success(), "program failed:\n{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

/// Runs a program expected to be rejected, and returns the diagnostic.
fn run_err(name: &str, src: &str) -> String {
    run_raw_err(name, &format!("{}{}", PRELUDE, src))
}

/// Like `run_err`, but the source is taken exactly as written -- no imports.
fn run_raw_err(name: &str, src: &str) -> String {
    let path = write_temp(name, src);
    let out = Command::new(env!("CARGO_BIN_EXE_binz")).arg("run").arg(&path).output().unwrap();
    assert!(!out.status.success(), "expected failure, program succeeded");
    String::from_utf8(out.stderr).unwrap()
}

fn in_main(body: &str) -> String {
    format!("function main(): i32 {{\n{}\nreturn 0;\n}}", body)
}

#[test]
fn primitives_and_casts() {
    let out = run_ok(
        "prims",
        &in_main(
            r#"
            const a: i32 = 7;
            const b: i64 = 7;
            const c: f64 = 0.5;
            io.print(cast<str>(a) + " " + cast<str>(b) + " " + cast<str>(c));
            io.print(cast<str>(cast<f64>(a) + c));
            io.print(cast<str>(cast<i32>(2.9)));
            io.print(cast<str>(true) + " " + cast<str>(!true));
            io.print(cast<str>(7 % 3) + " " + cast<str>(-a));
        "#,
        ),
    );
    assert_eq!(out, "7 7 0.5\n7.5\n2\ntrue false\n1 -7\n");
}

#[test]
fn control_flow_and_short_circuit() {
    let out = run_ok(
        "flow",
        &in_main(
            r#"
            var i: i32 = 0;
            var s: str = "";
            while (i < 5) {
                if (i % 2 == 0) { s = s + cast<str>(i); } else { s = s + "-"; }
                i = i + 1;
            }
            io.print(s);
            const zero: i32 = 0;
            if (zero != 0 && 10 / zero > 1) { io.print("bad"); } else { io.print("short-circuited"); }
        "#,
        ),
    );
    assert_eq!(out, "0-2-4\nshort-circuited\n");
}

#[test]
fn structs_are_value_types() {
    let out = run_ok(
        "structs",
        "struct P { x: i64, y: i64 }
         function main(): i32 {
             var a: P = P { x: 1, y: 2 };
             var b: P = a;
             b.x = 99;
             io.print(cast<str>(a.x) + \",\" + cast<str>(b.x));
             return 0;
         }",
    );
    assert_eq!(out, "1,99\n");
}

#[test]
fn pointers_alias_the_original() {
    let out = run_ok(
        "ptr",
        "struct P { x: i64, y: i64 }
         function shift(p: *P, d: i64): void { (*p).x = (*p).x + d; return; }
         function main(): i32 {
             var a: P = P { x: 1, y: 2 };
             shift(&a, 10);
             var n: i64 = 5;
             var pn: *i64 = &n;
             *pn = *pn * 2;
             io.print(cast<str>(a.x) + \",\" + cast<str>(n));
             return 0;
         }",
    );
    assert_eq!(out, "11,10\n");
}

#[test]
fn structs_pass_and_return_by_value() {
    let out = run_ok(
        "sret",
        "struct P { x: i64, y: i64 }
         function swap(p: P): P { return P { x: p.y, y: p.x }; }
         function main(): i32 {
             var a: P = P { x: 1, y: 2 };
             const b: P = swap(a);
             io.print(cast<str>(b.x) + \",\" + cast<str>(b.y) + \" orig \" + cast<str>(a.x));
             return 0;
         }",
    );
    assert_eq!(out, "2,1 orig 1\n");
}

#[test]
fn functions_are_first_class() {
    let out = run_ok(
        "firstclass",
        "function add(a: i64, b: i64): i64 { return a + b; }
         function apply(f: function(i64, i64): i64, a: i64, b: i64): i64 { return f(a, b); }
         function main(): i32 {
             const f: function(i64, i64): i64 = add;
             const say: function(str): void = io.print;
             say(cast<str>(apply(f, 2, 3)));
             return 0;
         }",
    );
    assert_eq!(out, "5\n");
}

#[test]
fn recursion() {
    let out = run_ok(
        "rec",
        "function fib(n: i64): i64 { if (n < 2) { return n; } return fib(n - 1) + fib(n - 2); }
         function main(): i32 { io.print(cast<str>(fib(20))); return 0; }",
    );
    assert_eq!(out, "6765\n");
}

#[test]
fn main_return_is_the_exit_code() {
    let path = write_temp("exitcode", "function main(): i32 { return 3; }");
    let st = Command::new(env!("CARGO_BIN_EXE_binz")).arg("run").arg(&path).status().unwrap();
    assert_eq!(st.code(), Some(3));
}

#[test]
fn artifact_round_trips() {
    let path = write_temp(
        "artifact",
        &format!("{}function main(): i32 {{ io.print(\"from artifact\"); return 0; }}", PRELUDE),
    );
    let bzc = path.with_extension("binzc");
    let st = Command::new(env!("CARGO_BIN_EXE_binz"))
        .arg("build")
        .arg(&path)
        .arg("-o")
        .arg(&bzc)
        .status()
        .unwrap();
    assert!(st.success());
    let out = Command::new(env!("CARGO_BIN_EXE_binz")).arg("exec").arg(&bzc).output().unwrap();
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "from artifact\n");

    let dump = Command::new(env!("CARGO_BIN_EXE_binz")).arg("dump").arg(&bzc).output().unwrap();
    let text = String::from_utf8(dump.stdout).unwrap();
    assert!(text.contains("function #0 main"), "{}", text);
    assert!(text.contains("push.native"), "{}", text);
}

// ------------------------------------------------------------ rejections

#[test]
fn rejects_implicit_conversion() {
    let e = run_err(
        "noimplicit",
        &in_main("const a: i32 = 1; const b: i64 = 2; io.print(cast<str>(a + b));"),
    );
    assert!(e.contains("same type"), "{}", e);
}

#[test]
fn rejects_const_assignment() {
    let e = run_err("noconstassign", &in_main("const a: i32 = 1; a = 2;"));
    assert!(e.contains("cannot be reassigned"), "{}", e);
}

#[test]
fn rejects_field_access_through_pointer() {
    let e = run_err(
        "noarrow",
        "struct P { x: i32 }
         function main(): i32 { var p: P = P { x: 1 }; const q: *P = &p; io.print(cast<str>(q.x)); return 0; }",
    );
    assert!(e.contains("dereference it first"), "{}", e);
}

#[test]
fn rejects_out_of_order_struct_fields() {
    let e = run_err(
        "order",
        "struct P { x: i32, y: i32 }
         function main(): i32 { var p: P = P { y: 1, x: 2 }; return 0; }",
    );
    assert!(e.contains("declaration order"), "{}", e);
}

#[test]
fn rejects_address_of_const() {
    let e = run_err("addrconst", &in_main("const a: i32 = 1; const p: *i32 = &a;"));
    assert!(e.contains("`var` place"), "{}", e);
}

#[test]
fn rejects_non_bool_condition() {
    let e = run_err("cond", &in_main("var a: i32 = 1; if (a) { }"));
    assert!(e.contains("expected `bool`"), "{}", e);
}

#[test]
fn rejects_missing_return() {
    let e = run_err("noret", "function f(): i32 { io.print(\"x\"); }\nfunction main(): i32 { return 0; }");
    assert!(e.contains("on every path"), "{}", e);
}

#[test]
fn rejects_value_recursive_struct() {
    let e = run_err("cyclic", "struct N { next: N }\nfunction main(): i32 { return 0; }");
    assert!(e.contains("contains itself"), "{}", e);
}

#[test]
fn rejects_int_literal_for_float() {
    let e = run_err("intfloat", &in_main("const x: f64 = 3;"));
    assert!(e.contains("decimal point"), "{}", e);
}

#[test]
fn traps_on_division_by_zero() {
    let e = run_err("divzero", &in_main("var z: i32 = 0; io.print(cast<str>(10 / z));"));
    assert!(e.contains("division by zero"), "{}", e);
}

#[test]
fn traps_on_overflow() {
    let e = run_err("overflow", &in_main("var a: i32 = 2147483647; io.print(cast<str>(a + a));"));
    assert!(e.contains("overflow"), "{}", e);
}

// ------------------------------------------------------------- containers

#[test]
fn fixed_arrays_are_values() {
    let out = run_ok(
        "arrays",
        &in_main(
            r#"
            var a: [i32; 4] = [10, 20, 30, 40];
            a[2] = 99;
            var b: [i32; 4] = a;
            b[0] = 1;
            io.print(cast<str>(a[0]) + " " + cast<str>(a[2]) + " " + cast<str>(container.size(a)));
            io.print(cast<str>(container.find(a, 40)) + " " + cast<str>(container.find(a, 7)));
            var zeros: [i64; 5] = [0; 5];
            zeros[4] = 7;
            io.print(cast<str>(zeros[0]) + " " + cast<str>(zeros[4]));
        "#,
        ),
    );
    assert_eq!(out, "10 99 4\n3 -1\n0 7\n");
}

#[test]
fn arrays_nest_in_structs_and_functions() {
    let out = run_ok(
        "arraynest",
        r#"
        struct P { x: i64, y: i64 }
        struct Grid { cells: [i32; 3], corners: [P; 2] }

        function total(a: [i32; 3]): i32 {
            var i: i32 = 0;
            var sum: i32 = 0;
            while (i < container.size(a)) { sum = sum + a[i]; i = i + 1; }
            return sum;
        }

        function ramp(): [i32; 3] {
            var out: [i32; 3] = [0; 3];
            out[1] = 2;
            out[2] = 4;
            return out;
        }

        function main(): i32 {
            var g: Grid = Grid { cells: [1, 2, 3], corners: [P { x: 0, y: 0 }, P { x: 9, y: 9 }] };
            g.cells[1] = 20;
            g.corners[1].x = 77;
            io.print(cast<str>(g.cells[1]) + " " + cast<str>(g.corners[1].x) + " " + cast<str>(g.corners[0].x));
            io.print(cast<str>(total([2, 4, 6])));
            const r: [i32; 3] = ramp();
            io.print(cast<str>(r[0]) + cast<str>(r[1]) + cast<str>(r[2]));
            var m: [[i32; 2]; 3] = [[0; 2]; 3];
            m[2][1] = 5;
            io.print(cast<str>(m[2][1]) + cast<str>(m[0][1]));
            var nums: [i32; 3] = [1, 2, 3];
            const p: *i32 = &nums[1];
            *p = 100;
            io.print(cast<str>(nums[1]));
            return 0;
        }
        "#,
    );
    assert_eq!(out, "20 77 0\n12\n024\n50\n100\n");
}

#[test]
fn vector_grows_and_shrinks() {
    let out = run_ok(
        "vector",
        &in_main(
            r#"
            var v: Vector<str> = Vector<str>{};
            container.push(v, "one");
            container.push(v, "two");
            container.push(v, "three");
            v[1] = "TWO";
            container.insert(v, 0, "zero");
            io.print(v[0] + " " + v[1] + " " + v[2] + " " + v[3]);
            io.print(container.erase(v, 0) + " " + container.pop(v) + " " + cast<str>(container.size(v)));
            io.print(cast<str>(container.find(v, "TWO")) + " " + cast<str>(container.find(v, "nope")));
            container.clear(v);
            io.print(cast<str>(container.size(v)));
        "#,
        ),
    );
    assert_eq!(out, "zero one TWO three\nzero three 2\n1 -1\n0\n");
}

#[test]
fn linked_list_walks_from_both_ends() {
    let out = run_ok(
        "list",
        &in_main(
            r#"
            var l: LinkedList<i32> = LinkedList<i32>{1, 2, 3};
            container.insert(l, 0, 0);
            container.push(l, 4);
            var i: i32 = 0;
            var line: str = "";
            while (i < container.size(l)) { line = line + cast<str>(l[i]); i = i + 1; }
            io.print(line);
            io.print(cast<str>(container.erase(l, 2)) + " " + cast<str>(container.pop(l)) + " " + cast<str>(container.size(l)));
            io.print(cast<str>(container.find(l, 4)) + " " + cast<str>(l[container.size(l) - 1]));
        "#,
        ),
    );
    assert_eq!(out, "01234\n2 4 3\n-1 3\n");
}

#[test]
fn sets_hold_each_key_once() {
    let out = run_ok(
        "sets",
        &in_main(
            r#"
            var s: Set<str> = Set<str>{"b", "a", "b"};
            io.print(cast<str>(container.add(s, "c")) + " " + cast<str>(container.add(s, "a")) + " " + cast<str>(container.size(s)));
            io.print(cast<str>(container.contains(s, "a")) + " " + cast<str>(container.remove(s, "b")) + " " + cast<str>(container.contains(s, "b")));
            io.print(s[0] + s[1]);
            var t: SortedSet<i32> = SortedSet<i32>{5, 1, 4, 1};
            container.add(t, 3);
            var i: i32 = 0;
            var line: str = "";
            while (i < container.size(t)) { line = line + cast<str>(t[i]); i = i + 1; }
            io.print(line);
        "#,
        ),
    );
    assert_eq!(out, "true false 3\ntrue true false\nac\n1345\n");
}

#[test]
fn containers_are_handles_and_copy_is_explicit() {
    let out = run_ok(
        "handles",
        r#"
        function fill(v: Vector<i32>): void {
            container.push(v, 3);
            return;
        }
        function main(): i32 {
            var u: Vector<i32> = Vector<i32>{1, 2};
            var alias: Vector<i32> = u;
            var snapshot: Vector<i32> = container.copy(u);
            fill(u);
            io.print(cast<str>(container.size(u)) + " " + cast<str>(container.size(alias)) + " " + cast<str>(container.size(snapshot)));
            var rows: Vector<Vector<i32>> = Vector<Vector<i32>>{};
            container.push(rows, Vector<i32>{1, 2});
            container.push(rows[0], 9);
            io.print(cast<str>(container.size(rows[0])) + " " + cast<str>(rows[0][2]));
            return 0;
        }
        "#,
    );
    assert_eq!(out, "3 3 2\n3 9\n");
}

#[test]
fn a_str_is_sized_by_the_string_module() {
    let out = run_ok("strlen", &in_main("io.print(cast<str>(string.size(\"hello\")));"));
    assert_eq!(out, "5\n");
}

#[test]
fn a_str_is_not_a_container() {
    let e = run_err("strcontainer", &in_main("io.print(cast<str>(container.size(\"hello\")));"));
    assert!(e.contains("a `str` answers `string.size`"), "{}", e);
}

#[test]
fn a_module_member_never_collides_with_a_user_name() {
    // `add` is a `binz/container` verb, but it is only ever reached as
    // `container.add`, so the bare name stays the program's to use.
    let out = run_ok(
        "shadow",
        "function add(a: i32, b: i32): i32 { return a + b; }
         function main(): i32 {
             var s: Set<i32> = Set<i32>{};
             container.add(s, 9);
             io.print(cast<str>(add(2, 3)) + \" \" + cast<str>(container.size(s)));
             return 0;
         }",
    );
    assert_eq!(out, "5 1\n");
}

#[test]
fn rejects_wrong_array_literal_length() {
    let e = run_err("arrlen", &in_main("const a: [i32; 3] = [1, 2];"));
    assert!(e.contains("needs 3 element(s)"), "{}", e);
}

#[test]
fn rejects_array_literal_without_a_type() {
    let e = run_err("arrhint", &in_main("io.print(cast<str>([1, 2]));"));
    assert!(e.contains("needs a declared type"), "{}", e);
}

#[test]
fn rejects_the_wrong_builtin_for_the_container() {
    let e = run_err("wrongop", &in_main("var s: Set<i32> = Set<i32>{}; container.push(s, 1);"));
    assert!(e.contains("use `container.add`"), "{}", e);
    let e = run_err("wrongop2", &in_main("var v: Vector<i32> = Vector<i32>{}; container.add(v, 1);"));
    assert!(e.contains("use `container.push`"), "{}", e);
    let e = run_err("wrongop3", &in_main("var a: [i32; 1] = [1]; container.push(a, 2);"));
    assert!(e.contains("never changes size"), "{}", e);
}

#[test]
fn rejects_assigning_into_a_set() {
    let e = run_err("setassign", &in_main("var s: Set<i32> = Set<i32>{1}; s[0] = 2;"));
    assert!(e.contains("are its keys"), "{}", e);
}

#[test]
fn rejects_a_struct_inside_a_heap_container() {
    let e = run_err(
        "structelem",
        "struct P { x: i32 }
         function main(): i32 { var v: Vector<P> = Vector<P>{}; return 0; }",
    );
    assert!(e.contains("one-slot values"), "{}", e);
    let e = run_err("setelem", &in_main("var s: Set<Vector<i32>> = Set<Vector<i32>>{};"));
    assert!(e.contains("holds keys"), "{}", e);
}

#[test]
fn rejects_taking_the_address_of_a_heap_element() {
    let e = run_err(
        "heapaddr",
        &in_main("var v: Vector<i32> = Vector<i32>{1}; const p: *i32 = &v[0];"),
    );
    assert!(e.contains("has no address"), "{}", e);
}

#[test]
fn traps_on_out_of_range_index() {
    let e = run_err("arroob", &in_main("var a: [i32; 2] = [1, 2]; io.print(cast<str>(a[5]));"));
    assert!(e.contains("out of range for an array of length 2"), "{}", e);
    let e = run_err("vecoob", &in_main("var v: Vector<i32> = Vector<i32>{1}; io.print(cast<str>(v[3]));"));
    assert!(e.contains("out of range for a Vector of length 1"), "{}", e);
    let e = run_err("emptypop", &in_main("var v: Vector<i32> = Vector<i32>{}; io.print(cast<str>(container.pop(v)));"));
    assert!(e.contains("pop on an empty Vector"), "{}", e);
}

#[test]
fn traps_on_nan_as_a_set_key() {
    let e = run_err(
        "nankey",
        &in_main("var s: SortedSet<f64> = SortedSet<f64>{}; var z: f64 = 0.0; container.add(s, z / z);"),
    );
    assert!(e.contains("NaN"), "{}", e);
}

#[test]
fn hashmap_reads_writes_and_iterates() {
    let out = run_ok(
        "map",
        &in_main(
            r#"
            var ages: HashMap<str, i32> = HashMap<str, i32>{"ana": 31, "bruno": 27};
            ages["carla"] = 45;
            ages["ana"] = 32;
            io.print(cast<str>(container.size(ages)) + " " + cast<str>(ages["ana"]));
            io.print(cast<str>(container.contains(ages, "bruno")) + " " + cast<str>(container.contains(ages, "zed")));
            io.print(cast<str>(container.remove(ages, "bruno")) + " " + cast<str>(container.remove(ages, "bruno")));
            const ks: Vector<str> = container.keys(ages);
            var i: i32 = 0;
            var line: str = "";
            while (i < container.size(ks)) {
                line = line + ks[i] + "=" + cast<str>(ages[ks[i]]) + " ";
                i = i + 1;
            }
            io.print(line);
            container.clear(ages);
            io.print(cast<str>(container.size(ages)));
        "#,
        ),
    );
    assert_eq!(out, "3 32\ntrue false\ntrue false\nana=32 carla=45 \n0\n");
}

#[test]
fn hashmap_aliases_and_copies_deeply() {
    let out = run_ok(
        "mapalias",
        &in_main(
            r#"
            var a: HashMap<str, Vector<i32>> = HashMap<str, Vector<i32>>{};
            a["xs"] = Vector<i32>{1, 2};
            const shared: HashMap<str, Vector<i32>> = a;
            container.push(shared["xs"], 3);
            const mine: HashMap<str, Vector<i32>> = container.copy(a);
            container.push(mine["xs"], 4);
            io.print(cast<str>(container.size(a["xs"])) + " " + cast<str>(container.size(mine["xs"])));
        "#,
        ),
    );
    assert_eq!(out, "3 4\n");
}

#[test]
fn hashmap_keeps_each_key_once() {
    let out = run_ok(
        "mapdup",
        &in_main(
            r#"
            const m: HashMap<i32, str> = HashMap<i32, str>{1: "one", 1: "uno"};
            io.print(cast<str>(container.size(m)) + " " + m[1]);
        "#,
        ),
    );
    assert_eq!(out, "1 uno\n");
}

#[test]
fn rejects_the_wrong_builtin_for_a_hashmap() {
    let e = run_err("mapop", &in_main("var m: HashMap<str, i32> = HashMap<str, i32>{}; container.push(m, 1);"));
    assert!(e.contains("write `m[key] = value`"), "{}", e);
    let e = run_err("mapop2", &in_main("var m: HashMap<str, i32> = HashMap<str, i32>{}; container.erase(m, 0);"));
    assert!(e.contains("use `container.remove`"), "{}", e);
    let e = run_err("mapop3", &in_main("var v: Vector<i32> = Vector<i32>{}; const k: Vector<i32> = container.keys(v);"));
    assert!(e.contains("only defined for a `HashMap<K, V>`"), "{}", e);
}

#[test]
fn rejects_a_bad_hashmap_key_or_value() {
    let e = run_err(
        "mapkey",
        "struct P { x: i32 }
         function main(): i32 { var m: HashMap<P, i32> = HashMap<P, i32>{}; return 0; }",
    );
    assert!(e.contains("keyed by value"), "{}", e);
    let e = run_err(
        "mapval",
        "struct P { x: i32 }
         function main(): i32 { var m: HashMap<i32, P> = HashMap<i32, P>{}; return 0; }",
    );
    assert!(e.contains("one-slot values"), "{}", e);
    let e = run_err("mapidx", &in_main("var m: HashMap<str, i32> = HashMap<str, i32>{}; m[1] = 2;"));
    assert!(e.contains("in a key: expected `str`"), "{}", e);
}

#[test]
fn traps_on_a_missing_hashmap_key() {
    let e = run_err(
        "mapmiss",
        &in_main("const m: HashMap<str, i32> = HashMap<str, i32>{\"a\": 1}; io.print(cast<str>(m[\"b\"]));"),
    );
    assert!(e.contains("is not in the HashMap"), "{}", e);
}

// ------------------------------------------------------ the standard library

#[test]
fn imports_bind_the_last_path_segment() {
    let out = run_raw_ok(
        "import",
        "import binz/io;\n\
         import binz/string;\n\
         function main(): i32 { io.print(string.upper(\"binz\")); return 0; }",
    );
    assert_eq!(out, "BINZ\n");
}

#[test]
fn rejects_a_module_used_without_its_import() {
    let e = run_raw_err(
        "noimport",
        "function main(): i32 { io.print(\"x\"); return 0; }",
    );
    assert!(e.contains("add `import binz/io;`"), "{}", e);
}

#[test]
fn rejects_an_unqualified_stdlib_name() {
    let e = run_raw_err("bareprint", "function main(): i32 { print(\"x\"); return 0; }");
    assert!(e.contains("`print` is in `binz/io`"), "{}", e);
}

/// `len` was the spelling before the stdlib had modules; the diagnostic has
/// to carry a reader all the way to the line that replaces it.
#[test]
fn points_a_bare_len_at_size() {
    let e = run_err("barelen", &in_main("var v: Vector<i32> = Vector<i32>{}; io.print(cast<str>(len(v)));"));
    assert!(e.contains("`len` is now `size`"), "{}", e);
    assert!(e.contains("`container.size`"), "{}", e);
}

#[test]
fn rejects_an_unknown_module() {
    let e = run_raw_err("nomodule", "import binz/json;\nfunction main(): i32 { return 0; }");
    assert!(e.contains("there is no module `binz/json`"), "{}", e);
}

#[test]
fn rejects_a_path_outside_binz() {
    let e = run_raw_err("badpath", "import std/io;\nfunction main(): i32 { return 0; }");
    assert!(e.contains("spelled `binz/<name>`"), "{}", e);
}

#[test]
fn rejects_a_repeated_import() {
    let e = run_raw_err(
        "dupimport",
        "import binz/io;\nimport binz/io;\nfunction main(): i32 { return 0; }",
    );
    assert!(e.contains("already imported"), "{}", e);
}

#[test]
fn rejects_an_import_below_the_first_item() {
    let e = run_raw_err(
        "lateimport",
        "function main(): i32 { return 0; }\nimport binz/io;",
    );
    assert!(e.contains("at the top of the file"), "{}", e);
}

/// An import owns its binding, so `io` can never be two things at once.
#[test]
fn rejects_a_name_that_shadows_an_imported_module() {
    let e = run_raw_err(
        "shadowmod",
        "import binz/io;\nfunction io(): i32 { return 0; }\nfunction main(): i32 { return 0; }",
    );
    assert!(e.contains("is the imported module `binz/io`"), "{}", e);
}

#[test]
fn rejects_a_member_a_module_does_not_have() {
    let e = run_err("nomember", &in_main("io.print(cast<str>(string.push(\"a\", \"b\")));"));
    assert!(e.contains("`binz/string` has no `push`"), "{}", e);
    assert!(e.contains("it is in `binz/container`"), "{}", e);
}

/// The rule: a stdlib function is a value exactly when its type is writable.
#[test]
fn a_monomorphic_member_is_a_value_and_a_generic_one_is_not() {
    let out = run_ok(
        "membervalue",
        "function main(): i32 {
             const shout: function(str): str = string.upper;
             io.print(shout(\"ok\"));
             return 0;
         }",
    );
    assert_eq!(out, "OK\n");

    let e = run_err("genericvalue", &in_main("const f: function(str): i32 = container.size;"));
    assert!(e.contains("is not a value"), "{}", e);
}

#[test]
fn string_positions_are_character_indices() {
    let out = run_ok(
        "strchars",
        &in_main(
            "const s: str = \"maçã\";
             io.print(cast<str>(string.size(s)) + \" \" + string.at(s, 2) + \" \"
                 + string.slice(s, 0, 2) + \" \" + cast<str>(string.find(s, \"çã\")));",
        ),
    );
    assert_eq!(out, "4 ç ma 2\n");
}

#[test]
fn string_splits_and_joins_through_a_vector() {
    let out = run_ok(
        "strsplit",
        &in_main(
            "const parts: Vector<str> = string.split(\"a,b,c\", \",\");
             io.print(cast<str>(container.size(parts)) + \" \" + string.join(parts, \"-\"));",
        ),
    );
    assert_eq!(out, "3 a-b-c\n");
}

/// No `null`, so `parse` traps and `canParse` is the guard -- the same
/// shape as `container.contains` in front of a `HashMap` read.
#[test]
fn parsing_is_guarded_rather_than_optional() {
    let out = run_ok(
        "parse",
        &in_main(
            "io.print(cast<str>(int.canParse(\"12\")) + \" \" + cast<str>(int.canParse(\"x\"))
                 + \" \" + cast<str>(int.parse(\"12\")) + \" \" + cast<str>(float.parse(\"1.5\")));",
        ),
    );
    assert_eq!(out, "true false 12 1.5\n");

    let e = run_err("parsetrap", &in_main("io.print(cast<str>(int.parse(\"x\")));"));
    assert!(e.contains("guard it with `int.canParse`"), "{}", e);
}

#[test]
fn int_forms_answer_in_the_width_they_were_given() {
    let out = run_ok(
        "intwidth",
        &in_main(
            "const small: i32 = -7;
             const wide: i64 = -8000000000;
             const a: i32 = int.max(int.abs(small), 3);
             const b: i64 = int.min(int.abs(wide), 5);
             io.print(cast<str>(a) + \" \" + cast<str>(b));",
        ),
    );
    assert_eq!(out, "7 5\n");

    let e = run_err("intabsfloat", &in_main("const x: f64 = 1.0; io.print(cast<str>(int.abs(x)));"));
    assert!(e.contains("an `f64` answers `float.abs`"), "{}", e);
}

#[test]
fn float_module_covers_the_usual_arithmetic() {
    let out = run_ok(
        "floatmod",
        &in_main(
            "io.print(cast<str>(float.sqrt(9.0)) + \" \" + cast<str>(float.pow(2.0, 8.0))
                 + \" \" + cast<str>(float.floor(2.7)) + \" \" + cast<str>(float.round(2.5))
                 + \" \" + cast<str>(float.isNan(0.0)));",
        ),
    );
    assert_eq!(out, "3.0 256.0 2.0 3.0 false\n");
}
