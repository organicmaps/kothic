This file provides guidance to LLM/AI agents when working with code in this repository.

## Overview

Kothic is the MapCSS → drawing-rules compiler for Organic Maps, checked out as a git submodule at
`tools/kothic` of the main organicmaps repo. It parses MapCSS stylesheets and produces the app's
native binary drawing-rules files (`drules_*.bin`) plus the derived `types.txt`,
`classificator.txt`, `visibility.txt`, `colors.txt` and `patterns.txt`.

Pure Python, standard library only — `requirements.txt` is intentionally empty, do not add external
dependencies. Keep the code compatible with Python 3.9 (CI's pinned version; README says 3.8+).

The style-editing workflow that drives this tool is documented in the parent repo
(`data/CLAUDE.md`, `docs/STYLES.md` there).

## Commands

```bash
# All unit tests (run from the repo root; plain unittest, no pytest)
python3 -m unittest discover -s tests

# One file / one test
python3 -m unittest tests.testDrules
python3 -m unittest tests.testDrules.DrulesTest.test_parse_binary_skips_section_padding

# Lint (default ruff rules; gates CI together with the unit tests)
ruff check --target-version=py39
```

Integration run — regenerates all themes from the parent Organic Maps checkout (assumes this repo
sits at `tools/kothic` inside it) and writes `.bin`/`.txt` files into `drules/`:

```bash
cd integration-tests
python3 full_drules_gen.py -d ../../../data -o drules --txt
```

The production pipeline is driven from the parent repo by `tools/unix/generate_drules.sh`:
`libkomwm.py` once per style/variant → `merge_variants.py` packs light+dark into one family file
per style → `merge_styles.py` builds a tools-only merged file (`drules_merged.bin`, not shipped).

## Architecture

Data flow: MapCSS text → `src/mapcss/` (generic parser) → `src/libkomwm.py` (Organic
Maps-specific compiler) → `src/drules.py` (native format) → merge tools.

### src/mapcss/ — MapCSS parser

- `MapCSS.parse()` in `__init__.py` is a regex-based parser handling selectors, zoom ranges
  (`|z10-12`), `[key=value]` conditions, `@import`, `@variable` substitution and `eval('…')`
  expressions. `build_choosers_tree()`/`finalize_choosers_tree()` then index the parsed
  `StyleChooser`s by (object type, zoom, class name) with per-zoom pre-filtered rule chains — this
  optimization tree is what makes full generation fast.
- Static vs dynamic tags: conditions on tags known from `mapcss-mapping.csv` are resolved at
  compile time; conditions on tags listed in `mapcss-dynamic.txt` become *runtime conditions*,
  emitted as `apply_if` strings in the drules and evaluated by the app at runtime.
- `StyleChooser` = one selector+declaration block; `Rule` = one selector in a chain (subject, zoom,
  conditions, `::object-id` subparts); `Condition` = one tag test; `Eval` compiles MapCSS `eval()`
  to Python. `webcolors/` is a vendored color-name library.

### src/libkomwm.py — the compiler (main entry point)

- Inputs: the stylesheet (`-s`), `mapcss-mapping.csv` (OSM tags → classificator types like
  `highway-primary`) and `mapcss-dynamic.txt` from the data dir (`-d`), `priorities_*.prio.txt`
  from `-p`. For every classificator type × zoom it queries the style and builds `DrawElement`s
  (lines, areas, icons, captions, path texts, shields).
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
  a caption with an icon but no `text-offset`, etc. increment `validation_errors_count`; the run
  then exits non-zero without writing drules.
- Global state: the parsed `style`, `prio_ranges` and `visibilities` are module globals; workers
  share them via the `fork` start method when `MULTIPROCESSING` is on. Any caller running
  generation more than once per process must reset them and set
  `libkomwm.MULTIPROCESSING = False` (see `integration-tests/full_drules_gen.py` and
  `tests/testLibkomwm.py`).

### src/drules.py — native drawing-rules format

- Replaced protobuf. Builder classes (`Container`, `ClassifElement`, `DrawElement`, `LineRule`, …)
  mirror the old protobuf API: attribute assignment, eagerly-created sub-messages,
  `.extend()`/`.append()` on repeated fields. Presence is tracked on scalar *assignment* only, so
  the `if dr_element.symbol.priority:` read idiom in libkomwm never marks a child as set.
- **The wire format must stay in sync with `libs/indexer/drules_format.{hpp,cpp}` in the main
  repo** (magic `OMDR`, versioned, sections with skippable padding).
- Color model: builders hold actual ARGB values; the palette+index encoding is internal to the
  binary writer/reader. A *family* file packs several variants (e.g. light/dark) that must be
  structurally identical except for the four color fields (`color`, `stroke_color`, `text_color`,
  `text_stroke_color`) — `serialize_binary` raises `ValueError` otherwise.
- `serialize_text` produces the canonical, deterministic text dump (all variants side by side)
  used to review style changes in the main repo. Colors are written by value, so adding or removing
  a color doesn't renumber the whole dump.

### Merge tools

- `src/merge_variants.py` — packs single-variant `.bin`s of one style family into a family file.
- `src/merge_styles.py` — takes the union of each type's zoom range across styles, filling missing
  low/high zooms from the other style; output is tools-only (mwm viewer).

### Tests

Each `tests/test*.py` inserts `src/` into `sys.path` itself, so tests run from the repo root
without any packaging. Fixtures live in `tests/assets/case-*`; `testLibkomwm.py` runs a full
mini-generation over a trimmed style (`case-2-generate-drules-mini`) and asserts on the produced
files.
