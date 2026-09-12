mod ast;
mod bytecode;
mod compiler;
mod error;
mod lexer;
mod obj;
mod parser;
mod types;
mod vm;

use std::path::{Path, PathBuf};
use std::process::exit;

const USAGE: &str = "\
binZ 0.1.0 -- a small strongly typed compiled language

usage:
  binz build <file.binz> [-o <out.binzc>]   compile to a bytecode artifact
  binz run   <file.binz>                    compile and run
  binz exec  <file.binzc>                   run an artifact
  binz dump  <file.binzc>                   disassemble an artifact
";

fn compile_path(path: &str) -> bytecode::Module {
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read {}: {}", path, e);
            exit(2);
        }
    };
    let result = lexer::Lexer::new(&src)
        .tokenize()
        .and_then(|toks| parser::Parser::new(toks).parse_program())
        .and_then(|items| compiler::compile(&items));
    match result {
        Ok(m) => m,
        Err(e) => {
            eprint!("{}", e.report(path, &src));
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
