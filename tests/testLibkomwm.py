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

    def compile_classifier_style(self, css, error=None):
        with TemporaryDirectory() as tmp:
            directory = Path(tmp)
            (directory / 'style.mapcss').write_text(css, encoding='utf-8')
            (directory / 'mapcss-dynamic.txt').write_text('name\nextra_tag\n', encoding='utf-8')
            (directory / 'mapcss-mapping.csv').write_text(
                'highway|tertiary;1;\nhwtag|oneway;2;\nhwtag|lit;3;\nhighway|path|bicycle;4;\n', encoding='utf-8')
            for number, group in enumerate(('BG-by-size', 'BG-top', 'FG', 'overlays'), 1):
                (directory / f'priorities_{number}_{group}.prio.txt').write_text(
                    'highway-tertiary\nhighway-tertiary::outer\nhighway-tertiary::stripe\n=== 100\n'
                    if group == 'FG' else '', encoding='utf-8')

            output = directory / 'rules'
            run = subprocess.run([sys.executable, libkomwm.__file__, '-s', str(directory / 'style.mapcss'),
                                  '-o', str(output), '-d', tmp, '-p', tmp, '-t', '15'], capture_output=True, text=True)
            if error is not None:
                self.assertNotEqual(run.returncode, 0, run.stdout + run.stderr)
                self.assertIn(error, run.stderr)
                self.assertFalse(output.with_suffix('.bin').exists())
                return
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
            rules = drules.load_container(output.with_suffix('.bin'))
            road = next(entry for entry in rules.cont if entry.name == 'highway-tertiary')
            return [element for element in road.element if element.scale == 15]

    def matching_lines(self, elements, *types, named=False):
        def matches(guard):
            if guard in ('name', '!name'):
                return named == (guard == 'name')
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
        self.assertEqual([line.width for line in self.matching_lines(rules)], [1, 2, 3])
        self.assertEqual([line.width for line in self.matching_lines(rules, 'hwtag=oneway')], [1, 3, 2])

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

    def test_invalid_classifier_paths_fail_compilation(self):
        for condition, error in [('extra_tag=hwtag==oneway', 'Empty classifier path component'),
                                 ('extra_tag=hwtag=missing', 'Unknown extra_tag classifier paths hwtag=missing'),
                                 ('extra_tag>hwtag=oneway', 'Unsupported classifier condition')]:
            with self.subTest(condition=condition):
                self.compile_classifier_style(
                    f'line|z15[highway=tertiary][{condition}] {{ width: 2; color: #000000; }}', error=error)

    def test_generate_drules_validation_errors(self):
        assets_dir = Path(__file__).parent / 'assets' / 'case-3-styles-validation'  # noqa: F841
        # TODO: needs refactoring of libkomwm.validation_errors_count to have a list
        #       of validation errors.
        self.assertTrue(True)
