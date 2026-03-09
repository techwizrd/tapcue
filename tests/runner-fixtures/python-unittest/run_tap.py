import os
import unittest

from pycotap import TAPTestRunner


def main() -> int:
    suite = unittest.defaultTestLoader.discover(os.path.dirname(__file__), pattern="test_*.py")
    result = TAPTestRunner().run(suite)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main())
