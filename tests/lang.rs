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
