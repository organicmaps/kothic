This file provides guidance to AI agents when working with code in this repository.

Kothic is a MapCSS parser/processor tailored for Organic Maps. It compiles MapCSS stylesheets into the binary `drules_proto*.bin` (+ optional `.txt`) drawing-rule files that the renderer consumes, plus auxiliary metadata files (`classificator.txt`, `visibility.txt`, `colors.txt`, `patterns.txt`, `types.txt`).

The parent project's `CLAUDE.md` (one level up at `omim/CLAUDE.md`) covers overall Organic Maps conventions; this file only documents kothic-specific details.

## Commands

```bash
# Install deps (Python >= 3.8, protobuf ~3.20 — matches Ubuntu's python3-protobuf)
pip3 install -r requirements.txt

# Unit tests — discover all test*.py under tests/ (must run from project root)
python3 -m unittest discover -s tests

# Single test module / method
python3 -m unittest tests.testLibkomwm
python3 -m unittest tests.testLibkomwm.LibKomwmTest.test_generate_drules_mini

# Lint (matches CI exactly — see .github/workflows/unit-tests.yml)
ruff check --exclude=src/drules_struct_pb2.py --target-version=py39

# Integration test — regenerates drules for all 6 themes from the real data/
cd integration-tests && python3 full_drules_gen.py -d ../../../data -o drules --txt

# Run the generator directly for one style
python3 src/libkomwm.py -s <style.mapcss> -o <output_prefix> -p <priorities_dir> [--txt]

# Full production regeneration (all 8 theme variants + merge) — run from anywhere
../unix/generate_drules.sh
```

## Architecture

**Entry point:** `src/libkomwm.py` — orchestrates the whole pipeline. `komap_mapswithme(options)` is the function callers (tests, `full_drules_gen.py`, `generate_drules.sh`) hook into. The bottom `main()` is the CLI wrapper.

**Parser:** `src/mapcss/` — the MapCSS engine.
- `__init__.py` defines the `MapCSS` class and all the regex-based tokenization (zoom, conditions, declarations, `@import`, variables).
- `StyleChooser.py` — a single CSS-like rule block (selector + declaration). `MapCSS.parse()` produces a list of these.
- `Rule.py` — one selector within a chooser (subject + zoom range + conditions). `type_matches` maps MapCSS subjects (`area`/`line`/`way`/`node`) to compatible geometry types.
- `Condition.py` — a single `[tag=value]` predicate (eq/ne/lt/gt/regex/set/unset).
- `Eval.py` — `eval('...')` MapCSS expressions, compiled to Python via `compile()`.
- `webcolors/` — vendored color parsing (`whatever_to_hex`, `whatever_to_cairo`).

**Protobuf output:** `src/drules_struct_pb2.py` is **auto-generated from `data/drules_struct.proto`** in the main repo — do not hand-edit. It defines `ContainerProto`, `ClassifElementProto`, `DrawElementProto`, etc. that `libkomwm` serializes.

**Priority ranges** (see the long comment block near the top of `libkomwm.py`): drawing rules live in four ordered ranges — `overlays` (icons/captions, ±10000), `FG` (foreground areas/lines, 0–1000), `BG-top` (water, -1000–0), `BG-by-size` (landcover, -2000–-1000). The companion `priorities_*.prio.txt` files in `data/styles/<style>/include/` are **re-formatted and re-sorted in place** every time the generator runs; the renderer's layering logic in `drape_frontend/stylist.cpp` mirrors these ranges.

**Test assets:** `tests/assets/` holds three case directories. `case-2-generate-drules-mini` is a stripped-down style (zooms 0–10, highway-only) whose generated output is checked for exact line/style counts — if you change the generator output format, those expected counts in `testLibkomwm.py` will need updating.

## Conventions specific to kothic

- This is a Python project inside a mostly-C++ repo; the parent `CLAUDE.md`'s C++ rules don't apply here. Follow PEP 8 / ruff defaults, target Python 3.9 (per CI).
- `src/drules_struct_pb2.py` is auto-generated — never modify directly; regenerate from `data/drules_struct.proto`.
- `libkomwm` carries module-level mutable state (`prio_ranges`, `visibilities`, `MULTIPROCESSING`). Tests and `full_drules_gen.py` `deepcopy` and restore it between runs — preserve that pattern if you add new global state.
- Integration tests are not run by CI (`unit-tests.yml` only runs `unittest discover -s tests`). The integration script exists to verify the generator against the real `data/` styles.
