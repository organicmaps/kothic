import sys
import unittest
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path

# Add `src` directory to the import paths
sys.path.insert(0, str(Path(__file__).parent.parent / 'src'))

import drules
import merge_styles


def _read_varuint(data, pos):
    shift = 0
    result = 0
    while True:
        b = data[pos]
        pos += 1
        result |= (b & 127) << shift
        if not (b & 128):
            return result, pos
        shift += 7


def _write_varuint(value):
    result = bytearray()
    while value > 127:
        result.append((value & 127) | 128)
        value >>= 7
    result.append(value)
    return result


def _make_element(scale, width):
    element = drules.DrawElement()
    element.scale = scale
    line = drules.LineRule()
    line.width = width
    line.color = 0xFF010203
    element.lines.append(line)
    return element


def _make_container(*types):
    container = drules.Container()
    color = drules.ColorElement()
    color.name = "base"
    color.color = 0xFF010203
    container.colors.value.append(color)

    for name, elements in types:
        rule = drules.ClassifElement()
        rule.name = name
        rule.element.extend(elements)
        container.cont.append(rule)
    return container


class DrulesTest(unittest.TestCase):
    def test_parse_binary_skips_section_padding(self):
        container = _make_container(("highway-primary", [_make_element(10, 2.0)]))
        blob = bytearray(drules.serialize_binary([container], ["light"]))

        pos = len(drules.MAGIC) + 1 + 1
        variant_name_len, pos = _read_varuint(blob, pos)
        pos += variant_name_len

        section_size_pos = pos
        section_size, section_body_pos = _read_varuint(blob, section_size_pos)
        section_end = section_body_pos + section_size

        encoded_size = _write_varuint(section_size + 1)
        blob[section_size_pos:section_body_pos] = encoded_size
        section_end += len(encoded_size) - (section_body_pos - section_size_pos)
        blob[section_end:section_end] = b"\0"

        variant_names, containers = drules.parse_binary(bytes(blob))

        self.assertEqual(variant_names, ["light"])
        self.assertEqual(len(containers[0].cont), 1)

    def test_merge_styles_preserves_missing_type_elements_and_colors(self):
        base = _make_container(("type-b", [_make_element(10, 1.0)]))
        alternative = _make_container(("type-a", [_make_element(5, 2.0), _make_element(7, 4.0)]))

        with redirect_stdout(StringIO()):
            diff = merge_styles.create_diff(merge_styles.read_zoom_extremes(base),
                                            merge_styles.read_zoom_extremes(alternative))
        merged = merge_styles.apply_diff(base, diff)

        self.assertEqual(len(merged.colors.value), 1)
        self.assertEqual([rule.name for rule in merged.cont], ["type-a", "type-b"])
        self.assertEqual([element.scale for element in merged.cont[0].element], [5, 7])
        self.assertEqual([element.lines[0].width for element in merged.cont[0].element], [2.0, 4.0])

    def test_serialize_text_inlines_colors(self):
        light = _make_container(("highway-primary", [_make_element(10, 2.0)]))
        dark = _make_container(("highway-primary", [_make_element(10, 2.0)]))
        dark.cont[0].element[0].lines[0].color = 0xFF0A0B0C

        text = drules.serialize_text([light, dark], ["light", "dark"])

        self.assertIn("  base #FF010203", text.splitlines())  # The same value in both variants.
        self.assertIn(" color=#FF010203/#FF0A0B0C ", text)

    def test_serialize_text_keeps_other_lines_when_a_color_is_added(self):
        base = _make_container(("highway-primary", [_make_element(10, 2.0)]))
        extended = _make_container(("highway-primary", [_make_element(10, 2.0)]))
        extra = drules.ColorElement()
        extra.name = "extra"
        extra.color = 0xFF112233
        extended.colors.value.insert(0, extra)

        lines = drules.serialize_text([base], ["light"]).splitlines()
        extended_lines = drules.serialize_text([extended], ["light"]).splitlines()

        self.assertEqual([line for line in extended_lines if "extra" not in line], lines)


if __name__ == '__main__':
    unittest.main()
