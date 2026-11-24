import unittest
import sys
from pathlib import Path
import shutil
import subprocess
from tempfile import TemporaryDirectory

# Add `src` directory to the import paths
sys.path.insert(0, str(Path(__file__).parent.parent / 'src'))

import drules
import libkomwm


class LibKomwmTest(unittest.TestCase):
    def test_get_type_tags(self):
        def items(selectors):
            # The order matters: the first tag is the type's main one.
            return list(libkomwm.get_type_tags(selectors).items())

        self.assertEqual(items('[highway=primary][bridge?]'), [('highway', 'primary'), ('bridge', 'yes')])
        # Only the first selector counts.
        self.assertEqual(items('[amenity=parking][fee],[amenity=parking][parking=lane]'),
                         [('amenity', 'parking'), ('fee', 'yes')])
        # A forbidden key is absent, so that MapCSS [!tunnel] matches the type.
        self.assertEqual(items('[natural=water][intermittent=yes][!tunnel]'),
                         [('natural', 'water'), ('intermittent', 'yes')])

    def generate_mini(self, directory, without_fork=False):
        fixture = Path(__file__).parent / 'assets' / 'case-2-generate-drules-mini'
        shutil.copytree(fixture, directory)
        src = Path(libkomwm.__file__).parent
        command = [sys.executable, str(src / 'libkomwm.py')]
        if without_fork:
            # Exercise the default compiler path when Windows-style start methods are available.
            command = [sys.executable, '-c',
                       "import sys; sys.path.insert(0, sys.argv.pop(1)); import libkomwm; "
                       "from unittest.mock import Mock; "
                       "libkomwm.get_all_start_methods = Mock(return_value=['spawn']); "
                       "libkomwm.get_context = Mock(side_effect=AssertionError('fork unavailable')); "
                       "libkomwm.main()", str(src)]
        command += ['-s', str(directory / 'main.mapcss'), '-o', str(directory / 'style_output'),
                    '-p', str(directory / 'include'), '-d', str(directory), '-f', '0', '-t', '10', '-x']
        run = subprocess.run(command, capture_output=True, text=True)
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)

    def test_generate_drules_mini(self):
        with TemporaryDirectory() as tmp:
            assets_dir = Path(tmp) / 'mini'
            self.generate_mini(assets_dir)
            lines = [line.strip() for line in (assets_dir / 'types.txt').read_text().splitlines()]
            self.assertEqual(len(lines), 1173, "Generated types.txt file should contain 1173 lines")
            self.assertEqual(len([line for line in lines if line != 'mapswithme']), 148,
                             "Actual types count should be 148 as in mapcss-mapping.csv")
            container = drules.load_container(assets_dir / 'style_output.bin')
            self.assertEqual(len(container.cont), 20,
                             "Generated style_output.bin should contain 20 types with drawing rules")

    def test_generate_drules_without_fork_matches_default(self):
        with TemporaryDirectory() as tmp:
            default, serial = Path(tmp) / 'default', Path(tmp) / 'serial'
            self.generate_mini(default)
            self.generate_mini(serial, without_fork=True)
            for name in ('classificator.txt', 'types.txt', 'colors.txt', 'patterns.txt',
                         'visibility.txt', 'style_output.bin', 'style_output.txt'):
                self.assertEqual((default / name).read_bytes(), (serial / name).read_bytes(), name)

    def compile_classifier_style(self, css, error=None, type_name='highway-tertiary'):
        with TemporaryDirectory() as tmp:
            directory = Path(tmp)
            (directory / 'style.mapcss').write_text(css, encoding='utf-8')
            (directory / 'mapcss-dynamic.txt').write_text('name\npopulation\nbbox_area\nextra_tag\n', encoding='utf-8')
            (directory / 'mapcss-mapping.csv').write_text(
                'highway|tertiary;1;\nhwtag|oneway;2;\nhwtag|lit;3;\nhighway|path|bicycle;4;\n'
                'natural|land;5;\nrailway|rail;6;\n', encoding='utf-8')
            priorities = {
                'FG': 'highway-tertiary\nhighway-path-bicycle\nrailway-rail\nhwtag-oneway\nhwtag-lit\nnatural-land\n'
                      '=== 100\n'
                      'highway-tertiary::outer\n=== 110\nhighway-tertiary::stripe\n=== 120\n',
                'overlays': 'highway-tertiary::label\n=== 100\n',
                'BG-by-size': 'natural-land\n=== 100\n',
            }
            for number, group in enumerate(('BG-by-size', 'BG-top', 'FG', 'overlays'), 1):
                (directory / f'priorities_{number}_{group}.prio.txt').write_text(
                    priorities.get(group, ''), encoding='utf-8')

            types_file = directory / 'types.txt'
            types_file.write_text('existing types\n', encoding='utf-8')
            output = directory / 'rules'
            if error is not None:
                output.with_suffix('.bin').write_bytes(b'existing rules')
                original = {p.name: p.read_bytes() for p in directory.iterdir()}
            run = subprocess.run([sys.executable, libkomwm.__file__, '-s', str(directory / 'style.mapcss'),
                                  '-o', str(output), '-d', tmp, '-p', tmp, '-t', '15'], capture_output=True, text=True)
            if error is not None:
                self.assertNotEqual(run.returncode, 0, run.stdout + run.stderr)
                self.assertIn(error, run.stderr)
                self.assertEqual({p.name: p.read_bytes() for p in directory.iterdir()}, original)
                return
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
            rules = drules.load_container(output.with_suffix('.bin'))
            entry = next(entry for entry in rules.cont if entry.name == type_name)
            return [element for element in entry.element if element.scale == 15]

    def test_shield_font_size_override(self):
        css = """
line|z15[highway=tertiary] { width: 1; color: #000000; }
line|z15[highway=tertiary]::label { shield-color: #ffffff; shield-font-size: 9; }
"""
        [element] = self.compile_classifier_style(css)
        self.assertEqual(element.shield.height, 9)
        for size in ('0', 'eval(prop("missing"))'):
            with self.subTest(size=size):
                override = f'line|z15[highway=tertiary]::label {{ shield-font-size: {size}; }}'
                [element] = self.compile_classifier_style(css + override)
                self.assertFalse(element.shield._is_set())

    def test_invalid_shield_size_preserves_outputs(self):
        for size in ('-1', 'large', '2147483648'):
            with self.subTest(size=size):
                self.compile_classifier_style(
                    f'line|z15[highway=tertiary] {{ width: 2; shield-font-size: {size}; }}',
                    error=f'Invalid shield-font-size {size!r} for highway-tertiary at z15')

    def test_casing_width_cap_join_and_priority(self):
        [element] = self.compile_classifier_style("""
line|z15[highway=tertiary] { width: 4; casing-width-add: 1; casing-linecap: round; casing-linejoin: bevel; }
line|z15[highway=tertiary]::outer { casing-width: 1; casing-width-add: 7; casing-linecap: round; }
""")
        self.assertEqual([(line.width, line.cap, line.join, line.priority) for line in element.lines], [
            (10, drules.ROUNDCAP, drules.BEVELJOIN, 99),
            (4, drules.BUTTCAP, drules.ROUNDJOIN, 100),
            (6, drules.ROUNDCAP, drules.ROUNDJOIN, 110),
        ])

    def test_square_line_and_casing_caps(self):
        [element] = self.compile_classifier_style("""
line|z15[highway=tertiary] { width: 4; linecap: square; casing-width: 2; casing-linecap: square; }
""")
        self.assertEqual([(line.width, line.cap, line.priority) for line in element.lines], [
            (8, drules.SQUARECAP, 99),
            (4, drules.SQUARECAP, 100),
        ])

    def test_invalid_caps_and_joins_preserve_outputs(self):
        for key in ('linecap', 'linejoin', 'casing-linecap', 'casing-linejoin'):
            with self.subTest(key=key):
                self.compile_classifier_style(
                    f'line|z15[highway=tertiary]::outer {{ width: 4; casing-width: 1; {key}: typo; }}',
                    error=f"Unsupported {key} 'typo' for highway-tertiary::outer at z15")

    def matching_lines(self, elements, *types, named=False, population=0):
        def matches(guard):
            if guard in ('name', '!name'):
                return named == (guard == 'name')
            if guard.startswith('population>='):
                return population >= int(guard.removeprefix('population>='))
            if guard.startswith('population<'):
                return population < int(guard.removeprefix('population<'))
            operator, path = guard.split('=', 1)
            self.assertIn(operator, ('extra_tag', 'extra_tag!'))
            present = any(kind == path or kind.startswith(path + '=') for kind in types)
            return present == (operator == 'extra_tag')

        return [line for element in elements if all(matches(guard) for guard in element.apply_if)
                for line in element.lines]

    def test_classifier_override_keeps_base_rule(self):
        rules = self.compile_classifier_style('''
line|z15[highway=tertiary] { width: 1; color: #000000; }
line|z15[highway=tertiary][ hwtag = oneway ] { width: 3; }
''')
        self.assertEqual([line.width for line in self.matching_lines(rules)], [1])
        self.assertEqual([line.width for line in self.matching_lines(rules, 'hwtag=oneway')], [3])

    def test_classifier_group_applies_when_either_alternative_matches(self):
        rules = self.compile_classifier_style('''
line|z15[highway=tertiary] { width: 1; color: #000000; }
line|z15[highway=tertiary][hwtag=oneway],
line|z15[highway=tertiary][extra_tag!=highway = path] { width: 2; }
''')
        self.assertEqual([line.width for line in self.matching_lines(rules, 'highway=path=bicycle')], [1])
        for types in [(), ('hwtag=oneway',), ('hwtag=oneway', 'highway=path=bicycle')]:
            with self.subTest(types=types):
                self.assertEqual([line.width for line in self.matching_lines(rules, *types)], [2])

    def test_classifier_overrides_cascade_when_both_match(self):
        rules = self.compile_classifier_style('''
line|z15[highway=tertiary] { width: 1; color: #000000; }
line|z15[highway=tertiary][hwtag=oneway] { width: 3; }
line|z15[highway=tertiary][hwtag=lit] { dashes: 2,4; }
''')
        [line] = self.matching_lines(rules, 'hwtag=oneway', 'hwtag=lit')
        self.assertEqual(line.width, 3)
        self.assertEqual(line.dashdot.dd, [2, 4])

    def test_classifier_group_preserves_layer_order_for_eval(self):
        rules = self.compile_classifier_style('''
line|z15[highway=tertiary] { width: 1; color: #000000; }
line|z15[highway=tertiary][hwtag=oneway]::stripe,
line|z15[highway=tertiary]::outer,
line|z15[highway=tertiary]::stripe { width: eval(prop("width") + 1); color: eval(prop("color")); }
''')
        self.assertEqual({line.priority: line.width for line in self.matching_lines(rules)}, {100: 1, 110: 2, 120: 3})
        self.assertEqual({line.priority: line.width for line in self.matching_lines(rules, 'hwtag=oneway')},
                         {100: 1, 110: 3, 120: 2})

    def test_classifier_branch_with_name_guards(self):
        rules = self.compile_classifier_style('''
line|z15[highway=tertiary] { width: 1; color: #000000; }
line|z15[highway=tertiary][hwtag=oneway][name] { width: 3; }
line|z15[highway=tertiary][hwtag=oneway][!name] { width: 2; }
''')
        for named, width in [(False, 2), (True, 3)]:
            with self.subTest(named=named):
                self.assertEqual([line.width for line in self.matching_lines(rules, named=named)], [1])
                self.assertEqual([line.width for line in self.matching_lines(rules, 'hwtag=oneway', named=named)], [width])

    def test_unconditional_alternative_covers_nameless_features(self):
        rules = self.compile_classifier_style('''
line|z15[highway=tertiary], line|z15[highway=tertiary][name] { width: 1; color: #000000; }
''')
        [rule] = rules
        self.assertEqual(rule.apply_if, [])
        self.assertEqual([line.width for line in rule.lines], [1])

    def test_classifier_branch_partitions_overlapping_runtime_guards(self):
        rules = self.compile_classifier_style('''
line|z15[highway=tertiary] { width: 1; color: #000000; }
line|z15[highway=tertiary][hwtag=oneway][name] { width: 3; }
line|z15[highway=tertiary][hwtag=oneway][population=1000] { dashes: 2,4; }
''')
        for oneway, named in [((), False), ((), True), (('hwtag=oneway',), False), (('hwtag=oneway',), True)]:
            for population in (999, 1000, 1001):
                with self.subTest(oneway=oneway, named=named, population=population):
                    [line] = self.matching_lines(rules, *oneway, named=named, population=population)
                    self.assertEqual(line.width, 3 if oneway and named else 1)
                    self.assertEqual(line.dashdot.dd, [2, 4] if oneway and population == 1000 else [])

    def test_classifier_only_alternative_is_indexed_for_other_types(self):
        rules = self.compile_classifier_style('''
line|z15[railway=rail] { width: 1; color: #000000; }
line|z15[highway=tertiary], line|z15[extra_tag=hwtag=oneway] { width: 3; color: #000000; }
''', type_name='railway-rail')
        self.assertEqual([line.width for line in self.matching_lines(rules)], [1])
        self.assertEqual([line.width for line in self.matching_lines(rules, 'hwtag=oneway')], [3])

    def test_impossible_classifier_alternative_needs_no_priority(self):
        rules = self.compile_classifier_style('''
line|z15[highway=tertiary] { width: 1; color: #000000; }
line|z15[highway=tertiary][extra_tag!=hwtag][extra_tag=hwtag=oneway]::impossible { width: 2; }
''')
        [rule] = rules
        self.assertEqual(rule.apply_if, [])
        self.assertEqual([line.width for line in rule.lines], [1])

    def test_invalid_classifier_paths_fail_compilation(self):
        for condition, error in [('extra_tag=hwtag==oneway', 'Empty classifier path component'),
                                 ('extra_tag=hwtag=missing', 'Unknown extra_tag classifier path hwtag=missing'),
                                 ('extra_tag>hwtag=oneway', 'Unsupported classifier condition'),
                                 ('hwtag=~/oneway/', 'Unsupported classifier condition')]:
            with self.subTest(condition=condition):
                self.compile_classifier_style(
                    f'line|z15[highway=tertiary][{condition}] {{ width: 2; color: #000000; }}', error=error)

    def test_guarded_path_text_requires_line_in_same_branch(self):
        self.compile_classifier_style('''
line|z15[highway=tertiary] { width: 1; color: #000000; }
line|z15[highway=tertiary]::label { text: name; text-position: line; text-color: #000000; }
line|z15[highway=tertiary][hwtag=oneway] { width: 0; }
''', error='Path text or shield without line at z15 for highway-tertiary')

    def test_background_cannot_depend_on_classifier(self):
        self.compile_classifier_style('''
area|z15[natural=land] { fill-color: #111111; }
area|z15[natural=land][hwtag=lit] { fill-color: #222222; }
''', error='Feature-dependent background at z15 for natural-land')

    def test_classifier_cascade_rejects_predicates_without_a_complement(self):
        self.compile_classifier_style('''
line|z15[highway=tertiary][hwtag=oneway][bbox_area>10] { width: 1; color: #000000; }
''', error='Unsupported runtime condition in a selector cascade: bbox_area>10')
