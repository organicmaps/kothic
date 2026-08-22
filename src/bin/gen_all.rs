//! Generates native drules for all 6 Organic Maps style variants.
//!
//! Usage: gen_all -d <data path> [-o <outdir>] [-f <minzoom>] [-t <maxzoom>] [-x]

use std::path::Path;

use clap::Parser;

use kothic::pipeline::{Options, Pipeline, generate_drules};

const STYLES: &[(&str, &str, &str)] = &[
    (
        "default_light",
        "styles/default/light/style.mapcss",
        "styles/default/include",
    ),
    (
        "default_dark",
        "styles/default/dark/style.mapcss",
        "styles/default/include",
    ),
    (
        "outdoors_light",
        "styles/outdoors/light/style.mapcss",
        "styles/outdoors/include",
    ),
    (
        "outdoors_dark",
        "styles/outdoors/dark/style.mapcss",
        "styles/outdoors/include",
    ),
    (
        "vehicle_light",
        "styles/vehicle/light/style.mapcss",
        "styles/vehicle/include",
    ),
    (
        "vehicle_dark",
        "styles/vehicle/dark/style.mapcss",
        "styles/vehicle/include",
    ),
];

#[derive(Parser)]
#[command(
    name = "gen_all",
    about = "Generates native drules for all 6 Organic Maps style variants."
)]
struct Cli {
    /// Base 'data' path
    #[arg(short = 'd', long = "data-path")]
    data: String,

    /// Output directory
    #[arg(short = 'o', long = "output-dir", default_value = "drules")]
    output_dir: String,

    /// Minimum zoom level
    #[arg(short = 'f', long = "minzoom", default_value_t = 0)]
    minzoom: i32,

    /// Maximum zoom level
    #[arg(short = 't', long = "maxzoom", default_value_t = 20)]
    maxzoom: i32,

    /// Also write a human-readable .txt drules dump
    #[arg(short = 'x', long = "txt")]
    txt: bool,
}

fn main() {
    let cli = Cli::parse();

    let data = cli.data.clone();
    if !Path::new(&data).is_dir() {
        eprintln!("ERROR: Please specify base 'data' path.");
        std::process::exit(2);
    }

    println!("Start generating styles");
    for (name, style_path, include_path) in STYLES {
        println!("Generating {} style ...", name);
        let options = Options {
            filename: Some(format!("{}/{}", data, style_path)),
            minzoom: cli.minzoom,
            maxzoom: cli.maxzoom,
            outfile: format!("{}/{}", cli.output_dir, name),
            txt: cli.txt,
            priorities_path: format!("{}/{}", data, include_path),
            data: Some(data.clone()),
        };
        let mut pipeline = Pipeline::new();
        if let Err(e) = generate_drules(&options, &mut pipeline) {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
    println!("Done!");
}
