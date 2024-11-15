import unittest
import sys
from pathlib import Path

# Add `src` directory to the import paths
sys.path.insert(0, str(Path(__file__).parent.parent / 'src'))

from mapcss.Eval import Eval

class EvalTest(unittest.TestCase):
    def test_eval_tag(self):
        a = Eval("""eval( tag("lanes") )""")
        self.assertEqual(a.compute({"lanes": "4"}), "4")
        self.assertEqual(a.compute({"natural": "trees"}), "")
        self.assertSetEqual(a.extract_tags(), {"lanes"})

    def test_complex_eval(self):
        a = Eval(""" eval( any( metric(tag("height")), metric ( num(tag("building:levels")) * 3), metric("1m"))) """)
        self.assertEqual(a.compute({"building:levels": "3"}), "9")
        self.assertSetEqual(a.extract_tags(), {"height", "building:levels"})

if __name__ == '__main__':
    unittest.main()
