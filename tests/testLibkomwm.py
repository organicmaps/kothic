import unittest
import sys
from pathlib import Path
from copy import deepcopy

# Add `src` directory to the import paths
sys.path.insert(0, str(Path(__file__).parent.parent / 'src'))

import drules
import libkomwm
from libkomwm import komap_mapswithme


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

    def test_generate_drules_mini(self):
        assets_dir = Path(__file__).parent / 'assets' / 'case-2-generate-drules-mini'

        class Options(object):
            pass

        options = Options()
        options.data = None
        options.minzoom = 0
        options.maxzoom = 10
        options.txt = True
        options.filename = str( assets_dir / "main.mapcss" )
        options.outfile = str( assets_dir / "style_output" )
        options.priorities_path = str( assets_dir / "include" )

        try:
            # Save state
            libkomwm.MULTIPROCESSING = False
            prio_ranges_orig = deepcopy(libkomwm.prio_ranges)
            libkomwm.visibilities = {}

            # Run style generation
            komap_mapswithme(options)

            # Restore state
            libkomwm.prio_ranges = prio_ranges_orig
            libkomwm.MULTIPROCESSING = True
            libkomwm.visibilities = {}

            # Check that types.txt contains 1173 lines
            with open(assets_dir / "types.txt", "rt") as typesFile:
                lines = [line.strip() for line in typesFile]
                self.assertEqual(len(lines), 1173, "Generated types.txt file should contain 1173 lines")
                self.assertEqual(len([line for line in lines if line!="mapswithme"]), 148, "Actual types count should be 148 as in mapcss-mapping.csv")

            # Check that style_output.bin has 20 types with drawing rules.
            container = drules.load_container(assets_dir / "style_output.bin")
            self.assertEqual(len(container.cont), 20,
                             "Generated style_output.bin should contain 20 types with drawing rules")

            def lines_at(type_name, zoom):
                classif = next(c for c in container.cont if c.name == type_name)
                return next(e for e in classif.element if e.scale == zoom).lines

            def element_at(type_name, zoom):
                classif = next(c for c in container.cont if c.name == type_name)
                return next(e for e in classif.element if e.scale == zoom)

            # A zero font size suppresses an inherited shield instead of emitting a zero-height
            # element for the renderer to discard.
            self.assertFalse(element_at("highway-motorway", 10).shield._is_set())
            self.assertEqual(element_at("highway-trunk", 10).shield.height, 9)

            # An automatic casing is rendered below its line (priority - 1), keeps its own
            # linecap and is 2 * casing-width wider than the line. Both casing rules in
            # include/Roads.mapcss use a width of 1.
            casing, line = lines_at("highway-world_level", 4)
            self.assertEqual((line.cap, line.priority), (drules.BUTTCAP, 310))
            self.assertEqual((casing.cap, casing.priority), (drules.ROUNDCAP, 309))
            self.assertAlmostEqual(casing.width, line.width + 2, places=5)

            # 'casing-width-add' widens the line first, so the casing is 2 * (width + add).
            # It must be resolved even when the line carries its own width.
            casing, line = lines_at("highway-world_towns_level", 6)
            self.assertEqual(casing.priority, line.priority - 1)
            self.assertAlmostEqual(casing.width, (line.width + 1) * 2, places=5)

        finally:
            # Clean up generated files
            files2delete = ["classificator.txt", "colors.txt", "patterns.txt", "style_output.bin",
                            "style_output.txt", "types.txt", "visibility.txt"]
            for filename in files2delete:
                (assets_dir / filename).unlink(missing_ok=True)

    def test_generate_drules_validation_errors(self):
        assets_dir = Path(__file__).parent / 'assets' / 'case-3-styles-validation'  # noqa: F841
        # TODO: needs refactoring of libkomwm.validation_errors_count to have a list
        #       of validation errors.
        self.assertTrue(True)
