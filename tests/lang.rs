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
                       import binz/float;\nimport binz/container;\nimport binz/map;\n\
                       import binz/random;\n";

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

/// A project on disk, since `@root` is the directory of the entry file and a
/// module has to be a real file next to it. `main.binz` is always the entry.
/// The directory name has to be unique for the same reason `write_temp`'s
/// file name does -- tests run in parallel.
fn write_project(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("binz_proj_{}", name));
    let _ = std::fs::remove_dir_all(&dir);
    for (rel, src) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::File::create(&path).unwrap().write_all(src.as_bytes()).unwrap();
    }
    dir.join("main.binz")
}

fn run_project(name: &str, files: &[(&str, &str)]) -> std::process::Output {
    let path = write_project(name, files);
    Command::new(env!("CARGO_BIN_EXE_binz")).arg("run").arg(&path).output().unwrap()
}

fn run_project_ok(name: &str, files: &[(&str, &str)]) -> String {
    let out = run_project(name, files);
    assert!(out.status.success(), "program failed:\n{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

fn run_project_err(name: &str, files: &[(&str, &str)]) -> String {
    let out = run_project(name, files);
    assert!(!out.status.success(), "expected failure, program succeeded");
    String::from_utf8(out.stderr).unwrap()
}

/// `binz test` over a project, which is how a test run is always started:
/// `@root` needs a real directory, and a stub needs a module to replace a
/// function of.
fn test_project(name: &str, files: &[(&str, &str)]) -> std::process::Output {
    let path = write_project(name, files);
    Command::new(env!("CARGO_BIN_EXE_binz")).arg("test").arg(&path).output().unwrap()
}

/// Runs the tests of a project, asserting every one of them passed, and
/// returns the report.
fn tests_pass(name: &str, files: &[(&str, &str)]) -> String {
    let out = test_project(name, files);
    let report = String::from_utf8(out.stdout).unwrap();
    assert!(
        out.status.success(),
        "tests failed:\n{}\n{}",
        report,
        String::from_utf8_lossy(&out.stderr)
    );
    report
}

/// Runs tests expected to fail -- either because one of them did, or because
/// the program was rejected -- and returns stdout and stderr together, since
/// a failed assertion is reported on one and a diagnostic on the other.
fn tests_fail(name: &str, files: &[(&str, &str)]) -> String {
    let out = test_project(name, files);
    assert!(!out.status.success(), "expected a failure, the run succeeded");
    format!(
        "{}{}",
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap()
    )
}

/// The module most of the test-library cases stub: one function, with a side
/// effect loud enough that a stub that did not take is obvious.
const RATES: (&str, &str) = (
    "rates.binz",
    "import binz/io;\n\
     function lookup(country: str): f64 {\n\
         io.print(\"the real rates module ran\");\n\
         return 1.0;\n\
     }\n",
);

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
            io.print(cast<str>(map.size(ages)) + " " + cast<str>(ages["ana"]));
            io.print(cast<str>(map.contains(ages, "bruno")) + " " + cast<str>(map.contains(ages, "zed")));
            io.print(cast<str>(map.remove(ages, "bruno")) + " " + cast<str>(map.remove(ages, "bruno")));
            const ks: Vector<str> = map.keys(ages);
            var i: i32 = 0;
            var line: str = "";
            while (i < container.size(ks)) {
                line = line + ks[i] + "=" + cast<str>(ages[ks[i]]) + " ";
                i = i + 1;
            }
            io.print(line);
            map.clear(ages);
            io.print(cast<str>(map.size(ages)));
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
            const mine: HashMap<str, Vector<i32>> = map.copy(a);
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
            io.print(cast<str>(map.size(m)) + " " + m[1]);
        "#,
        ),
    );
    assert_eq!(out, "1 uno\n");
}

/// `binz/map` and `binz/container` are two type families: each sends the
/// other's types back with the line that does work.
#[test]
fn a_map_and_a_container_each_name_the_other() {
    let m = "var m: HashMap<str, i32> = HashMap<str, i32>{}; ";
    let e = run_err("mapop", &in_main(&format!("{}container.push(m, 1);", m)));
    assert!(e.contains("write `m[key] = value`"), "{}", e);
    let e = run_err("mapop2", &in_main(&format!("{}container.erase(m, 0);", m)));
    assert!(e.contains("use `map.remove`"), "{}", e);
    let e = run_err("mapop4", &in_main(&format!("{}io.print(cast<str>(container.size(m)));", m)));
    assert!(e.contains("a map answers `map.size`"), "{}", e);

    let v = "var v: Vector<i32> = Vector<i32>{}; ";
    let e = run_err("mapop3", &in_main(&format!("{}const k: Vector<i32> = map.keys(v);", v)));
    assert!(e.contains("`map.keys` needs a map, found `Vector<i32>`"), "{}", e);
    let e = run_err("mapop5", &in_main(&format!("{}io.print(cast<str>(map.size(v)));", v)));
    assert!(e.contains("a container answers `container.size`"), "{}", e);
}

