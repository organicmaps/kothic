Kothic Mapcss parser/processor tailored for Organic Maps use.

Requires Python 3.11 or later. The compiler uses the standard library.

## Running unittests

To run all unittests execute next command from project root folder:

```bash
python3 -m unittest discover -s tests
```

this will search for all `test*.py` files within `tests` directory
and execute tests from those files.

## Running integration tests

File `integration-tests/full_drules_gen.py` is intended to generate drules
files for all 6 themes from main Organic Maps repo. It could be used to understand
which parts of the project are actually used by Organic Maps repo.

Usage:

```shell
cd integration-tests
python3 full_drules_gen.py -d ../../../data -o drules --txt
```

This command will run generation for styles - default light, default dark,
outdoors light, outdoors dark, vehicle light, vehicle dark and put `*.bin`
and `*.txt` files into 'drules' subfolder.

## Matching additional classifier types

When a selector uses a second classifier key, Kothic emits an `extra_tag`
condition for the drawing-rule reader. A feature must carry that classifier
type or one of its descendants for the condition to match. Use `!=` to test
for its absence. The reader must support `extra_tag`.

```mapcss
line|z15[highway=tertiary] { width: 1; color: #000000; }
line|z15[highway=tertiary][hwtag=oneway] { width: 3; }
```

Tertiary roads use width 1 by default. The second declaration overrides the
width for roads carrying `hwtag|oneway`. To specify a classifier path directly,
register `extra_tag` in `mapcss-dynamic.txt` and write conditions such as
`[extra_tag=highway=path=bicycle]`.
