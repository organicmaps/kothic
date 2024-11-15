import unittest
import sys
from pathlib import Path

# Add `src` directory to the import paths
sys.path.insert(0, str(Path(__file__).parent.parent / 'src'))

class RuleTest(unittest.TestCase):
    def test_upper(self):
        self.assertEqual('foo'.upper(), 'FOO')

if __name__ == '__main__':
    unittest.main()
