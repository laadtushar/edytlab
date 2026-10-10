#!/usr/bin/env python3
"""mksrt.py cues.json out.srt -> SRT of the on-screen text (titles, list screens, end card), so the words that are burned in can also be uploaded."""
import json, sys
def ts(x):
    h, r = divmod(x, 3600); m, s = divmod(r, 60)
    return f"{int(h):02d}:{int(m):02d}:{int(s):02d},{int(round((s - int(s)) * 1000)):03d}"
cues = json.load(open(sys.argv[1]))["captions"]
cues.sort()
cues = [list(c) for c in cues]
for i in range(len(cues) - 1):
    if cues[i][1] > cues[i + 1][0] - 0.03:
        cues[i][1] = max(cues[i][0] + 0.4, cues[i + 1][0] - 0.03)
with open(sys.argv[2], "w") as f:
    for i, (a, b, t) in enumerate(cues, 1):
        f.write(f"{i}\n{ts(a)} --> {ts(max(a + 0.8, b))}\n{t}\n\n")
print(sys.argv[2], len(cues), "cues")
