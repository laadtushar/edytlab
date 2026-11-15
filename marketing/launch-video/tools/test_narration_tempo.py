"""Narration gets an atempo filter only when its tempo is not exactly 1.0."""
import importlib.util
import sys
import unittest
from pathlib import Path
from unittest import mock

TOOLS = Path(__file__).resolve().parent


def load_audio():
    """Import the actual mixer; decoding and placing are replaced by the fixture."""
    spec = importlib.util.spec_from_file_location('launch_audio_tempo_fixture', TOOLS / 'audio.py')
    module = importlib.util.module_from_spec(spec)
    with mock.patch.object(sys, 'path', list(sys.path)):
        spec.loader.exec_module(module)
    return module


class NarrationTempo(unittest.TestCase):
    """mix_pcm and the exact 1.0 tempo sentinel."""

    @staticmethod
    def filters(narration):
        """Mix with no clips or sounds and return the filter given for each narration item."""
        audio = load_audio()
        seen = []
        cues = {'clips': [], 'sfx': [], 'total': 1.0}
        with mock.patch.object(audio, '_checked_media_path'), \
                mock.patch.object(audio, 'decode', side_effect=lambda args, filt: seen.append(filt) or b''), \
                mock.patch.object(audio, 'place'):
            audio.mix_pcm(cues, narration, 'sfx', False, -20, 0)
        return seen

    def test_exact_one_or_missing_tempo_adds_no_filter(self):
        """Tempo 1.0, or none, decodes without atempo."""
        seen = self.filters([{'t': 0, 'file': 'a.mp3', 'tempo': 1.0}, {'t': 0, 'file': 'b.mp3'}])
        self.assertEqual([f.startswith('atempo=') for f in seen], [False, False])

    def test_any_other_tempo_adds_the_filter(self):
        """Any other tempo, even 1.0001, decodes through atempo."""
        seen = self.filters([
            {'t': 0, 'file': 'a.mp3', 'tempo': 1.25},
            {'t': 0, 'file': 'b.mp3', 'tempo': 1.0001},
        ])
        self.assertTrue(seen[0].startswith('atempo=1.25,'))
        self.assertTrue(seen[1].startswith('atempo=1.0001,'))


if __name__ == '__main__':
    unittest.main()
