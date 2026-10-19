#![expect(clippy::print_stdout)]
//! perf/bun-gap: compress every file listed in `<list>` (one path per line) and print
//! `<hash> <bytes> <iterations> <path>` per file, so two runs (e.g. with and without
//! `OXC_BG_SKIP_CLEAN=1`) can be compared. Files that fail to parse print `SKIP`.
//! With `<out_dir>`, also writes each output to `<out_dir>/<index>.js`.
//!
//! `cargo run -p oxc_minifier --profile coverage --example bg_corpus -- <list> [out_dir]`

use std::{
    hash::{Hash, Hasher},
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
};

use oxc_allocator::Allocator;
use oxc_codegen::{Codegen, CodegenOptions};
use oxc_minifier::{CompressOptions, Minifier, MinifierOptions};
use oxc_parser::Parser;
use oxc_span::SourceType;
use rustc_hash::FxHasher;

fn main() {
    let mut args = std::env::args().skip(1);
    let list = args.next().expect("list file");
    let out_dir = args.next();
    for (index, line) in std::fs::read_to_string(list).unwrap().lines().enumerate() {
        let path = Path::new(line);
        let Ok(source_text) = std::fs::read_to_string(path) else { continue };
        let source_type = SourceType::from_path(path).unwrap_or_default();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let allocator = Allocator::default();
            let ret = Parser::new(&allocator, &source_text, source_type).parse();
            if !ret.diagnostics.is_empty() {
                return None;
            }
            let mut program = ret.program;
            let ret = Minifier::new(MinifierOptions {
                mangle: None,
                mangle_properties: None,
                compress: Some(if std::env::var_os("BG_CORPUS_SMALLEST").is_some() {
                    CompressOptions::smallest()
                } else {
                    CompressOptions::default()
                }),
            })
            .minify(&allocator, &mut program);
            let code = Codegen::new()
                .with_options(if std::env::var_os("BG_CORPUS_PRETTY").is_some() {
                    CodegenOptions::default()
                } else {
                    CodegenOptions::minify()
                })
                .with_scoping(ret.scoping)
                .build(&program)
                .code;
            Some((code, ret.iterations))
        }));
        match result {
            Ok(Some((code, iterations))) => {
                let mut h = FxHasher::default();
                code.hash(&mut h);
                println!("{:016x} {} {} {}", h.finish(), code.len(), iterations, line);
                if let Some(dir) = &out_dir {
                    std::fs::write(Path::new(dir).join(format!("{index}.js")), code).unwrap();
                }
            }
            Ok(None) => println!("SKIP parse {line}"),
            Err(_) => println!("SKIP panic {line}"),
        }
    }
}
