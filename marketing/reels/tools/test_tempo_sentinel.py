"""The cue file records clip_tempo only when the clip was tempo-processed (exact 1.0 means a plain copy)."""
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock


TOOLS = Path(__file__).resolve().parent
PROJECT = TOOLS.parent
with mock.patch.dict(os.environ, {"EDYTLAB_REEL_ROOT": str(PROJECT)}), \
        mock.patch.object(sys, "path", [str(TOOLS), *sys.path]):
    import build


def cue(cue_id):
    return {"id": cue_id, "start": 0.0, "end": 2.0, "kind": "step", "onscreen": "On screen",
            "narration": "Say it", "narration_spoken": "Say it", "words": 2, "max_words": 5}


class TempoSentinelTests(unittest.TestCase):
    def test_cue_file_records_clip_tempo_only_off_one(self):
        timing = {c: {"clip": f"{c}.wav", "speech_end": 1.0, "tempo": t}
                  for c, t in (("a", 1.0), ("b", 1.25), ("c", 1.0001))}
        tl = {"file": "reel", "duration": 4.0, "cues": [cue("a"), cue("b"), cue("c")]}
        with tempfile.TemporaryDirectory(dir=PROJECT) as folder:
            path = Path(folder) / "cues.json"
            build.write_cues(tl, timing, str(path))
            written = {c["id"]: c for c in json.loads(path.read_text())["cues"]}
        self.assertNotIn("clip_tempo", written["a"])
        self.assertEqual(written["b"]["clip_tempo"], 1.25)
        self.assertEqual(written["c"]["clip_tempo"], 1.0001)


if __name__ == "__main__":
    unittest.main()
