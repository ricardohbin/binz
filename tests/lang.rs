//! End-to-end tests: each case is compiled and executed by the real binary.

use std::io::Write;
use std::process::Command;

fn write_temp(name: &str, src: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("binz_test_{}.binz", name));
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(src.as_bytes()).unwrap();
    path
}

/// Runs a program, asserting it exits 0, and returns its stdout.
fn run_ok(name: &str, src: &str) -> String {
    let path = write_temp(name, src);
    let out = Command::new(env!("CARGO_BIN_EXE_binz")).arg("run").arg(&path).output().unwrap();
    assert!(
        out.status.success(),
        "program failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// Runs a program expected to be rejected, and returns the diagnostic.
fn run_err(name: &str, src: &str) -> String {
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
            print(cast<str>(a) + " " + cast<str>(b) + " " + cast<str>(c));
            print(cast<str>(cast<f64>(a) + c));
            print(cast<str>(cast<i32>(2.9)));
            print(cast<str>(true) + " " + cast<str>(!true));
            print(cast<str>(7 % 3) + " " + cast<str>(-a));
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
            print(s);
            const zero: i32 = 0;
            if (zero != 0 && 10 / zero > 1) { print("bad"); } else { print("short-circuited"); }
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
             print(cast<str>(a.x) + \",\" + cast<str>(b.x));
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
             print(cast<str>(a.x) + \",\" + cast<str>(n));
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
             print(cast<str>(b.x) + \",\" + cast<str>(b.y) + \" orig \" + cast<str>(a.x));
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
             const say: function(str): void = print;
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
         function main(): i32 { print(cast<str>(fib(20))); return 0; }",
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
    let path = write_temp("artifact", "function main(): i32 { print(\"from artifact\"); return 0; }");
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
        &in_main("const a: i32 = 1; const b: i64 = 2; print(cast<str>(a + b));"),
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
         function main(): i32 { var p: P = P { x: 1 }; const q: *P = &p; print(cast<str>(q.x)); return 0; }",
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
    let e = run_err("noret", "function f(): i32 { print(\"x\"); }\nfunction main(): i32 { return 0; }");
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
    let e = run_err("divzero", &in_main("var z: i32 = 0; print(cast<str>(10 / z));"));
    assert!(e.contains("division by zero"), "{}", e);
}

#[test]
fn traps_on_overflow() {
    let e = run_err("overflow", &in_main("var a: i32 = 2147483647; print(cast<str>(a + a));"));
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
            print(cast<str>(a[0]) + " " + cast<str>(a[2]) + " " + cast<str>(len(a)));
            print(cast<str>(find(a, 40)) + " " + cast<str>(find(a, 7)));
            var zeros: [i64; 5] = [0; 5];
            zeros[4] = 7;
            print(cast<str>(zeros[0]) + " " + cast<str>(zeros[4]));
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
            while (i < len(a)) { sum = sum + a[i]; i = i + 1; }
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
            print(cast<str>(g.cells[1]) + " " + cast<str>(g.corners[1].x) + " " + cast<str>(g.corners[0].x));
            print(cast<str>(total([2, 4, 6])));
            const r: [i32; 3] = ramp();
            print(cast<str>(r[0]) + cast<str>(r[1]) + cast<str>(r[2]));
            var m: [[i32; 2]; 3] = [[0; 2]; 3];
            m[2][1] = 5;
            print(cast<str>(m[2][1]) + cast<str>(m[0][1]));
            var nums: [i32; 3] = [1, 2, 3];
            const p: *i32 = &nums[1];
            *p = 100;
            print(cast<str>(nums[1]));
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
            push(v, "one");
            push(v, "two");
            push(v, "three");
            v[1] = "TWO";
            insert(v, 0, "zero");
            print(v[0] + " " + v[1] + " " + v[2] + " " + v[3]);
            print(erase(v, 0) + " " + pop(v) + " " + cast<str>(len(v)));
            print(cast<str>(find(v, "TWO")) + " " + cast<str>(find(v, "nope")));
            clear(v);
            print(cast<str>(len(v)));
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
            insert(l, 0, 0);
            push(l, 4);
            var i: i32 = 0;
            var line: str = "";
            while (i < len(l)) { line = line + cast<str>(l[i]); i = i + 1; }
            print(line);
            print(cast<str>(erase(l, 2)) + " " + cast<str>(pop(l)) + " " + cast<str>(len(l)));
            print(cast<str>(find(l, 4)) + " " + cast<str>(l[len(l) - 1]));
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
            print(cast<str>(add(s, "c")) + " " + cast<str>(add(s, "a")) + " " + cast<str>(len(s)));
            print(cast<str>(contains(s, "a")) + " " + cast<str>(remove(s, "b")) + " " + cast<str>(contains(s, "b")));
            print(s[0] + s[1]);
            var t: SortedSet<i32> = SortedSet<i32>{5, 1, 4, 1};
            add(t, 3);
            var i: i32 = 0;
            var line: str = "";
            while (i < len(t)) { line = line + cast<str>(t[i]); i = i + 1; }
            print(line);
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
            push(v, 3);
            return;
        }
        function main(): i32 {
            var u: Vector<i32> = Vector<i32>{1, 2};
            var alias: Vector<i32> = u;
            var snapshot: Vector<i32> = copy(u);
            fill(u);
            print(cast<str>(len(u)) + " " + cast<str>(len(alias)) + " " + cast<str>(len(snapshot)));
            var rows: Vector<Vector<i32>> = Vector<Vector<i32>>{};
            push(rows, Vector<i32>{1, 2});
            push(rows[0], 9);
            print(cast<str>(len(rows[0])) + " " + cast<str>(rows[0][2]));
            return 0;
        }
        "#,
    );
    assert_eq!(out, "3 3 2\n3 9\n");
}

#[test]
fn len_also_answers_for_str() {
    let out = run_ok("strlen", &in_main("print(cast<str>(len(\"hello\")));"));
    assert_eq!(out, "5\n");
}

#[test]
fn user_names_shadow_container_builtins() {
    let out = run_ok(
        "shadow",
        "function add(a: i32, b: i32): i32 { return a + b; }
         function main(): i32 { print(cast<str>(add(2, 3))); return 0; }",
    );
    assert_eq!(out, "5\n");
}

#[test]
fn rejects_wrong_array_literal_length() {
    let e = run_err("arrlen", &in_main("const a: [i32; 3] = [1, 2];"));
    assert!(e.contains("needs 3 element(s)"), "{}", e);
}

#[test]
fn rejects_array_literal_without_a_type() {
    let e = run_err("arrhint", &in_main("print(cast<str>([1, 2]));"));
    assert!(e.contains("needs a declared type"), "{}", e);
}

#[test]
fn rejects_the_wrong_builtin_for_the_container() {
    let e = run_err("wrongop", &in_main("var s: Set<i32> = Set<i32>{}; push(s, 1);"));
    assert!(e.contains("use `add`"), "{}", e);
    let e = run_err("wrongop2", &in_main("var v: Vector<i32> = Vector<i32>{}; add(v, 1);"));
    assert!(e.contains("use `push`"), "{}", e);
    let e = run_err("wrongop3", &in_main("var a: [i32; 1] = [1]; push(a, 2);"));
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
    let e = run_err("arroob", &in_main("var a: [i32; 2] = [1, 2]; print(cast<str>(a[5]));"));
    assert!(e.contains("out of range for an array of length 2"), "{}", e);
    let e = run_err("vecoob", &in_main("var v: Vector<i32> = Vector<i32>{1}; print(cast<str>(v[3]));"));
    assert!(e.contains("out of range for a Vector of length 1"), "{}", e);
    let e = run_err("emptypop", &in_main("var v: Vector<i32> = Vector<i32>{}; print(cast<str>(pop(v)));"));
    assert!(e.contains("pop on an empty Vector"), "{}", e);
}

#[test]
fn traps_on_nan_as_a_set_key() {
    let e = run_err(
        "nankey",
        &in_main("var s: SortedSet<f64> = SortedSet<f64>{}; var z: f64 = 0.0; add(s, z / z);"),
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
            print(cast<str>(len(ages)) + " " + cast<str>(ages["ana"]));
            print(cast<str>(contains(ages, "bruno")) + " " + cast<str>(contains(ages, "zed")));
            print(cast<str>(remove(ages, "bruno")) + " " + cast<str>(remove(ages, "bruno")));
            const ks: Vector<str> = keys(ages);
            var i: i32 = 0;
            var line: str = "";
            while (i < len(ks)) {
                line = line + ks[i] + "=" + cast<str>(ages[ks[i]]) + " ";
                i = i + 1;
            }
            print(line);
            clear(ages);
            print(cast<str>(len(ages)));
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
            push(shared["xs"], 3);
            const mine: HashMap<str, Vector<i32>> = copy(a);
            push(mine["xs"], 4);
            print(cast<str>(len(a["xs"])) + " " + cast<str>(len(mine["xs"])));
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
            print(cast<str>(len(m)) + " " + m[1]);
        "#,
        ),
    );
    assert_eq!(out, "1 uno\n");
}

#[test]
fn rejects_the_wrong_builtin_for_a_hashmap() {
    let e = run_err("mapop", &in_main("var m: HashMap<str, i32> = HashMap<str, i32>{}; push(m, 1);"));
    assert!(e.contains("write `m[key] = value`"), "{}", e);
    let e = run_err("mapop2", &in_main("var m: HashMap<str, i32> = HashMap<str, i32>{}; erase(m, 0);"));
    assert!(e.contains("use `remove`"), "{}", e);
    let e = run_err("mapop3", &in_main("var v: Vector<i32> = Vector<i32>{}; const k: Vector<i32> = keys(v);"));
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
        &in_main("const m: HashMap<str, i32> = HashMap<str, i32>{\"a\": 1}; print(cast<str>(m[\"b\"]));"),
    );
    assert!(e.contains("is not in the HashMap"), "{}", e);
}
