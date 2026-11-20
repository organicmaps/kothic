#!/usr/bin/env python3
"""Extend each type's zoom range to cover all input styles.

Retain the earliest style that defines the type and copy all rules at its boundary zooms
outward as needed. The result is for tools such as the MWM viewer.

Usage: merge_styles.py <in1.bin> <in2.bin> [<in3.bin> ...] <out.bin> [<out.txt>]
"""

import collections
import copy
import sys

import drules


def read_zoom_extremes(cont):
    elements_by_type = collections.defaultdict(list)
    for rule in cont.cont:
        elements_by_type[rule.name].extend(rule.element)
    return {typ: (min(e.scale for e in elements), max(e.scale for e in elements), elements)
            for typ, elements in elements_by_type.items() if elements}


def zooms_string(z1, z2):
    if z2 != z1:
        return "zooms {}-{}".format(min(z1, z2), max(z1, z2))
    return "zoom {}".format(z1)


def add_missing_zooms(dest, typ, elements, source, target, high):
    start, end = (source + 1, target + 1) if high else (target, source)
    side = "high" if high else "low"
    if start < end:
        print(f"{typ}: missing {side} {zooms_string(start, end - 1)}")
        boundary = [element for element in elements if element.scale == source]
        for z in range(start, end):
            for element in boundary:
                fix = copy.deepcopy(element)
                fix.scale = z
                dest[typ].append(fix)
    elif end < start:
        print(f"{typ}: extra {side} {zooms_string(end, start - 1)}")


def create_diff(zooms1, zooms2):
    add_elements_low = collections.defaultdict(list)
    add_elements_high = collections.defaultdict(list)
    seen = set(zooms2.keys())
    for typ in zooms1:
        low, high, elements = zooms1[typ]
        if typ in zooms2:
            seen.remove(typ)
            add_missing_zooms(add_elements_low, typ, elements, low, zooms2[typ][0], False)
            add_missing_zooms(add_elements_high, typ, elements, high, zooms2[typ][1], True)
        else:
            print(f"{typ}: not found in the alternative style; {zooms_string(low, high)}")

    add_types = []
    for typ in sorted(seen):
        low, high, elements = zooms2[typ]
        print(f"{typ}: missing completely; {zooms_string(low, high)}")
        cont = drules.ClassifElement()
        cont.name = typ
        cont.element.extend(copy.deepcopy(elements))
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
