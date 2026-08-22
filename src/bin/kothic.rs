//! Command-line driver for the drules generation pipeline.
//!
//! Usage: kothic -s <stylesheet.mapcss> -o <outfile> -p <priorities dir>
//!               [-f <minzoom>] [-t <maxzoom>] [-x] [-d <data dir>]

use std::path::PathBuf;

use clap::Parser;

use kothic::pipeline::{Options, Pipeline, generate_drules};

#[derive(Parser)]
#[command(
    name = "kothic",
    about = "Generates native drules files from a MapCSS stylesheet."
)]
struct Cli {
    /// MapCSS stylesheet
    #[arg(short = 's', long = "stylesheet")]
    stylesheet: String,

    /// Base output path; writes <outfile>.bin (and <outfile>.txt with -x)
    #[arg(short = 'o', long = "output-file")]
    output_file: String,

    /// Directory with the priorities_*.prio.txt files
    #[arg(short = 'p', long = "priorities-path")]
    priorities_path: PathBuf,

    /// Minimum zoom level
    #[arg(short = 'f', long = "minzoom", default_value_t = 0)]
    minzoom: i32,

    /// Maximum zoom level
    #[arg(short = 't', long = "maxzoom", default_value_t = 20)]
    maxzoom: i32,

    /// Also write a human-readable .txt drules dump
    #[arg(short = 'x', long = "txt")]
    txt: bool,

    /// Path to mapcss-mapping.csv and other input files
    #[arg(short = 'd', long = "data-path")]
    data: Option<PathBuf>,
}

fn main() {
    let cli = Cli::parse();

    if cli.output_file == "-" {
        eprintln!("ERROR: Please specify base output path.");
        std::process::exit(2);
    }
    if !cli.priorities_path.is_dir() {
        eprintln!("ERROR: A path to priorities *.prio.txt files is required.");
        std::process::exit(2);
    }

    let options = Options {
        filename: Some(cli.stylesheet),
        minzoom: cli.minzoom,
        maxzoom: cli.maxzoom,
        outfile: cli.output_file,
        txt: cli.txt,
        priorities_path: cli.priorities_path.display().to_string(),
        data: cli.data.map(|d| d.display().to_string()),
    };

    let mut pipeline = Pipeline::new();
    if let Err(e) = generate_drules(&options, &mut pipeline) {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}
