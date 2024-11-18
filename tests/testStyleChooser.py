import unittest
import sys
from pathlib import Path

# Add `src` directory to the import paths
sys.path.insert(0, str(Path(__file__).parent.parent / 'src'))

from mapcss import parseCondition, Condition
from mapcss.Eval import Eval
from mapcss.StyleChooser import StyleChooser, make_nice_style


class StyleChooserTest(unittest.TestCase):
    def test_rules_chain(self):
        sc = StyleChooser((0, 16))

        sc.newObject()
        sc.addCondition(parseCondition("highway=footway"))
        sc.addCondition(parseCondition("footway=sidewalk"))

        sc.newObject()
        sc.addCondition(parseCondition("highway=footway"))
        sc.addCondition(parseCondition("footway=crossing"))
        sc.addCondition(Condition("eq", ("::class", "::*")))

        self.assertTrue( sc.testChains({ "highway": "footway", "footway": "sidewalk" }) )
        self.assertTrue( sc.testChains({ "highway": "footway", "footway": "crossing" }) )
        self.assertFalse( sc.testChains({ "highway": "footway"}) )
        self.assertFalse( sc.testChains({ "highway": "residential", "footway": "crossing" }) )

        rule1, tt = sc.testChains({ "highway": "footway", "footway": "sidewalk" })
        self.assertEqual(tt, "::default")

        rule2, tt = sc.testChains({ "highway": "footway", "footway": "crossing" })
        self.assertEqual(tt, "::*")

        self.assertNotEqual(rule1, rule2)

    def test_zoom(self):
        sc = StyleChooser((0, 16))

        sc.newObject()
        sc.addZoom( (10, 19) )
        sc.addCondition(parseCondition("railway=station"))
        sc.addCondition(parseCondition("transport=subway"))
        sc.addCondition(parseCondition("city=yerevan"))

        sc.newObject()
        sc.addZoom( (4, 15) )
        sc.addCondition(parseCondition("railway=station"))
        sc.addCondition(parseCondition("transport=subway"))
        sc.addCondition(parseCondition("city=yokohama"))

        rule1, tt = sc.testChains({ "railway": "station", "transport": "subway", "city": "yerevan" })
        self.assertEqual(rule1.minZoom, 10)
        self.assertEqual(rule1.maxZoom, 19)

        rule2, tt = sc.testChains({ "railway": "station", "transport": "subway", "city": "yokohama" })
        self.assertEqual(rule2.minZoom, 4)
        self.assertEqual(rule2.maxZoom, 15)

    def test_extract_tags(self):
        sc = StyleChooser((0, 16))

        sc.newObject()
        sc.addCondition(parseCondition("aerialway=rope_tow"))

        sc.newObject()
        sc.addCondition(parseCondition("piste:type=downhill"))

        self.assertSetEqual(sc.extract_tags(), {"aerialway", "piste:type"})

        sc = StyleChooser((0, 16))

        sc.newObject()
        sc.addCondition(parseCondition("aeroway=terminal"))
        sc.addCondition(parseCondition("building"))

        sc.newObject()
        sc.addCondition(parseCondition("waterway=dam"))
        sc.addCondition(parseCondition("building:part"))

        self.assertSetEqual(sc.extract_tags(), {"waterway", "building:part", "building", "aeroway"})

    def test_make_nice_style(self):
        style = make_nice_style({
            "outline-color": "none",
            "bg-color": "red",
            "dash-color": "#ffff00",
            "front-color": "rgb(0, 255, 255)",
            "line-width": Eval("""eval(min(tag("line_width"), 10))"""),
            "outline-width": "2.5",
            "arrow-opacity": "0.5",
            "offset-2": "20",
            "border-radius": "4",
            "line-extrude": "16",
            "dashes": "3,3,1.5,3",
            "wrong-dashes": "yes, yes, yes, no",
            "make-nice": True,
            "additional-len": 44.5
        })

        expectedStyle = {
            "outline-color": "none",
            "bg-color": (1.0, 0.0, 0.0),
            "dash-color": (1.0, 1.0, 0.0),
            "front-color": (0.0, 1.0, 1.0),
            "line-width": Eval("""eval(min(tag("line_width"), 10))"""),
            "outline-width": 2.5,
            "arrow-opacity": 0.5,
            "offset-2": 20.0,
            "border-radius": 4.0,
            "line-extrude": 16.0,
            "dashes": [3.0, 3.0, 1.5, 3.0],
            "wrong-dashes": [],
            "make-nice": True,
            "additional-len": 44.5
        }

        self.assertEqual(style, expectedStyle)

    def test_styles(self):
        sc = StyleChooser((15, 19))
        sc.newObject()
        sc.addStyles([{
            "width": "1.3",
            "opacity": "0.6",
            "bg-color": "blue"
        }])
        sc.addStyles([{
            "color": "#FFFFFF",
            "casing-width": "+10"
        }])

        self.assertEqual(len(sc.styles), 2)
        self.assertEqual(sc.styles[0], {
            "width": 1.3,
            "opacity": 0.6,
            "bg-color": (0.0, 0.0, 1.0)
        })
        self.assertEqual(sc.styles[1], {
            "color": (1.0, 1.0, 1.0),
            "casing-width": 5.0
        })

    def test_runtime_conditions(self):
        #sc.addRuntimeCondition(Condition(condType, ('extra_tag', cond)))
        pass

if __name__ == '__main__':
    unittest.main()
