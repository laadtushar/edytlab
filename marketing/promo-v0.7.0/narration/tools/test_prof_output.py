"""Keep the profiler's integer text contract across scalar round protocols."""
import contextlib
import io
import runpy
import sys
import unittest
from unittest import mock

import numpy as np
import prof


class FloatingRoundScalar(float):
    """Model a numeric scalar whose round method returns a floating value."""

    def __rmul__(self, other):
        """Keep the scalar protocol through the profiler's decibel conversion."""
        return type(self)(other * float(self))

    def __round__(self, ndigits=None):
        """Delegate rounding while retaining a floating result for this fixture."""
        return float(round(float(self), ndigits))


class ProfilerOutput(unittest.TestCase):
    """Exercise CLI formatting with decoded samples and different round results."""

    def test_levels_are_integer_text_for_integer_and_floating_round_results(self):
        """Preserve timestamp truncation and dB rounding, including half-to-even."""
        self.assertIsInstance(round(20 * FloatingRoundScalar(-1.525)), float)
        samples = np.zeros((144000, 2), dtype=np.float32).tobytes()
        for scalar in (float, np.float64, FloatingRoundScalar):
            with self.subTest(scalar=scalar.__name__):
                stdout = io.StringIO()
                arguments = [prof.__file__, str(prof.ROOT / 'synthetic.wav'), '0.75']
                levels = [scalar(value) for value in (-1.525, -1.475, 0.625, -0.625)]
                with mock.patch.object(sys, 'argv', arguments), \
                        mock.patch('subprocess.run', return_value=mock.Mock(stdout=samples)), \
                        mock.patch.object(np, 'log10', side_effect=levels), \
                        contextlib.redirect_stdout(stdout):
                    runpy.run_path(prof.__file__, run_name='__main__')
                self.assertEqual(stdout.getvalue(), '0:-30 0:-30 1:12 2:-12\n')


if __name__ == '__main__':
    unittest.main()
