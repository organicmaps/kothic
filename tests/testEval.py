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

    def test_eval_prop(self):
        a = Eval("""eval( prop("dpi") / 2 )""")
        self.assertEqual(a.compute({"lanes": "4"}, {"dpi": 144}), "72")
        self.assertEqual(a.compute({"lanes": "4"}, {"orientation": "vertical"}), "")

    def test_eval_num(self):
        a = Eval("""eval( num(tag("lanes")) + 2 )""")
        self.assertEqual(a.compute({"lanes": "4"}), "6")
        self.assertEqual(a.compute({"lanes": "many"}), "2")

    def test_eval_metric(self):
        a = Eval("""eval( metric(tag("height")) )""")
        self.assertEqual(a.compute({"height": "512"}), "512")
        self.assertEqual(a.compute({"height": "10m"}), "10")
        self.assertEqual(a.compute({"height": " 10m"}), "10")
        self.assertEqual(a.compute({"height": "500cm"}), "5")
        self.assertEqual(a.compute({"height": "500 cm"}), "5")
        self.assertEqual(a.compute({"height": "250CM"}), "2.5")
        self.assertEqual(a.compute({"height": "250 CM"}), "2.5")
        self.assertEqual(a.compute({"height": "30см"}), "0.3")
        self.assertEqual(a.compute({"height": " 30 см"}), "0.3")
        self.assertEqual(a.compute({"height": "1200 mm"}), "1.2")
        self.assertEqual(a.compute({"height": "2400MM"}), "2.4")
        self.assertEqual(a.compute({"height": "2800 мм"}), "2.8")

    def test_eval_metric_with_scale(self):
        a = Eval("""eval( metric(tag("height")) )""")
        self.assertEqual(a.compute({"height": "512"}, xscale=4), "2048")
        self.assertEqual(a.compute({"height": "512"}, zscale=4), "512")
        self.assertEqual(a.compute({"height": "10m"}, xscale=4), "40")
        self.assertEqual(a.compute({"height": " 10m"}, xscale=4), "40")
        self.assertEqual(a.compute({"height": "500cm"}, xscale=4), "20")
        self.assertEqual(a.compute({"height": "500 cm"}, xscale=4), "20")
        self.assertEqual(a.compute({"height": "250CM"}, xscale=4), "10")
        self.assertEqual(a.compute({"height": "250 CM"}, xscale=4), "10")
        self.assertEqual(a.compute({"height": "30см"}, xscale=4), "1.2")
        self.assertEqual(a.compute({"height": " 30 см"}, xscale=4), "1.2")
        self.assertEqual(a.compute({"height": "1200 mm"}, xscale=4), "4.8")
        self.assertEqual(a.compute({"height": "2400MM"}, xscale=4), "9.6")
        self.assertEqual(a.compute({"height": "2800 мм"}, xscale=4), "11.2")

    def test_complex_eval(self):
        a = Eval(""" eval( any( metric(tag("height")), metric ( num(tag("building:levels")) * 3), metric("1m"))) """)
        self.assertEqual(a.compute({"building:levels": "3"}), "9")
        self.assertSetEqual(a.extract_tags(), {"height", "building:levels"})

if __name__ == '__main__':
    unittest.main()
