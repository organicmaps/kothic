//! Pack several single-variant native drules files (e.g. light + dark of one
//! style family) that share the same structure into a single family file with a
//! per-variant color palette, plus a canonical, reviewable text dump.
//!
//! Usage: merge_variants <out_base> <variant_name> <variant.bin> [<variant_name> <variant.bin> ...]
//!   writes <out_base>.bin and <out_base>.txt.

use std::path::Path;

use kothic::drules::{self, Container};

fn list_repr(names: &[String]) -> String {
    let inner: Vec<String> = names.iter().map(|n| format!("'{}'", n)).collect();
    format!("[{}]", inner.join(", "))
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    if argv.len() < 4 || !argv.len().is_multiple_of(2) {
        eprintln!(
            "Pack several single-variant native drules files (e.g. light + dark of one style family) \
             that share the same structure into a single family file with a per-variant color palette, \
             plus a canonical, reviewable text dump.\n\n\
             Usage: merge_variants <out_base> <variant_name> <variant.bin> [<variant_name> <variant.bin> ...]\n  \
             writes <out_base>.bin and <out_base>.txt."
        );
        std::process::exit(1);
    }

    let out_base = &argv[1];
    let mut names: Vec<String> = Vec::new();
    let mut containers: Vec<Container> = Vec::new();
    let mut i = 2;
    while i < argv.len() {
        names.push(argv[i].clone());
        match drules::load_container(Path::new(&argv[i + 1])) {
            Ok(c) => containers.push(c),
            Err(e) => {
                eprintln!("ERROR: cannot load {}: {}", argv[i + 1], e);
                std::process::exit(1);
            }
        }
        i += 2;
    }

    match drules::save_binary(Path::new(&format!("{out_base}.bin")), &containers, &names).and_then(
        |()| drules::save_text(Path::new(&format!("{out_base}.txt")), &containers, &names),
    ) {
        Ok(()) => {}
        Err(e) => {
            eprintln!(
                "ERROR: cannot pack {} into {}: {}",
                list_repr(&names),
                out_base,
                e
            );
            std::process::exit(1);
        }
    }

    println!(
        "Packed variants {} into {}.bin/.txt",
        list_repr(&names),
        out_base
    );
}
