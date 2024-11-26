import unittest
import sys
from pathlib import Path

from libkomwm import komap_mapswithme

# Add `src` directory to the import paths
sys.path.insert(0, str(Path(__file__).parent.parent / 'src'))

class Options(object):
    pass

class MapCSSTest(unittest.TestCase):
    def test_generate_drules(self):
        assets_dir = Path(__file__).parent / 'assets' / 'case-2-generate-drules'
        options = Options()
        options.data = None
        options.minzoom = 0
        options.maxzoom = 20
        options.txt = True
        options.filename = str( assets_dir / "main.mapcss" )
        options.outfile = str( assets_dir / "style.bin" )
        options.priorities_path = str( assets_dir / "include" )

        komap_mapswithme(options)
