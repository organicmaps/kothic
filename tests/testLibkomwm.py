import unittest
import sys
from pathlib import Path

# Add `src` directory to the import paths
sys.path.insert(0, str(Path(__file__).parent.parent / 'src'))

import libkomwm
from libkomwm import komap_mapswithme

class MapCSSTest(unittest.TestCase):
    def test_generate_drules(self):
        assets_dir = Path(__file__).parent / 'assets' / 'case-2-generate-drules'

        class Options(object):
            pass

        options = Options()
        options.data = None
        options.minzoom = 0
        options.maxzoom = 20
        options.txt = True
        options.filename = str( assets_dir / "main.mapcss" )
        options.outfile = str( assets_dir / "style.bin" )
        options.priorities_path = str( assets_dir / "include" )

        try:
            # Save state
            prio_ranges_orig = libkomwm.prio_ranges.copy()

            komap_mapswithme(options)

            # Restore state
            libkomwm.prio_ranges = prio_ranges_orig
            self.assertTrue(True, "Completed with no errors!")
            # TODO: Check generated files content
        finally:
            # Clean up generated files
            files2delete = ["classificator.txt", "colors.txt", "patterns.txt", "style.bin.bin",
                            "style.bin.txt", "types.txt", "visibility.txt"]
            for filename in files2delete:
                (assets_dir / filename).unlink(missing_ok=True)

    def test_generate_drules_mini(self):
        assets_dir = Path(__file__).parent / 'assets' / 'case-3-generate-drules-mini'

        class Options(object):
            pass

        options = Options()
        options.data = None
        options.minzoom = 0
        options.maxzoom = 20
        options.txt = True
        options.filename = str( assets_dir / "main.mapcss" )
        options.outfile = str( assets_dir / "style.bin" )
        options.priorities_path = str( assets_dir / "include" )

        try:
            # Save state
            libkomwm.MULTIPROCESSING = False
            prio_ranges_orig = libkomwm.prio_ranges.copy()

            komap_mapswithme(options)

            # Restore state
            libkomwm.prio_ranges = prio_ranges_orig
            libkomwm.MULTIPROCESSING = True
            self.assertTrue(True, "Completed with no errors!")
            # TODO: Check generated files content
        finally:
            # Clean up generated files
            files2delete = ["classificator.txt", "colors.txt", "patterns.txt", "style.bin.bin",
                            "style.bin.txt", "types.txt", "visibility.txt"]
            for filename in files2delete:
                (assets_dir / filename).unlink(missing_ok=True)

    def test_generate_drules_validation_errors(self):
        assets_dir = Path(__file__).parent / 'assets' / 'case-4-styles-validation'
        # TODO: needs refactoring of libkomwm.validation_errors_count to have a list
        #       of validation errors.
        self.assertTrue(True)
