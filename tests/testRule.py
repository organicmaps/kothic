import unittest
import sys
from pathlib import Path

# Add `src` directory to the import paths
sys.path.insert(0, str(Path(__file__).parent.parent / 'src'))

from mapcss.Rule import Rule
from mapcss.Condition import Condition
from mapcss import parseCondition

class RuleTest(unittest.TestCase):
    def test_rule(self):
        rule = Rule()
        rule.conditions = [
            parseCondition("aeroway=aerodrome"),
            parseCondition("aerodrome=international"),
            Condition("eq", ("::class", "::int_name"))
        ]

        tt = rule.test({
            "aeroway": "aerodrome",
            "aerodrome": "international"
        })
        self.assertTrue(tt)
        self.assertEqual(tt, "::int_name")

if __name__ == '__main__':
    unittest.main()
