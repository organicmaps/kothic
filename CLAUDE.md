This file provides guidance to LLM/AI agents when working with code in this repository.

## Overview

Kothic is the MapCSS → drawing-rules compiler for Organic Maps, checked out as a git submodule at
`tools/kothic` of the main organicmaps repo. It parses MapCSS stylesheets and produces the app's
native binary drawing-rules files (`drules_*.bin`) plus the derived `types.txt`,
`classificator.txt`, `visibility.txt`, `colors.txt` and `patterns.txt`.

This is a **Rust rewrite** of the original pure-Python toolchain (edition 2024, stable Rust).
Output must stay **byte-identical** to what the Python implementation produced — this constraint
shapes a lot of the design (see `src/compat.rs`). Dependencies are kept minimal (`regex`, `md-5`,
`clap`, `indexmap`, `paste`); don't add new ones without a good reason.

The style-editing workflow that drives this tool is documented in the parent repo
(`data/CLAUDE.md`, `docs/STYLES.md` there).

## Commands

```bash
# Build (debug is enough for tests; release enables LTO)
cargo build --release

# All unit tests (CI runs build + test)
cargo test

# One binary/module's tests / one test by name
cargo test drules
cargo test generate_drules_mini

# Lint
cargo clippy
```

Integration run — regenerates all 6 themes from the parent Organic Maps checkout (assumes this
repo sits at `tools/kothic` inside it) and writes `.bin`/`.txt` files into `drules/`:

```bash
cargo run --bin gen_all -d ../../../data -o drules --txt
```

The production pipeline is driven from the parent repo by `tools/unix/generate_drules.sh`:
`kothic` once per style/variant → `merge_variants` packs light+dark into one family file per
style → `merge_styles` builds a tools-only merged file (`drules_merged.bin`, not shipped).

## Architecture

Data flow: MapCSS text → `mapcss.rs` (generic parser) → `pipeline.rs` (Organic Maps-specific
compiler) → `drules.rs` (native format) → merge tools.

### src/mapcss.rs (+ style_chooser.rs, rule.rs, condition.rs, eval.rs) — MapCSS parser

- `MapCSS::parse()` is a tokenizer+regex parser handling selectors, zoom ranges (`|z10-12`),
  `[key=value]` conditions, `@import`, `@variable` substitution and `eval('…')` expressions.
  `build_choosers_tree()`/`finalize_choosers_tree()` then index the parsed `StyleChooser`s by
  (object type, zoom, class name) with per-zoom pre-filtered rule chains — this optimization tree
  is what makes full generation fast.
- Static vs dynamic tags: conditions on tags known from `mapcss-mapping.csv` are resolved at
  compile time; conditions on tags listed in `mapcss-dynamic.txt` become *runtime conditions*,
  emitted as `apply_if` strings in the drules and evaluated by the app at runtime.
- `StyleChooser` = one selector+declaration block (`style_chooser.rs`; holds `rule_chains` and the
  resulting style dicts of `StyleValue`s); `Rule` = one selector in a chain (subject, zoom,
  conditions, `::object-id` subparts) (`rule.rs`); `Condition` = one tag test (`condition.rs`);
  `Eval` (`eval.rs`) parses MapCSS `eval()` with a small recursive-descent parser replacing the
  Python implementation's restricted-`eval` approach. Color names live in `color.rs` (the old
  vendored webcolors table is built in there).

### src/pipeline.rs — the compiler (main entry point)

- Inputs (CLI in `src/bin/kothic.rs`, clap-based): the stylesheet (`-s`), `mapcss-mapping.csv`
  (OSM tags → classificator types like `highway-primary`) and `mapcss-dynamic.txt` from the data
  dir (`-d`), `priorities_*.prio.txt` from `-p`. For every classificator type × zoom it queries
  the style and builds `DrawElement`s (lines, areas, icons, captions, path texts, shields).
- Side effects beyond the `.bin`/`.txt` output: writes `types.txt`, `classificator.txt`,
  `visibility.txt` into the data dir; `colors.txt` and `patterns.txt` there are read first and
  rewritten, **accumulating** values across invocations; the priority files are re-formatted and
  re-sorted **in place**. This is why `generate_drules.sh` stages everything in a temp dir.
- Priorities: four ranges (overlays / FG / BG-top / BG-by-size) documented at the top of the file.
  `LAYER_PRIORITY_RANGE` and `OVERLAYS_MAX_PRIORITY` must match `drule::kLayerPriorityRange` /
  `drule::kOverlaysMaxPriority` in the C++ core; the layering logic lives in
  `drape_frontend/stylist.cpp`. Casing lines and captions-with-icons get automatic priorities
  derived from their main drule (optional captions are pushed below all other overlays).
- Validation: a missing priority, pathtext/shield with no line at that zoom, missing `text-color`,
  a caption with an icon but no `text-offset`, etc. increment `Pipeline::validation_errors_count`;
  the run then exits non-zero without writing drules.
- State: unlike the Python version's module globals, all mutable state lives in the `Pipeline`
  struct passed through the generation functions — each run constructs a fresh `Pipeline::new()`.

### src/drules.rs — native drawing-rules format

- Ported from the protobuf-era Python builders: builder types (`Container`, `ClassifElement`,
  `DrawElement`, `LineRule`, …) keep the same shape (eagerly-created sub-rules, repeated-field
  `.extend()`/`.push()`, canonical string form used for de-duplication).
- **The wire format must stay in sync with `libs/indexer/drules_format.{hpp,cpp}` in the main
  repo** (magic `OMDR`, versioned, sections with skippable padding).
- Color model: builders hold actual ARGB values; the palette+index encoding is internal to the
  binary writer/reader. A *family* file packs several variants (e.g. light/dark) that must be
  structurally identical except for the four color fields (`color`, `stroke_color`, `text_color`,
  `text_stroke_color`) — `save_binary` returns an error otherwise.
- `save_text` produces the canonical, deterministic text dump (all variants side by side) used to
  review style changes in the main repo.

### Merge tools

- `src/bin/merge_variants.rs` — packs single-variant `.bin`s of one style family into a family
  file (format logic lives in `drules.rs`).
- `src/merge_styles.rs` + `src/bin/merge_styles.rs` — take the union of each type's zoom range
  across styles, filling missing low/high zooms from the other style; output is tools-only
  (mwm viewer).

### src/compat.rs — Python-compatible formatting

Byte-identical output requires CPython semantics for float `repr()`/`str()`, `"{:.Pg}".format()`,
banker's rounding and single-quote string reprs — reimplemented here because Rust's `Display`/
`{:.PrecG}` differ. If generated text differs by formatting only, look here first.

### Tests

Rust unit tests live inline in each module under `#[cfg(test)]` (`tests/` only holds fixtures);
run them all with plain `cargo test`. Fixtures live in `tests/assets/case-*`;
`pipeline.rs` runs a full mini-generation over a trimmed style
(`case-2-generate-drules-mini`) and asserts on the produced files.
