import unittest
import sys
from pathlib import Path

# Add `src` directory to the import paths
sys.path.insert(0, str(Path(__file__).parent.parent / 'src'))

from mapcss import parseDeclaration, MapCSS


class MapCSSTest(unittest.TestCase):
    def test_declarations(self):
        decl = parseDeclaration(""" linejoin: round; """)
        self.assertEqual(len(decl), 1)
        self.assertEqual(decl[0], {"linejoin": "round"})

        decl = parseDeclaration("""\tlinejoin :\nround ; """)
        self.assertEqual(len(decl), 1)
        self.assertEqual(decl[0], {"linejoin": "round"})

        decl = parseDeclaration(""" icon-image: parking_private-s.svg; text: "name"; """)
        self.assertEqual(len(decl), 1)
        self.assertEqual(decl[0], {
            "icon-image": "parking_private-s.svg",
            "text": "name"
        })

        decl = parseDeclaration("""
            pattern-offset: 90\t;
            pattern-image:\tarrow-m.svg   ;
            pattern-spacing: @trunk0 ;""")
        self.assertEqual(len(decl), 1)
        self.assertEqual(decl[0], {
            "pattern-offset": "90",
            "pattern-image": "arrow-m.svg",
            "pattern-spacing": "@trunk0",
        })

    def test_parse_variables(self):
        parser = MapCSS()
        parser.parse("""
@city_label: #999999;
@country_label: #444444;
@wave_length: 25;
""")
        self.assertEqual(parser.variables, {
            "city_label": "#999999",
            "country_label": "#444444",
            "wave_length": "25"
        })

    def test_parse_colors(self):
        parser = MapCSS()
        parser.parse("""
@city_label : #999999;
@country_label: #444444 ;
  @wave_length: 25;
""")
        self.assertEqual(parser.variables, {
            "city_label": "#999999",
            "country_label": "#444444",
            "wave_length": "25"
        })

    def test_parse_import(self):
        parser = MapCSS()
        mapcssFile = Path(__file__).parent / 'assets' / 'case-1-import' / 'main.mapcss'
        parser.parse(filename=str(mapcssFile))

        colors = parser.get_colors()
        self.assertEqual(colors, {
            "GuiText-color": (1.0, 1.0, 1.0),
            "GuiText-opacity": 0.7,
            "Route-color": (0.0, 0.0, 1.0),
            "Route-opacity": 0.5,
        })

    def test_parse_basic_chooser(self):
        parser = MapCSS()
        static_tags = {"tourism": True, "office": True,
                        "craft": True, "amenity": True}
        parser.parse("""
node|z17-[tourism],
area|z17-[tourism],
node|z18-[office],
area|z18-[office],
node|z18-[craft],
area|z18-[craft],
node|z19-[amenity],
area|z19-[amenity],
{text: name; text-color: #000030; text-offset: 1;}
""", static_tags=static_tags)

        self.assertEqual(len(parser.choosers), 1)
        self.assertEqual(len(parser.choosers[0].ruleChains), 8)

    def test_parse_basic_chooser_2(self):
        parser = MapCSS()
        static_tags = {"highway": True}
        parser.parse("""
@trunk0: #FF7326;

line|z6[highway=trunk],
line|z6[highway=motorway],
{color: @trunk0; opacity: 0.3;}
line|z7-9[highway=trunk],
line|z7-9[highway=motorway],
{color: @trunk0; opacity: 0.7;}
""", static_tags=static_tags)

        self.assertEqual(len(parser.choosers), 2)
        self.assertEqual(len(parser.choosers[0].ruleChains), 2)
        self.assertEqual(parser.choosers[0].ruleChains[0].subject, 'line')
        self.assertEqual(parser.choosers[0].selzooms, [6, 6])
        self.assertEqual(parser.choosers[1].selzooms, [7, 9])

        rule, object_id = parser.choosers[0].testChains({"highway": "trunk"})
        self.assertEqual(object_id, "::default")

    def test_parse_basic_chooser_3(self):
        parser = MapCSS()
        static_tags = {"addr:housenumber": True, "addr:street": False}
        parser.parse("""
/* Some Comment Here */

/*
   This sample is borrowed from Organic Maps Basemap_label.mapcss file
 */
node|z18-[addr:housenumber][addr:street]::int_name
{text: int_name; text-color: #65655E; text-position: center;}
""", static_tags=static_tags)

        building_tags = {"building": "yes", "addr:housenumber": "12", "addr:street": "Baker street"}

        # Check that mapcss parsed correctly
        self.assertEqual(len(parser.choosers), 1)
        styleChooser = parser.choosers[0]
        self.assertEqual(len(styleChooser.ruleChains), 1)
        self.assertEqual(styleChooser.selzooms, [18, 19])
        rule, object_id = styleChooser.testChains(building_tags)
        self.assertEqual(object_id, "::int_name")

        rule = styleChooser.ruleChains[0]
        self.assertEqual(rule.subject, 'node')
        self.assertEqual(rule.extract_tags(), {'addr:housenumber', 'addr:street'})

    def test_parse_basic_chooser_class(self):
        parser = MapCSS()
        parser.parse("""
way|z-13::*
{
  linejoin: round;
}
""")

        # Check that mapcss parsed correctly
        self.assertEqual(len(parser.choosers), 1)
        styleChooser = parser.choosers[0]
        self.assertEqual(len(styleChooser.ruleChains), 1)
        self.assertEqual(styleChooser.selzooms, [0, 13])
        rule, object_id = styleChooser.testChains({})
        self.assertEqual(object_id, "::*")

        rule = styleChooser.ruleChains[0]
        self.assertEqual(rule.subject, 'way')
        self.assertEqual(rule.extract_tags(), {'*'})

    def test_parse_basic_chooser_class_2(self):
        parser = MapCSS()
        parser.parse("""
way|z10-::*
{
  linejoin: round;
}
""")

        # Check that mapcss parsed correctly
        self.assertEqual(len(parser.choosers), 1)
        styleChooser = parser.choosers[0]
        self.assertEqual(len(styleChooser.ruleChains), 1)
        self.assertEqual(styleChooser.selzooms, [10, 19])
        rule, object_id = styleChooser.testChains({})
        self.assertEqual(object_id, "::*")

        rule = styleChooser.ruleChains[0]
        self.assertEqual(rule.subject, 'way')
        self.assertEqual(rule.extract_tags(), {'*'})

    def test_parse_basic_chooser_colors(self):
        parser = MapCSS()
        parser.parse("""
way|z-6::*
{
  linejoin: round;
}

colors {
  GuiText-color: #FFFFFF;
  GuiText-opacity: 0.7;
  MyPositionAccuracy-color: #FFFFFF;
  MyPositionAccuracy-opacity: 0.06;
  Selection-color: #FFFFFF;
  Selection-opacity: 0.64;
  Route-color: #0000FF;
  RouteOutline-color: #00FFFF;
}
""")

        # Check that colors from mapcss parsed correctly
        colors = parser.get_colors()
        self.assertEqual(colors, {
            "GuiText-color": (1.0, 1.0, 1.0),
            "GuiText-opacity": 0.7,
            "MyPositionAccuracy-color": (1.0, 1.0, 1.0),
            "MyPositionAccuracy-opacity": 0.06,
            "Selection-color": (1.0, 1.0, 1.0),
            "Selection-opacity": 0.64,
            "Route-color": (0.0, 0.0, 1.0),
            "RouteOutline-color": (0.0, 1.0, 1.0)
        })


if __name__ == '__main__':
    unittest.main()
