#!/usr/bin/env python3
"""Merges several single-variant native drules files into one by taking the union of each type's
zoom range (a missing low/high zoom in one style is filled from another). Prints the differences
to stdout. The result is a tools-only file (e.g. for the mwm viewer) and is not bundled into the
apps.

Usage: merge_styles.py <in1.bin> <in2.bin> [<in3.bin> ...] <out.bin> [<out.txt>]
"""

import collections
import copy
import sys

import drules


def read_zoom_extremes(cont):
    """For each type returns [lowest-zoom element, highest-zoom element, all elements]."""
    result = {}
    for rule in cont.cont:
        zooms = [None, None, list(rule.element)]
        for elem in rule.element:
            if zooms[1] is None or elem.scale > zooms[1].scale:
                zooms[1] = elem
            if zooms[0] is None or elem.scale < zooms[0].scale:
                zooms[0] = elem
        if zooms[0] is not None:
            name = rule.name
            if name in result:
                if result[name][0].scale < zooms[0].scale:
                    zooms[0] = result[name][0]
                if result[name][1].scale > zooms[1].scale:
                    zooms[1] = result[name][1]
                zooms[2] = result[name][2] + zooms[2]
            result[name] = zooms
    return result


def zooms_string(z1, z2):
    if z2 != z1:
        return "zooms {}-{}".format(min(z1, z2), max(z1, z2))
    return "zoom {}".format(z1)


def add_missing_zooms(dest, typ, source, target, high):
    """Appends as many copies of source (re-scaled) to dest[typ] as needed to extend target's zoom
    range to cover source's."""
    if high:
        scales = (target.scale + 1, source.scale + 1)
    else:
        scales = (source.scale, target.scale)

    if scales[1] < scales[0]:
        print("{}: missing {} {}".format(typ, "high" if high else "low", zooms_string(scales[1], scales[0] - 1)))
        for z in range(scales[1], scales[0]):
            fix = copy.deepcopy(source)
            fix.scale = z
            dest[typ].append(fix)
    elif scales[1] > scales[0]:
        print("{}: extra {} {}".format(typ, "high" if high else "low", zooms_string(scales[0], scales[1] - 1)))


def create_diff(zooms1, zooms2):
    add_elements_low = collections.defaultdict(list)
    add_elements_high = collections.defaultdict(list)
    seen = set(zooms2.keys())
    for typ in zooms1:
        if typ in zooms2:
            seen.remove(typ)
            add_missing_zooms(add_elements_low, typ, zooms1[typ][0], zooms2[typ][0], False)
            add_missing_zooms(add_elements_high, typ, zooms1[typ][1], zooms2[typ][1], True)
        else:
            print("{}: not found in the alternative style; {}".format(
                typ, zooms_string(zooms1[typ][0].scale, zooms1[typ][1].scale)))

    add_types = []
    for typ in sorted(seen):
        print("{}: missing completely; {}".format(typ, zooms_string(zooms2[typ][0].scale, zooms2[typ][1].scale)))
        cont = drules.ClassifElement()
        cont.name = typ
        cont.element.extend(copy.deepcopy(zooms2[typ][2]))
        add_types.append(cont)

    return (add_elements_low, add_elements_high, add_types)


def apply_diff(cont, diff):
    d2pos = 0
    result = drules.Container()
    result.colors.value.extend(copy.deepcopy(cont.colors.value))
    for rule in cont.cont:
        typ = rule.name
        # Append diff types whose name sorts before typ.
        while d2pos < len(diff[2]) and diff[2][d2pos].name < typ:
            result.cont.extend([diff[2][d2pos]])
            d2pos += 1
        fix = drules.ClassifElement()
        fix.name = typ
        if typ in diff[0]:
            fix.element.extend(diff[0][typ])
        if rule.element:
            fix.element.extend(rule.element)
        if typ in diff[1]:
            fix.element.extend(diff[1][typ])
        result.cont.extend([fix])
    result.cont.extend(diff[2][d2pos:])
    return result


def main(argv):
    if len(argv) <= 3:
        sys.exit(__doc__)

    out_txt = None
    if argv[-1].endswith("txt"):
        out_txt = argv[-1]
        argv = argv[:-1]
    out_bin = argv[-1]
    inputs = argv[1:-1]

    print(f"Merging {inputs} into {out_bin}({out_txt if out_txt else 'no text output'})")

    merged = drules.load_container(inputs[0])
    for path in inputs[1:]:
        cur = drules.load_container(path)
        diff = create_diff(read_zoom_extremes(merged), read_zoom_extremes(cur))
        merged = apply_diff(merged, diff)

    drules.save_binary(out_bin, [merged], ["merged"])
    if out_txt:
        drules.save_text(out_txt, [merged], ["merged"])


if __name__ == "__main__":
    main(sys.argv)
