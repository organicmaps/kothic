Kothic MapCSS parser/processor tailored for Organic Maps use.

This is a Rust rewrite of the kothic project. It parses MapCSS
stylesheets and produces the native drawing rules (drules) files used by the
Organic Maps renderer: `.bin`/`.txt` drules, priorities files, `types.txt`,
`classificator.txt`, `visibility.txt`, `colors.txt` and `patterns.txt`.
Output is byte-identical to the original Python implementation.

## Building

Requires Rust (edition 2024).

```bash
cargo build --release
```

Binaries:

* `kothic` - the drules generation pipeline (`src/bin/kothic.rs`)
* `gen_all` - generates drules for all 6 Organic Maps style variants
  (`src/bin/gen_all.rs`)
* `merge_variants` - packs single-variant drules into a family file with a
  per-variant color palette (`src/bin/merge_variants.rs`)
* `merge_styles` - merges several single-variant drules into one union file
  (`src/bin/merge_styles.rs`)

## Usage

Generate drules for a style:

```bash
kothic -s <stylesheet.mapcss> -o <outfile> -p <priorities dir> \
       [-f <minzoom>] [-t <maxzoom>] [-x] [-d <data dir>]
```

* `-s, --stylesheet` - MapCSS stylesheet (required)
* `-o, --output-file` - base output path; writes `<outfile>.bin` (and
  `<outfile>.txt` with `-x`) plus the auxiliary files into the data dir
* `-p, --priorities-path` - directory with the `priorities_*.prio.txt` files
  (required; the files are re-formatted in place)
* `-f, --minzoom` / `-t, --maxzoom` - zoom range (defaults 0 and 20)
* `-x, --txt` - also write a human-readable `.txt` drules dump
* `-d, --data-path` - path to `mapcss-mapping.csv`, `mapcss-dynamic.txt` and
  other input files (defaults to the `<outfile>` parent directory)

## Running tests

```bash
cargo test
```

The unit tests cover the MapCSS parser, rule matching, style choosers, eval,
colors, the drules binary/text serializers and the full generation pipeline.
The pipeline output is verified against the `tests/assets/` fixtures
(`case-2-generate-drules-mini` runs the whole pipeline and checks the
`types.txt` content, the drules `.bin` structure and the byte-identical
priorities re-formatting).

## Running integration tests

Binary `gen_all` generates drules files for all 6 themes from main Organic Maps
repo. It could be used to understand which parts of the project are actually
used by Organic Maps repo.

Usage:

```shell
gen_all -d ../../../data -o drules --txt
```

This command will run generation for styles - default light, default dark,
outdoors light, outdoors dark, vehicle light, vehicle dark and put `*.bin`
and `*.txt` files into 'drules' subfolder.
