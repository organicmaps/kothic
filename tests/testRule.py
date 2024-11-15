import unittest
import sys
from pathlib import Path

# Add `src` directory to the import paths
sys.path.insert(0, str(Path(__file__).parent.parent / 'src'))

from mapcss.Rule import Rule
from mapcss.Condition import Condition
from mapcss import parseCondition

class RuleTest(unittest.TestCase):
    def test_rule_subject(self):
        self.assertEqual(Rule().subject, "")
        self.assertEqual(Rule("*").subject, "")
        self.assertEqual(Rule("way").subject, "way")
        self.assertEqual(Rule("area").subject, "area")
        self.assertEqual(Rule("node").subject, "node")
        self.assertEqual(Rule("planet").subject, "planet")

    def test_rule_type_matches(self):
        self.assertCountEqual(Rule().type_matches, ('area', 'line', 'way', 'node'))
        self.assertCountEqual(Rule("*").type_matches, ('area', 'line', 'way', 'node'))
        self.assertCountEqual(Rule("way").type_matches, ('area', 'line', 'way'))
        self.assertCountEqual(Rule("area").type_matches, ('area', 'way'))
        self.assertCountEqual(Rule("node").type_matches, ('node', ))
        self.assertCountEqual(Rule("planet").type_matches, set())

    def test_rule_with_conditions(self):
        rule = Rule()
        rule.conditions = [
            parseCondition("aeroway=aerodrome"),
            parseCondition("aerodrome=international")
        ]

        tt = rule.test({
            "aeroway": "aerodrome",
            "aerodrome": "international",
            "name": "JFK"
        })
        self.assertTrue(tt)
        self.assertEqual(tt, "::default")

        self.assertFalse(rule.test({
            "aeroway": "aerodrome",
            "name": "JFK"
        }))

    def test_rule_with_class(self):
        rule = Rule()
        rule.conditions = [
            parseCondition("highway=unclassified"),
            parseCondition("bridge?"),
            Condition("eq", ("::class", "::bridgeblack"))
        ]

        tt = rule.test({
            "highway": "unclassified",
            "bridge": "yes",
            "layer": "1"
        })
        self.assertTrue(tt)
        self.assertEqual(tt, "::bridgeblack")

        self.assertFalse(rule.test({
            "highway": "unclassified",
            "bridge": "no",
            "layer": "1"
        }))
        self.assertFalse(rule.test({
            "highway": "unclassified",
            "tunnel": "yes",
            "layer": "-1"
        }))

if __name__ == '__main__':
    unittest.main()