/// The verbs a map deliberately refuses name the one line that works,
/// rather than pointing at `binz/container`, which refuses a map too.
#[test]
fn rejects_the_verbs_a_map_does_not_have() {
    let m = "var m: HashMap<str, i32> = HashMap<str, i32>{}; ";
    let e = run_err("mapno1", &in_main(&format!("{}map.add(m, \"a\");", m)));
    assert!(e.contains("`binz/map` has no `add`: write `m[key] = value`"), "{}", e);
    let e = run_err("mapno2", &in_main(&format!("{}map.find(m, \"a\");", m)));
    assert!(e.contains("use `map.contains`"), "{}", e);
    let e = run_err("mapno3", &in_main(&format!("{}map.values(m);", m)));
    assert!(e.contains("read `m[key]` while walking `map.keys(m)`"), "{}", e);
    let e = run_err("mapno4", &in_main("var v: Vector<i32> = Vector<i32>{}; container.keys(v);"));
    assert!(e.contains("`binz/container` has no `keys`: only a map has keys"), "{}", e);
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

/// The one thing that tells `SortedMap` from `HashMap`: `map.keys` answers
/// ascending instead of in insertion order. Everything else is the same.
#[test]
fn a_sorted_map_hands_back_its_keys_in_order() {
    let out = run_ok(
        "smorder",
        &in_main(
            r#"
            var by_name: SortedMap<str, i32> = SortedMap<str, i32>{"pear": 3, "apple": 1};
            by_name["fig"] = 2;
            by_name["apple"] = 10;
            const ks: Vector<str> = map.keys(by_name);
            var i: i32 = 0;
            var line: str = "";
            while (i < container.size(ks)) {
                line = line + ks[i] + "=" + cast<str>(by_name[ks[i]]) + " ";
                i = i + 1;
            }
            io.print(line);

            const same: HashMap<str, i32> = HashMap<str, i32>{"pear": 3, "apple": 1, "fig": 2};
            const hk: Vector<str> = map.keys(same);
            io.print(hk[0] + " " + hk[1] + " " + hk[2]);
        "#,
        ),
    );
    assert_eq!(out, "apple=10 fig=2 pear=3 \npear apple fig\n");
}

/// A `SortedMap` answers every member of `binz/map`, and aliases and copies
/// exactly as the other heap containers do.
#[test]
fn a_sorted_map_answers_every_map_verb() {
    let out = run_ok(
        "smverbs",
        &in_main(
            r#"
            var m: SortedMap<i32, str> = SortedMap<i32, str>{3: "c", 1: "a"};
            m[2] = "b";
            io.print(cast<str>(map.size(m)) + " " + m[2]);
            io.print(cast<str>(map.contains(m, 2)) + " " + cast<str>(map.contains(m, 9)));
            io.print(cast<str>(map.remove(m, 2)) + " " + cast<str>(map.remove(m, 2)));

            const aliased: SortedMap<i32, str> = m;
            aliased[7] = "g";
            const mine: SortedMap<i32, str> = map.copy(m);
            mine[8] = "h";
            io.print(cast<str>(map.size(m)) + " " + cast<str>(map.size(mine)));

            map.clear(m);
            io.print(cast<str>(map.size(m)));
        "#,
        ),
    );
    assert_eq!(out, "3 b\ntrue false\ntrue false\n3 4\n0\n");
}

/// The red-black tree driven from binZ itself: every key inserted, every
/// other one deleted, and the survivors still ascending.
#[test]
fn a_sorted_map_stays_ordered_through_churn() {
    let out = run_ok(
        "smchurn",
        &in_main(
            r#"
            var m: SortedMap<i32, i32> = SortedMap<i32, i32>{};
            var i: i32 = 0;
            while (i < 500) {
                m[(i * 37) % 500] = i;
                i = i + 1;
            }
            i = 0;
            while (i < 500) {
                map.remove(m, i);
                i = i + 2;
            }
            const ks: Vector<i32> = map.keys(m);
            var ordered: bool = true;
            i = 1;
            while (i < container.size(ks)) {
                if (ks[i] <= ks[i - 1]) { ordered = false; }
                i = i + 1;
            }
            io.print(cast<str>(map.size(m)) + " " + cast<str>(ordered) + " " + cast<str>(ks[0]));
        "#,
        ),
    );
    assert_eq!(out, "250 true 1\n");
}

/// A sorted map has to compare its keys, so it inherits the rule that put
/// NaN out of a `SortedSet`.
#[test]
fn a_sorted_map_rejects_nan_as_a_key() {
    let e = run_err(
        "smnan",
        &in_main("var m: SortedMap<f64, i32> = SortedMap<f64, i32>{}; m[0.0 / 0.0] = 1;"),
    );
    assert!(e.contains("NaN"), "{}", e);
}

/// No `null`, so an absent key traps and the message names the guard.
#[test]
fn traps_on_a_missing_sorted_map_key() {
    let e = run_err(
        "smmiss",
        &in_main("const m: SortedMap<str, i32> = SortedMap<str, i32>{\"a\": 1}; io.print(cast<str>(m[\"b\"]));"),
    );
    assert!(e.contains("is not in the SortedMap"), "{}", e);
    assert!(e.contains("`map.contains`"), "{}", e);
}

/// A map is reached by key and nothing else, whichever map it is.
#[test]
fn rejects_positional_access_to_a_sorted_map() {
    let e = run_err(
        "smpos",
        &in_main("var m: SortedMap<str, i32> = SortedMap<str, i32>{}; container.push(m, 1);"),
    );
    assert!(e.contains("write `m[key] = value`"), "{}", e);
    let e = run_err(
        "smkey",
        &in_main("var m: SortedMap<str, i32> = SortedMap<str, i32>{}; m[1] = 2;"),
    );
    assert!(e.contains("in a key: expected `str`"), "{}", e);
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

// ----------------------------------------------------------- local modules

#[test]
fn a_local_module_exports_every_function_it_defines() {
    let out = run_project_ok(
        "exports",
        &[
            (
                "utils/math.binz",
                "import binz/int;\n\
                 function square(n: i32): i32 { return n * n; }\n\
                 function clamp(n: i32, lo: i32, hi: i32): i32 {\n\
                     return int.min(int.max(n, lo), hi);\n\
                 }\n",
            ),
            (
                "main.binz",
                "import binz/io;\n\
                 import @root/utils/math.binz;\n\
                 function main(): i32 {\n\
                     io.print(cast<str>(math.square(7)));\n\
                     io.print(cast<str>(math.clamp(42, 0, 10)));\n\
                     return 0;\n\
                 }\n",
            ),
        ],
    );
    assert_eq!(out, "49\n10\n");
}

/// A module's function has a type that can be written in binZ, so it is a
/// value -- the same rule that makes `io.print` one and `container.size` not.
#[test]
fn a_module_function_is_a_first_class_value() {
    let out = run_project_ok(
        "modvalue",
        &[
            ("utils/math.binz", "function square(n: i32): i32 { return n * n; }\n"),
            (
                "main.binz",
                "import binz/io;\n\
                 import @root/utils/math.binz;\n\
                 function apply(f: function(i32): i32, n: i32): i32 { return f(n); }\n\
                 function main(): i32 {\n\
                     const sq: function(i32): i32 = math.square;\n\
                     io.print(cast<str>(apply(sq, 9)));\n\
                     return 0;\n\
                 }\n",
            ),
        ],
    );
    assert_eq!(out, "81\n");
}

/// A module may import modules of its own, and the graph is loaded from the
/// entry file outwards.
#[test]
fn a_module_may_import_another_module() {
    let out = run_project_ok(
        "transitive",
        &[
            ("math.binz", "function square(n: i32): i32 { return n * n; }\n"),
            (
                "shape.binz",
                "import @root/math.binz;\n\
                 function area(side: i32): i32 { return math.square(side); }\n",
            ),
            (
                "main.binz",
                "import binz/io;\n\
                 import @root/shape.binz;\n\
                 function main(): i32 { io.print(cast<str>(shape.area(5))); return 0; }\n",
            ),
        ],
    );
    assert_eq!(out, "25\n");
}

/// Names are per file: an import is visible only where it is written, and two
/// files may define the same function without either shadowing the other.
#[test]
fn names_belong_to_one_file() {
    let out = run_project_ok(
        "perfile",
        &[
            ("alpha.binz", "function add(a: i32, b: i32): i32 { return a + b; }\n"),
            (
                "main.binz",
                "import binz/io;\n\
                 import @root/alpha.binz;\n\
                 function add(a: i32, b: i32): i32 { return a * b; }\n\
                 function main(): i32 {\n\
                     io.print(cast<str>(add(3, 4)) + \" \" + cast<str>(alpha.add(3, 4)));\n\
                     return 0;\n\
                 }\n",
            ),
        ],
    );
    assert_eq!(out, "12 7\n");
}

/// The same file reached by two paths through the graph is one module, so its
/// functions are compiled once and are the same functions.
#[test]
fn a_module_is_compiled_once_however_often_it_is_imported() {
    let path = write_project(
        "diamond",
        &[
            ("math.binz", "function square(n: i32): i32 { return n * n; }\n"),
            (
                "shape.binz",
                "import @root/math.binz;\n\
                 function area(side: i32): i32 { return math.square(side); }\n",
            ),
            (
                "main.binz",
                "import @root/math.binz;\n\
                 import @root/shape.binz;\n\
                 function main(): i32 { return math.square(2) - shape.area(2); }\n",
            ),
        ],
    );
    let out = Command::new(env!("CARGO_BIN_EXE_binz")).arg("build").arg(&path).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    // `main`, `area`, `square` -- `square` once, not once per importer.
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("3 functions"),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn rejects_an_import_cycle() {
    let err = run_project_err(
        "cycle",
        &[
            ("a.binz", "import @root/b.binz;\nfunction f(): i32 { return b.g(); }\n"),
            ("b.binz", "import @root/a.binz;\nfunction g(): i32 { return a.f(); }\n"),
            ("main.binz", "import @root/a.binz;\nfunction main(): i32 { return a.f(); }\n"),
        ],
    );
    assert!(
        err.contains("import cycle: @root/a.binz -> @root/b.binz -> @root/a.binz"),
        "{}",
        err
    );
}

#[test]
fn rejects_a_module_that_defines_main() {
    let err = run_project_err(
        "modmain",
        &[
            ("lib.binz", "function main(): i32 { return 0; }\n"),
            ("main.binz", "import @root/lib.binz;\nfunction main(): i32 { return 0; }\n"),
        ],
    );
    assert!(err.contains("only the file passed to `binz`"), "{}", err);
}

#[test]
fn rejects_a_module_file_that_is_not_there() {
    let err = run_project_err(
        "missing",
        &[("main.binz", "import @root/nope.binz;\nfunction main(): i32 { return 0; }\n")],
    );
    assert!(err.contains("cannot read `@root/nope.binz`"), "{}", err);
}

/// The file name *is* the binding, so it has to be an identifier, and the
/// diagnostic says so rather than complaining about a stray `-`.
#[test]
fn rejects_a_module_file_name_that_is_not_one_lowercase_word() {
    let err = run_project_err(
        "hyphen",
        &[("main.binz", "import @root/some-module.binz;\nfunction main(): i32 { return 0; }\n")],
    );
    assert!(err.contains("`-` cannot appear in a module file name"), "{}", err);

    let err = run_project_err(
        "capital",
        &[("main.binz", "import @root/Math.binz;\nfunction main(): i32 { return 0; }\n")],
    );
    assert!(err.contains("is lowercase, like every module name"), "{}", err);
}

#[test]
fn rejects_a_local_import_without_its_extension() {
    let err = run_project_err(
        "noext",
        &[("main.binz", "import @root/math;\nfunction main(): i32 { return 0; }\n")],
    );
    assert!(err.contains("write `@root/math.binz`"), "{}", err);
}

#[test]
fn rejects_an_anchor_that_is_not_root() {
    let err = run_project_err(
        "anchor",
        &[("main.binz", "import @project/math.binz;\nfunction main(): i32 { return 0; }\n")],
    );
    assert!(err.contains("every local import starts at `@root`"), "{}", err);
}

/// `io.print` means one thing everywhere, so a file of the project cannot
/// take a standard library module's name.
#[test]
fn rejects_a_local_module_named_after_a_standard_library_module() {
    let err = run_project_err(
        "shadowstd",
        &[
            ("io.binz", "function print(s: str): void { return; }\n"),
            ("main.binz", "import @root/io.binz;\nfunction main(): i32 { return 0; }\n"),
        ],
    );
    assert!(err.contains("is the standard library module `binz/io`"), "{}", err);
}

#[test]
fn a_local_module_owns_its_binding_in_the_file_that_imports_it() {
    let err = run_project_err(
        "binding",
        &[
            ("math.binz", "function square(n: i32): i32 { return n * n; }\n"),
            (
                "main.binz",
                "import @root/math.binz;\n\
                 function main(): i32 { const math: i32 = 1; return math; }\n",
            ),
        ],
    );
    assert!(err.contains("is the imported module `@root/math.binz`"), "{}", err);
}

#[test]
fn rejects_importing_the_same_module_twice() {
    let err = run_project_err(
        "twice",
        &[
            ("math.binz", "function square(n: i32): i32 { return n * n; }\n"),
            (
                "main.binz",
                "import @root/math.binz;\nimport @root/math.binz;\n\
                 function main(): i32 { return 0; }\n",
            ),
        ],
    );
    assert!(err.contains("`@root/math.binz` is already imported in this file"), "{}", err);
}

#[test]
fn rejects_a_member_the_module_does_not_define() {
    let err = run_project_err(
        "nomember",
        &[
            ("math.binz", "function square(n: i32): i32 { return n * n; }\n"),
            (
                "main.binz",
                "import @root/math.binz;\nfunction main(): i32 { return math.cube(2); }\n",
            ),
        ],
    );
    assert!(err.contains("`@root/math.binz` has no function `cube`"), "{}", err);
}

/// A module exports its functions and nothing else, so a struct stays inside
/// the file that declares it -- including in the signature of an exported
/// function, which the importing file would have no way to write down.
#[test]
fn rejects_reaching_a_struct_through_a_module() {
    let err = run_project_err(
        "structexport",
        &[
            ("shapes.binz", "struct P { x: i32 }\nfunction make(): P { return P{x: 1}; }\n"),
            (
                "main.binz",
                "import @root/shapes.binz;\nfunction main(): i32 { return shapes.make().x; }\n",
            ),
        ],
    );
    assert!(err.contains("a module exports its functions, not its types"), "{}", err);

    let err = run_project_err(
        "structname",
        &[
            ("shapes.binz", "struct P { x: i32 }\nfunction one(): i32 { return 1; }\n"),
            (
                "main.binz",
                "import @root/shapes.binz;\nfunction main(): i32 { return shapes.P(); }\n",
            ),
        ],
    );
    assert!(err.contains("is a struct in `@root/shapes.binz`"), "{}", err);
}

/// A span alone no longer says where an error is, so every diagnostic names
/// the file it came from.
#[test]
fn a_diagnostic_names_the_file_it_came_from() {
    let err = run_project_err(
        "blame",
        &[
            ("broken.binz", "function bad(): i32 { return \"x\"; }\n"),
            (
                "main.binz",
                "import @root/broken.binz;\nfunction main(): i32 { return broken.bad(); }\n",
            ),
        ],
    );
    assert!(err.contains("broken.binz:1:30"), "{}", err);
    assert!(err.contains("expected `i32`, found `str`"), "{}", err);
}

/// A module imports what it uses; the file that imports it is not a scope.
#[test]
fn a_module_imports_its_own_dependencies() {
    let err = run_project_err(
        "ownimports",
        &[
            ("math.binz", "function shout(): void { io.print(\"hi\"); }\n"),
            (
                "main.binz",
                "import binz/io;\nimport @root/math.binz;\n\
                 function main(): i32 { math.shout(); return 0; }\n",
            ),
        ],
    );
    assert!(err.contains("`io` is not imported"), "{}", err);
}

// ------------------------------------------- `as`, only where it is forced

/// Two directories may hold two files of the same name. That is the only
/// situation `as` exists for -- and then every one of them is renamed, so the
/// name is never the default for one import and a rename for another.
#[test]
fn two_modules_of_the_same_name_are_both_renamed() {
    let out = run_project_ok(
        "bothrenamed",
        &[
            ("geometry/math.binz", "function area(side: i32): i32 { return side * side; }\n"),
            ("utils/math.binz", "function double(n: i32): i32 { return n + n; }\n"),
            (
                "main.binz",
                "import binz/io;\n\
                 import @root/geometry/math.binz as geometryMath;\n\
                 import @root/utils/math.binz as utilsMath;\n\
                 function main(): i32 {\n\
                     io.print(cast<str>(geometryMath.area(5)) + \" \"\n\
                              + cast<str>(utilsMath.double(5)));\n\
                     return 0;\n\
                 }\n",
            ),
        ],
    );
    assert_eq!(out, "25 10\n");
}

#[test]
fn rejects_a_name_clash_with_neither_import_renamed() {
    let err = run_project_err(
        "clashbare",
        &[
            ("geometry/math.binz", "function area(side: i32): i32 { return side * side; }\n"),
            ("utils/math.binz", "function double(n: i32): i32 { return n + n; }\n"),
            (
                "main.binz",
                "import @root/geometry/math.binz;\nimport @root/utils/math.binz;\n\
                 function main(): i32 { return 0; }\n",
            ),
        ],
    );
    assert!(
        err.contains(
            "`@root/geometry/math.binz` and `@root/utils/math.binz` are named `math`"
        ),
        "{}",
        err
    );
    // The diagnostic names the one rename this import may have.
    assert!(
        err.contains("write `import @root/geometry/math.binz as geometryMath;`"),
        "{}",
        err
    );
}

/// Renaming one of them is not enough: the other would still hold the name by
/// default, which is the asymmetry the rule exists to prevent.
#[test]
fn rejects_a_name_clash_with_only_one_import_renamed() {
    let err = run_project_err(
        "clashhalf",
        &[
            ("geometry/math.binz", "function area(side: i32): i32 { return side * side; }\n"),
            ("utils/math.binz", "function double(n: i32): i32 { return n + n; }\n"),
            (
                "main.binz",
                "import @root/geometry/math.binz;\nimport @root/utils/math.binz as utilsMath;\n\
                 function main(): i32 { return 0; }\n",
            ),
        ],
    );
    assert!(err.contains("every one of them is renamed"), "{}", err);
    assert!(err.contains("main.binz:1:1"), "{}", err);
}

/// Without a clash there is nothing to resolve, and `as` would be a second
/// spelling for one module.
#[test]
fn rejects_a_rename_with_nothing_to_resolve() {
    let err = run_project_err(
        "lonerename",
        &[
            ("utils/math.binz", "function double(n: i32): i32 { return n + n; }\n"),
            (
                "main.binz",
                "import @root/utils/math.binz as utilsMath;\nfunction main(): i32 { return 0; }\n",
            ),
        ],
    );
    assert!(err.contains("`as` renames a module only when two or more"), "{}", err);
    assert!(err.contains("is the only `math` in this file"), "{}", err);
}

/// No two standard library modules are named the same, so `as` can never be
/// forced on one -- `io.print` reads identically in every file.
#[test]
fn rejects_renaming_a_standard_library_module() {
    let err = run_project_err(
        "stdrename",
        &[("main.binz", "import binz/io as out;\nfunction main(): i32 { return 0; }\n")],
    );
    assert!(err.contains("`binz/io` is always reached as `io`"), "{}", err);
}

/// The rename is not a choice: it is the directory and the file name joined.
/// Every other spelling is refused, including the ones the old free-form rule
/// allowed -- a bare lowercase word, or a suffix that is not in the path.
#[test]
fn a_rename_is_the_one_the_path_gives() {
    for wrong in ["geomath", "geometrymath", "geometryMath1", "math", "m", "GeometryMath"] {
        let err = run_project_err(
            &format!("derived_{}", wrong),
            &[
                ("geometry/math.binz", "function area(side: i32): i32 { return side * side; }\n"),
                ("utils/math.binz", "function double(n: i32): i32 { return n + n; }\n"),
                (
                    "main.binz",
                    &format!(
                        "import @root/geometry/math.binz as {};\n\
                         import @root/utils/math.binz as utilsMath;\n\
                         function main(): i32 {{ return 0; }}\n",
                        wrong
                    ),
                ),
            ],
        );
        assert!(
            err.contains("the rename of `@root/geometry/math.binz` is `geometryMath`"),
            "`as {}` was not refused: {}",
            wrong,
            err
        );
    }
}

/// A file directly under the anchor has no directory to borrow, so it uses
/// the anchor's own name.
#[test]
fn a_module_under_the_root_is_renamed_with_root() {
    let out = run_project_ok(
        "rootalias",
        &[
            ("math.binz", "function double(n: i32): i32 { return n + n; }\n"),
            ("utils/math.binz", "function triple(n: i32): i32 { return n + n + n; }\n"),
            (
                "main.binz",
                "import binz/io;\n\
                 import @root/math.binz as rootMath;\n\
                 import @root/utils/math.binz as utilsMath;\n\
                 function main(): i32 {\n\
                     io.print(cast<str>(rootMath.double(5)) + \" \"\n\
                              + cast<str>(utilsMath.triple(5)));\n\
                     return 0;\n\
                 }\n",
            ),
        ],
    );
    assert_eq!(out, "10 15\n");
}

/// A clash is per file, so the same module is `math` in a file that imports
/// only it, and renamed in one that does not.
#[test]
fn a_clash_is_per_file() {
    let out = run_project_ok(
        "perfileclash",
        &[
            ("geometry/math.binz", "function area(side: i32): i32 { return side * side; }\n"),
            ("utils/math.binz", "function double(n: i32): i32 { return n + n; }\n"),
            (
                "only.binz",
                "import @root/utils/math.binz;\n\
                 function twice(n: i32): i32 { return math.double(n); }\n",
            ),
            (
                "main.binz",
                "import binz/io;\n\
                 import @root/only.binz;\n\
                 import @root/geometry/math.binz as geometryMath;\n\
                 import @root/utils/math.binz as utilsMath;\n\
                 function main(): i32 {\n\
                     io.print(cast<str>(only.twice(4)) + \" \"\n\
                              + cast<str>(geometryMath.area(3)) + \" \"\n\
                              + cast<str>(utilsMath.double(1)));\n\
                     return 0;\n\
                 }\n",
            ),
        ],
    );
    assert_eq!(out, "8 9 2\n");
}



// ------------------------------------------------------------- binz/test

/// A test lives in the file it tests, is marked by `@test`, and says what it
/// has to say by failing.
#[test]
fn a_test_passes_and_a_test_fails() {
    let report = tests_fail(
        "testbasics",
        &[(
            "main.binz",
            "import binz/test;\n\
             function double(n: i32): i32 { return n + n; }\n\
             function main(): i32 { return 0; }\n\
             @test function doublesPositives(): void {\n\
                 test.equal(double(21), 42);\n\
             }\n\
             @test function doublesNegatives(): void {\n\
                 test.equal(double(-1), 0);\n\
             }\n",
        )],
    );
    assert!(report.contains("ok    doublesPositives"), "{}", report);
    assert!(report.contains("FAIL  doublesNegatives"), "{}", report);
    assert!(report.contains("expected `0`, found `-2`"), "{}", report);
    assert!(report.contains("1 passed, 1 failed"), "{}", report);
}

/// The `@test` tag is what marks a test, so the name may not say it again.
#[test]
fn a_test_is_not_named_test_anything() {
    for name in ["testDoubles", "TestDoubles", "test_doubles"] {
        let err = tests_fail(
            "testnamed",
            &[(
                "main.binz",
                &format!(
                    "import binz/test;\n\
                     function main(): i32 {{ return 0; }}\n\
                     @test function {}(): void {{ test.equal(1, 1); }}\n",
                    name
                ),
            )],
        );
        assert!(err.contains("a test name cannot start with `test`"), "{}: {}", name, err);
    }
}

/// The whole point: a stub replaces the module's function wherever it is
/// called from, so nothing has to be passed in to reach it.
#[test]
fn a_stub_replaces_a_module_function_everywhere() {
    let report = tests_pass(
        "stubreplaces",
        &[
            RATES,
            (
                "main.binz",
                "import binz/test;\n\
                 import @root/rates.binz;\n\
                 function total(amount: f64, country: str): f64 {\n\
                     return amount * rates.lookup(country);\n\
                 }\n\
                 function main(): i32 { return 0; }\n\
                 @test function appliesTheRate(): void {\n\
                     stub rates.lookup(country: str): f64 { return 0.5; }\n\
                     test.equal(total(100.0, \"br\"), 50.0);\n\
                 }\n",
            ),
        ],
    );
    assert!(!report.contains("the real rates module ran"), "{}", report);
    assert!(report.contains("1 passed, 0 failed"), "{}", report);
}

/// A stub dies with the test that installed it: the next one gets the real
/// function back, because each test runs in a machine of its own.
#[test]
fn a_stub_ends_with_its_test() {
    let report = tests_pass(
        "stubscope",
        &[
            RATES,
            (
                "main.binz",
                "import binz/test;\n\
                 import @root/rates.binz;\n\
                 function main(): i32 { return 0; }\n\
                 @test function usesTheStub(): void {\n\
                     stub rates.lookup(country: str): f64 { return 0.5; }\n\
                     test.equal(rates.lookup(\"br\"), 0.5);\n\
                 }\n\
                 @test function usesTheRealThing(): void {\n\
                     test.equal(rates.lookup(\"br\"), 1.0);\n\
                 }\n",
            ),
        ],
    );
    assert!(report.contains("the real rates module ran"), "{}", report);
    assert!(report.contains("2 passed"), "{}", report);
}

/// Without globals or closures a stub cannot record anything, so counting is
/// the library's job -- and it is asserted with the same `test.equal`.
#[test]
fn calls_are_counted() {
    let report = tests_pass(
        "stubcalls",
        &[
            RATES,
            (
                "main.binz",
                "import binz/test;\n\
                 import @root/rates.binz;\n\
                 function twice(country: str): f64 {\n\
                     return rates.lookup(country) + rates.lookup(country);\n\
                 }\n\
                 function main(): i32 { return 0; }\n\
                 @test function callsTheModuleTwice(): void {\n\
                     stub rates.lookup(country: str): f64 { return 1.0; }\n\
                     test.equal(twice(\"br\"), 2.0);\n\
                     test.equal(test.calls(rates.lookup), 2);\n\
                 }\n\
                 @test function countsNothingBeforeItRuns(): void {\n\
                     stub rates.lookup(country: str): f64 { return 1.0; }\n\
                     test.equal(test.calls(rates.lookup), 0);\n\
                 }\n",
            ),
        ],
    );
    assert!(report.contains("2 passed"), "{}", report);
}

/// `binz test` runs the tests of the file it is given and of everything that
/// file imports, so pointing at the entry runs the project.
#[test]
fn a_test_run_covers_the_whole_import_graph() {
    let report = tests_pass(
        "testgraph",
        &[
            (
                "math.binz",
                "import binz/test;\n\
                 function double(n: i32): i32 { return n + n; }\n\
                 @test function doublesPositives(): void { test.equal(double(2), 4); }\n",
            ),
            (
                "main.binz",
                "import binz/test;\n\
                 import @root/math.binz;\n\
                 function main(): i32 { return 0; }\n\
                 @test function reachesTheModule(): void { test.equal(math.double(3), 6); }\n",
            ),
        ],
    );
    assert!(report.contains("doublesPositives"), "{}", report);
    assert!(report.contains("reachesTheModule"), "{}", report);
    assert!(report.contains("2 passed"), "{}", report);
}

/// A test run needs no program, which is what lets a module be tested on its
/// own -- a module may not define `main` at all.
#[test]
fn a_module_is_tested_without_a_main() {
    let dir = write_project(
        "testmodulealone",
        &[
            (
                "math.binz",
                "import binz/test;\n\
                 function double(n: i32): i32 { return n + n; }\n\
                 @test function doublesPositives(): void { test.equal(double(2), 4); }\n",
            ),
            ("main.binz", "function main(): i32 { return 0; }\n"),
        ],
    );
    let out = Command::new(env!("CARGO_BIN_EXE_binz"))
        .arg("test")
        .arg(dir.parent().unwrap().join("math.binz"))
        .output()
        .unwrap();
    let report = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success(), "{}{}", report, String::from_utf8_lossy(&out.stderr));
    assert!(report.contains("1 passed"), "{}", report);
}

/// A test is not in the artifact a program is built from, so it cannot be
/// called and weighs nothing.
#[test]
fn a_test_cannot_be_called() {
    let same = run_project_err(
        "testcallsame",
        &[(
            "main.binz",
            "import binz/test;\n\
             function main(): i32 { checksOne(); return 0; }\n\
             @test function checksOne(): void { test.equal(1, 1); }\n",
        )],
    );
    assert!(same.contains("is an `@test` function"), "{}", same);

    let across = run_project_err(
        "testcallmodule",
        &[
            (
                "math.binz",
                "import binz/test;\n\
                 function double(n: i32): i32 { return n + n; }\n\
                 @test function doublesPositives(): void { test.equal(double(2), 4); }\n",
            ),
            (
                "main.binz",
                "import @root/math.binz;\n\
                 function main(): i32 { math.doublesPositives(); return 0; }\n",
            ),
        ],
    );
    assert!(across.contains("exported to nobody"), "{}", across);
}

/// `binz/test` answers to the test that is running, so outside one there is
/// nothing for it to answer to.
#[test]
fn binz_test_is_reachable_only_from_a_test() {
    let err = run_project_err(
        "testoutside",
        &[(
            "main.binz",
            "import binz/test;\n\
             function main(): i32 { test.equal(1, 1); return 0; }\n",
        )],
    );
    assert!(err.contains("only reached from an `@test` function"), "{}", err);
}

/// A stub replaces a function of this project. `io.print` means the same
/// thing in every program, and a test does not get to change that.
#[test]
fn the_standard_library_is_not_stubbable() {
    let err = tests_fail(
        "stubstd",
        &[(
            "main.binz",
            "import binz/io;\n\
             import binz/test;\n\
             function main(): i32 { return 0; }\n\
             @test function printsNothing(): void {\n\
                 stub io.print(s: str): void { return; }\n\
                 test.equal(1, 1);\n\
             }\n",
        )],
    );
    assert!(err.contains("is the standard library"), "{}", err);
}

/// Every stub goes at the top of the test, so what a test replaced is read
/// once rather than hunted for.
#[test]
fn a_stub_goes_at_the_top_of_its_test() {
    let late = tests_fail(
        "stublate",
        &[
            RATES,
            (
                "main.binz",
                "import binz/test;\n\
                 import @root/rates.binz;\n\
                 function main(): i32 { return 0; }\n\
                 @test function ratesAreFake(): void {\n\
                     test.equal(1, 1);\n\
                     stub rates.lookup(country: str): f64 { return 0.5; }\n\
                 }\n",
            ),
        ],
    );
    assert!(late.contains("goes at the top of the test"), "{}", late);

    let outside = run_project_err(
        "stuboutside",
        &[
            RATES,
            (
                "main.binz",
                "import @root/rates.binz;\n\
                 function main(): i32 {\n\
                     stub rates.lookup(country: str): f64 { return 0.5; }\n\
                     return 0;\n\
                 }\n",
            ),
        ],
    );
    assert!(outside.contains("belongs at the top of an `@test` function"), "{}", outside);
}

/// A stub that has drifted from the function it fakes is the one bug a test
/// library must not hide, so the signature is written out and checked.
#[test]
fn a_stub_has_the_signature_of_what_it_replaces() {
    let err = tests_fail(
        "stubsig",
        &[
            RATES,
            (
                "main.binz",
                "import binz/test;\n\
                 import @root/rates.binz;\n\
                 function main(): i32 { return 0; }\n\
                 @test function ratesAreFake(): void {\n\
                     stub rates.lookup(country: i32): f64 { return 0.5; }\n\
                     test.equal(1, 1);\n\
                 }\n",
            ),
        ],
    );
    assert!(err.contains("does not have the signature of `rates.lookup`"), "{}", err);
    assert!(err.contains("function lookup(str): f64"), "{}", err);
}

/// One function, one replacement.
#[test]
fn a_function_is_stubbed_once_per_test() {
    let err = tests_fail(
        "stubtwice",
        &[
            RATES,
            (
                "main.binz",
                "import binz/test;\n\
                 import @root/rates.binz;\n\
                 function main(): i32 { return 0; }\n\
                 @test function ratesAreFake(): void {\n\
                     stub rates.lookup(country: str): f64 { return 0.5; }\n\
                     stub rates.lookup(country: str): f64 { return 0.25; }\n\
                     test.equal(1, 1);\n\
                 }\n",
            ),
        ],
    );
    assert!(err.contains("already stubbed in this test"), "{}", err);
}

/// `binz test` calls a test, and it has nothing to pass and nothing to read.
#[test]
fn a_test_takes_nothing_and_answers_void() {
    let params = tests_fail(
        "testparams",
        &[(
            "main.binz",
            "import binz/test;\n\
             function main(): i32 { return 0; }\n\
             @test function checksOne(x: i32): void { test.equal(x, 1); }\n",
        )],
    );
    assert!(params.contains("a test takes no arguments"), "{}", params);

    let ret = tests_fail(
        "testret",
        &[(
            "main.binz",
            "import binz/test;\n\
             function main(): i32 { return 0; }\n\
             @test function checksOne(): i32 { test.equal(1, 1); return 0; }\n",
        )],
    );
    assert!(ret.contains("a test answers `void`"), "{}", ret);
}

/// `test.equal` compares what `==` compares, and both sides are one type --
/// binZ converts nothing on its own anywhere else either.
#[test]
fn test_equal_compares_one_type() {
    let mixed = tests_fail(
        "testequalmixed",
        &[(
            "main.binz",
            "import binz/test;\n\
             function main(): i32 { return 0; }\n\
             @test function comparesAcrossWidths(): void {\n\
                 const a: i64 = 1;\n\
                 const b: i32 = 1;\n\
                 test.equal(a, b);\n\
             }\n",
        )],
    );
    assert!(mixed.contains("expected `i64`"), "{}", mixed);

    let container = tests_fail(
        "testequalcontainer",
        &[(
            "main.binz",
            "import binz/test;\n\
             import binz/container;\n\
             function main(): i32 { return 0; }\n\
             @test function comparesVectors(): void {\n\
                 const v: Vector<i32> = Vector<i32>{ 1 };\n\
                 test.equal(v, v);\n\
             }\n",
        )],
    );
    assert!(container.contains("compares what `==` compares"), "{}", container);
}

/// `test.calls` counts a function of the program: a standard library call is
/// not one of the program's functions, so there is no id to count it under.
#[test]
fn a_standard_library_call_is_not_counted() {
    let err = tests_fail(
        "testcallsstd",
        &[(
            "main.binz",
            "import binz/io;\n\
             import binz/test;\n\
             function main(): i32 { return 0; }\n\
             @test function countsPrints(): void {\n\
                 test.equal(test.calls(io.print), 0);\n\
             }\n",
        )],
    );
    assert!(err.contains("is the standard library"), "{}", err);
}

/// `@test` is the only tag there is.
#[test]
fn test_is_the_only_tag() {
    let err = run_project_err(
        "unknowntag",
        &[(
            "main.binz",
            "function main(): i32 { return 0; }\n\
             @bench function measuresIt(): void { return; }\n",
        )],
    );
    assert!(err.contains("`@bench` is not a tag"), "{}", err);
}

/// A test and a function of the same file share one namespace, so one name
/// still means one thing.
#[test]
fn a_test_cannot_take_a_functions_name() {
    let err = run_project_err(
        "testnameclash",
        &[(
            "main.binz",
            "import binz/test;\n\
             function work(): i32 { return 1; }\n\
             @test function work(): void { test.equal(1, 1); }\n\
             function main(): i32 { return work(); }\n",
        )],
    );
    assert!(err.contains("`work` is already defined"), "{}", err);
}

/// `binz build` and `binz run` do not compile a test at all -- so a test that
/// no longer compiles stops `binz test`, and nothing else. That is the cost
/// of tests weighing nothing in the artifact.
#[test]
fn a_program_runs_with_a_broken_test() {
    let out = run_project_ok(
        "brokentest",
        &[(
            "main.binz",
            "import binz/io;\n\
             import binz/test;\n\
             function main(): i32 { io.print(\"ran\"); return 0; }\n\
             @test function checksSomethingGone(): void { test.equal(gone(), 1); }\n",
        )],
    );
    assert_eq!(out, "ran\n");

    let err = tests_fail(
        "brokentestfails",
        &[(
            "main.binz",
            "import binz/io;\n\
             import binz/test;\n\
             function main(): i32 { io.print(\"ran\"); return 0; }\n\
             @test function checksSomethingGone(): void { test.equal(gone(), 1); }\n",
        )],
    );
    assert!(err.contains("`gone` is not defined"), "{}", err);
}

/// The case equality cannot state: a branch that should not have been
/// reached.
#[test]
fn a_test_can_fail_outright() {
    let report = tests_fail(
        "testfail",
        &[(
            "main.binz",
            "import binz/test;\n\
             function main(): i32 { return 0; }\n\
             @test function refusesTheEmptyCase(): void {\n\
                 test.fail(\"an empty country should never be looked up\");\n\
             }\n",
        )],
    );
    assert!(report.contains("an empty country should never be looked up"), "{}", report);
}

/// A stub replaces the function itself, so there is no calling through to the
/// real one -- the call would land back in the stub, forever.
#[test]
fn a_stub_cannot_call_what_it_replaces() {
    let err = tests_fail(
        "stubrecurses",
        &[
            RATES,
            (
                "main.binz",
                "import binz/test;\n\
                 import @root/rates.binz;\n\
                 function main(): i32 { return 0; }\n\
                 @test function ratesAreFake(): void {\n\
                     stub rates.lookup(country: str): f64 {\n\
                         return rates.lookup(country);\n\
                     }\n\
                     test.equal(1, 1);\n\
                 }\n",
            ),
        ],
    );
    assert!(err.contains("a stub cannot reach `lookup`"), "{}", err);
}

// ----------------------------------------------------------- binz/random

/// `random.f64()` answers `[0, 1)` -- never 1.0, since that is the half-open
/// range every other language's random float agrees on.
#[test]
fn a_random_f64_is_a_fraction() {
    let out = run_ok(
        "randomf64",
        &in_main(
            r#"
            var i: i32 = 0;
            var lo: bool = true;
            var hi: bool = true;
            var same: i32 = 0;
            const first: f64 = random.f64();
            while (i < 5000) {
                const r: f64 = random.f64();
                if (r < 0.0) { lo = false; }
                if (r >= 1.0) { hi = false; }
                if (r == first) { same = same + 1; }
                i = i + 1;
            }
            io.print(cast<str>(lo) + " " + cast<str>(hi) + " " + cast<str>(same < 5));
        "#,
        ),
    );
    assert_eq!(out, "true true true\n");
}

/// Both ends are included, so `random.i32(1, 6)` is a die -- and over enough
/// throws every face turns up.
#[test]
fn a_random_i32_covers_its_range() {
    let out = run_ok(
        "randomi32",
        &in_main(
            r#"
            var counts: Vector<i32> = Vector<i32>{ 0, 0, 0, 0, 0, 0 };
            var i: i32 = 0;
            while (i < 6000) {
                const d: i32 = random.i32(1, 6);
                counts[d - 1] = counts[d - 1] + 1;
                i = i + 1;
            }
            var j: i32 = 0;
            var every: bool = true;
            while (j < 6) {
                if (counts[j] < 500) { every = false; }
                j = j + 1;
            }
            io.print(cast<str>(every) + " " + cast<str>(random.i32(4, 4)));
        "#,
        ),
    );
    assert_eq!(out, "true 4\n");
}

/// An empty range has no value to answer with, and binZ has no `null`.
#[test]
fn a_backwards_random_range_traps() {
    let e = run_err("randomempty", &in_main("io.print(cast<str>(random.i32(6, 1)));"));
    assert!(e.contains("empty range 6..1"), "{}", e);
}

/// Both members are monomorphic, so both are ordinary function values -- the
/// same rule that makes `io.print` one.
#[test]
fn random_members_are_values() {
    let out = run_ok(
        "randomvalues",
        &in_main(
            r#"
            const fraction: function(): f64 = random.f64;
            const die: function(i32, i32): i32 = random.i32;
            io.print(cast<str>(fraction() < 1.0) + " " + cast<str>(die(3, 3)));
        "#,
        ),
    );
    assert_eq!(out, "true 3\n");
}

/// The standard library is not stubbable, so a test replaces the module
/// function that reads a random number -- which is the whole seam.
#[test]
fn a_random_dependency_is_stubbed_at_the_module() {
    let report = tests_pass(
        "randomstub",
        &[
            (
                "rates.binz",
                "import binz/random;\n\
                 function lookup(country: str): f64 { return random.f64(); }\n",
            ),
            (
                "main.binz",
                "import binz/test;\n\
                 import @root/rates.binz;\n\
                 function total(amount: f64, country: str): f64 {\n\
                     return amount * rates.lookup(country);\n\
                 }\n\
                 @test function appliesTheRate(): void {\n\
                     stub rates.lookup(country: str): f64 { return 0.5; }\n\
                     test.equal(total(100.0, \"br\"), 50.0);\n\
                 }\n\
                 @test function withoutAStubOnlyTheBoundsHold(): void {\n\
                     const full: f64 = total(100.0, \"br\");\n\
                     test.equal(full >= 0.0 && full < 100.0, true);\n\
                 }\n",
            ),
        ],
    );
    assert!(report.contains("2 passed"), "{}", report);
}
