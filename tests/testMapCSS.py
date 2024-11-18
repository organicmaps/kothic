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

        decl = parseDeclaration(""" linejoin: round; """)
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
            pattern-spacing: 90 ;""")
        self.assertEqual(len(decl), 1)
        self.assertEqual(decl[0], {
            "pattern-offset": "90",
            "pattern-image": "arrow-m.svg",
            "pattern-spacing": "90",
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

    def test_parse_include(self):
        # TODO: Prepare *.mapcss files with @include(...)
        pass

    def test_parse_include(self):
        parser = MapCSS()
        dynamic_tags = {"tourism": True, "office": True,
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
""", dynamic_tags=dynamic_tags)

        self.assertEqual(len(parser.choosers), 1)
        self.assertEqual(len(parser.choosers[0].ruleChains), 8)

if __name__ == '__main__':
    unittest.main()
