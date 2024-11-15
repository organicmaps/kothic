import unittest
import sys
from pathlib import Path

# Add `src` directory to the import paths
sys.path.insert(0, str(Path(__file__).parent.parent / 'src'))

from mapcss import parseCondition, Condition
from mapcss.StyleChooser import StyleChooser


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

    def test_styles(self):
        sc = StyleChooser((15, 19))
        sc.newObject()
        sc.addStyles([{
            "width": "1.3",
            "opacity": "0.6"
        }])
        sc.addStyles([{
            "color": "#808080",
            "width": "1.6"
        }])

        self.assertEqual(len(sc.styles), 2)

    def test_runtime_conditions(self):
        #sc.addRuntimeCondition(Condition(condType, ('extra_tag', cond)))
        pass

if __name__ == '__main__':
    unittest.main()
