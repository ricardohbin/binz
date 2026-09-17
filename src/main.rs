mod ast;
mod bytecode;
mod compiler;
mod error;
mod lexer;
mod loader;
mod obj;
mod parser;
mod stdlib;
mod types;
mod vm;

use std::path::{Path, PathBuf};
use std::process::exit;

const USAGE: &str = "\
binZ 0.1.0 -- a small strongly typed compiled language

usage:
  binz build <file.binz> [-o <out.binzc>]   compile to a bytecode artifact
  binz run   <file.binz>                    compile and run
  binz test  <file.binz>                    run every `@test` it can reach
  binz exec  <file.binzc>                   run an artifact
  binz dump  <file.binzc>                   disassemble an artifact
";

/// Loads the file and everything it imports, then compiles the whole graph
/// as one program. A diagnostic names the file its span belongs to, which is
/// not always the one on the command line.
fn compile_path(path: &str) -> bytecode::Module {
    compile_mode(path, compiler::Mode::Run)
}

fn compile_mode(path: &str, mode: compiler::Mode) -> bytecode::Module {
    // The file on the command line is the only one the shell is responsible
    // for, so a missing one is a usage error rather than a compile error.
    if let Err(e) = std::fs::metadata(path) {
        eprintln!("error: cannot read {}: {}", path, e);
        exit(2);
    }
    let prog = match loader::load(path) {
        Ok(p) => p,
        Err(e) => {
            let where_ = e.file.clone().unwrap_or_else(|| path.to_string());
            let src = std::fs::read_to_string(&where_).unwrap_or_default();
            eprint!("{}", e.report(&where_, &src));
            exit(1);
        }
    };
    let result = match mode {
        compiler::Mode::Run => compiler::compile(&prog),
        compiler::Mode::Test => compiler::compile_tests(&prog),
    };
    match result {
        Ok(m) => m,
        Err(e) => {
            let where_ = e.file.clone().unwrap_or_else(|| path.to_string());
            let src = prog.source_of(&where_).map(|f| f.src.clone()).unwrap_or_default();
            eprint!("{}", e.report(&where_, &src));
            exit(1);
        }
    }
}

fn load_artifact(path: &str) -> bytecode::Module {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: cannot read {}: {}", path, e);
            exit(2);
        }
    };
    match bytecode::deserialize(&bytes) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: {}", e);
            exit(2);
        }
    }
}

fn run(m: &bytecode::Module) -> ! {
    match vm::Vm::run(m) {
        Ok(code) => exit(code),
        Err(e) => {
            eprintln!("{}", e);
            exit(101);
        }
    }
}

/// Runs every `@test` of the graph, each in a virtual machine of its own, and
/// reports them file by file. A test says what it has to say by failing, so
/// the whole report is its name and, when it fails, the line that stopped it.
fn run_tests(m: &bytecode::Module) -> i32 {
    if m.tests.is_empty() {
        println!("no `@test` functions here");
        return 0;
    }
    println!("running {} test(s)\n", m.tests.len());
    let mut failed = 0;
    let mut file = "";
    for t in &m.tests {
        if t.file != file {
            file = &t.file;
            println!("{}", file);
        }
        match vm::Vm::run_test(m, t.func) {
            Ok(()) => println!("  ok    {}", t.name),
            Err(e) => {
                failed += 1;
                println!("  FAIL  {}", t.name);
                // The function named by the error is the one that stopped,
                // which is only the test itself when the test asserted.
                if e.func == t.name {
                    println!("        {}", e.msg);
                } else {
                    println!("        {} (in `{}`)", e.msg, e.func);
                }
            }
        }
    }
    println!("\n{} passed, {} failed", m.tests.len() - failed, failed);
    if failed > 0 {
        1
    } else {
        0
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprint!("{}", USAGE);
        exit(2);
    }
    match args[1].as_str() {
        "build" => {
            let m = compile_path(&args[2]);
            let out: PathBuf = if let Some(i) = args.iter().position(|a| a == "-o") {
                match args.get(i + 1) {
                    Some(p) => PathBuf::from(p),
                    None => {
                        eprintln!("error: -o needs a path");
                        exit(2);
                    }
                }
            } else {
                Path::new(&args[2]).with_extension("binzc")
            };
            let bytes = bytecode::serialize(&m);
            if let Err(e) = std::fs::write(&out, &bytes) {
                eprintln!("error: cannot write {}: {}", out.display(), e);
                exit(2);
            }
            println!(
                "wrote {} ({} bytes, {} functions)",
                out.display(),
                bytes.len(),
                m.funcs.len()
            );
        }
        "run" => {
            let m = compile_path(&args[2]);
            run(&m);
        }
        "test" => {
            let m = compile_mode(&args[2], compiler::Mode::Test);
            exit(run_tests(&m));
        }
        "exec" => {
            let m = load_artifact(&args[2]);
            run(&m);
        }
        "dump" => {
            let m = load_artifact(&args[2]);
            print!("{}", bytecode::disassemble(&m));
        }
        other => {
            eprintln!("error: unknown command `{}`\n", other);
            eprint!("{}", USAGE);
            exit(2);
        }
    }
}
