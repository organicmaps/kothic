//! Merge several single-variant native drules files into one by taking the
//! union of each type's zoom range (a missing low/high zoom in one style is
//! filled from another). Prints the differences to stdout. The result is a
//! tools-only file (e.g. for the mwm viewer) and is not bundled into the apps.
//!
//! Usage: merge_styles <in1.bin> <in2.bin> [<in3.bin> ...] <out.bin> [<out.txt>]

use std::path::Path;

use kothic::drules::{self, Container};
use kothic::merge_styles::{apply_diff, create_diff, read_zoom_extremes};

fn list_repr(names: &[String]) -> String {
    let inner: Vec<String> = names.iter().map(|n| format!("'{}'", n)).collect();
    format!("[{}]", inner.join(", "))
}

fn main() {
    let mut argv: Vec<String> = std::env::args().collect();
    if argv.len() <= 3 {
        eprintln!(
            "Merge several single-variant native drules files into one by taking the union of each \
             type's zoom range (a missing low/high zoom in one style is filled from another). Prints \
             the differences to stdout. The result is a tools-only file (e.g. for the mwm viewer) and \
             is not bundled into the apps.\n\n\
             Usage: merge_styles <in1.bin> <in2.bin> [<in3.bin> ...] <out.bin> [<out.txt>]"
        );
        std::process::exit(1);
    }

    let mut out_txt: Option<String> = None;
    if argv[argv.len() - 1].ends_with("txt") {
        out_txt = Some(argv.pop().unwrap());
    }
    let out_bin = argv.pop().unwrap();
    let inputs: Vec<String> = argv[1..].to_vec();

    println!(
        "Merging {} into {}({})",
        list_repr(&inputs),
        out_bin,
        out_txt.as_deref().unwrap_or("no text output")
    );

    let mut merged: Container = drules::load_container(Path::new(&inputs[0])).unwrap_or_else(|e| {
        eprintln!("ERROR: cannot load {}: {}", inputs[0], e);
        std::process::exit(1);
    });
    for path in &inputs[1..] {
        let cur = drules::load_container(Path::new(path)).unwrap_or_else(|e| {
            eprintln!("ERROR: cannot load {}: {}", path, e);
            std::process::exit(1);
        });
        let diff = create_diff(&read_zoom_extremes(&merged), &read_zoom_extremes(&cur));
        merged = apply_diff(&merged, &diff);
    }

    drules::save_binary(
        Path::new(&out_bin),
        &[merged.clone()],
        &[String::from("merged")],
    )
    .unwrap_or_else(|e| {
        eprintln!("ERROR: cannot write {}: {}", out_bin, e);
        std::process::exit(1);
    });
    if let Some(out_txt) = out_txt {
        drules::save_text(Path::new(&out_txt), &[merged], &[String::from("merged")])
            .unwrap_or_else(|e| {
                eprintln!("ERROR: cannot write {}: {}", out_txt, e);
                std::process::exit(1);
            });
    }
}
