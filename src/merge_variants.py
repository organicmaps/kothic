#!/usr/bin/env python3
"""Packs several single-variant native drules files (e.g. light + dark of one style family) that
share the same structure into a single family file with a per-variant color palette, plus a
canonical, reviewable text dump.

Usage: merge_variants.py <out_base> <variant_name> <variant.bin> [<variant_name> <variant.bin> ...]
  writes <out_base>.bin and <out_base>.txt.
"""

import sys

import drules


def main(argv):
    if len(argv) < 4 or len(argv) % 2 != 0:
        sys.exit(__doc__)

    out_base = argv[1]
    names = []
    containers = []
    for i in range(2, len(argv), 2):
        names.append(argv[i])
        containers.append(drules.load_container(argv[i + 1]))

    try:
        drules.save_binary(out_base + ".bin", containers, names)
        drules.save_text(out_base + ".txt", containers, names)
    except ValueError as e:
        sys.exit(f"ERROR: cannot pack {names} into {out_base}: {e}")

    print(f"Packed variants {names} into {out_base}.bin/.txt")


if __name__ == "__main__":
    main(sys.argv)
