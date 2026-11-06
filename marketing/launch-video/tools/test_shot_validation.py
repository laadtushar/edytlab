"""Shot timing validation keeps its diagnostics with and without optimization."""
import json
from pathlib import Path
import subprocess
import sys
import unittest


TOOLS = Path(__file__).resolve().parent
PROBE = r'''
import json
import sys
sys.path.insert(0, sys.argv[1])
import launch
launch.caption_window = lambda *_: (0, 10)
launch.spec.DEMOS = {'BT': {'audio': {'sound': (2, 4)}}}
cases = [
    (-1, 1, False),  # Outside the caption.
    (0, 1, True),    # Listen starts too early.
    (2, 3, True),    # Listen extends past the sound.
    (2, 1, False),   # Silent shot overlaps the sound.
    (0, 1, False),   # Valid silent shot.
    (1.7, 2.3, True),  # Valid Listen shot at the permitted lead boundary.
]
results = []
for start, duration, listen in cases:
    try:
        launch.check_shot(launch.BT, start, duration, [1], listen)
    except AssertionError as error:
        results.append(error.args)
    else:
        results.append(None)
print(json.dumps(results))
'''


class ShotValidation(unittest.TestCase):
    """Optimization must not permit shots that break the documented timing rules."""

    def test_optimized_and_normal_validation_keep_diagnostics(self):
        """All four refusals and both valid boundaries behave identically under -O."""
        results = []
        for flags in ([], ['-O']):
            result = subprocess.run(
                [sys.executable, '-I', *flags, '-c', PROBE, str(TOOLS)],
                capture_output=True, text=True, check=True,
            )
            results.append(json.loads(result.stdout))
        self.assertEqual(results[0], results[1])
        self.assertEqual(
            [entry[0][2] for entry in results[0][:4]],
            ['outside its caption',
             "a Listen shot must start at most 0.3 s before the app's sound",
             'must end inside the sound',
             "the app's sound would leak into a silent shot"],
        )
        self.assertEqual(results[0][4:], [None, None])


if __name__ == '__main__':
    unittest.main()
