import unittest


class SampleTests(unittest.TestCase):
    def test_passes(self):
        self.assertEqual(2 + 2, 4)

    def test_fails(self):
        self.assertEqual("tapcue", "tap-cue")


if __name__ == "__main__":
    unittest.main()
