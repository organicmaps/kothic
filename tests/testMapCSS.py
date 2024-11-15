import unittest
import sys
from pathlib import Path

# Add `src` directory to the import paths
sys.path.insert(0, str(Path(__file__).parent.parent / 'src'))

from mapcss import parseDeclaration

class MapCSSTest(unittest.TestCase):
    """ Test eval(...) feature for CSS properties.
        NOTE: eval() is not used in Organic Maps styles. We can drop it completely.
    """
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


if __name__ == '__main__':
    unittest.main()
