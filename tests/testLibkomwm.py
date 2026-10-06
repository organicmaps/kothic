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

    def test_generate_drules_validation_errors(self):
        assets_dir = Path(__file__).parent / 'assets' / 'case-3-styles-validation'  # noqa: F841
        # TODO: needs refactoring of libkomwm.validation_errors_count to have a list
        #       of validation errors.
        self.assertTrue(True)
