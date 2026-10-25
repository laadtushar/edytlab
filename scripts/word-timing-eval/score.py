#!/usr/bin/env python3
"""Score word times against a reference (#659).

    python3 score.py hyp.tsv reference.words.tsv

`hyp.tsv` is the output of `cargo run -p ml-whisper --example word_times`
(`# windows N`, `# seconds S`, then `start<TAB>end<TAB>word`); the reference is
`prepare.py`'s. Words are compared lower-cased with everything but letters,
digits and apostrophes removed. The two lists are aligned by edit distance
(substitution, insertion and deletion cost 1 each); `wrong` is that distance,
and the timing errors are taken over the words that lined up *and* are equal,
so a misheard word is counted once, as wrong, and not as a timing error.

Timing errors are |hypothesis - reference| in seconds for start and for end:
mean, median, 90th percentile, max. `bias` is the mean signed error
(negative: early), and `<=0.1 s` / `<=0.2 s` are the shares of matched words
whose start (or end) is within that of the reference; `>1 s` counts the
matched words that are more than a second out (a word matched to the wrong
place, or a long stretch the transcript skipped).
"""
import re
import sys


def norm(text):
    return re.sub(r"[^a-z0-9']", "", text.lower())


def read_hyp(path):
    meta, words = {}, []
    for line in open(path):
        line = line.rstrip("\n")
        if line.startswith("# "):
            key, _, value = line[2:].partition(" ")
            meta[key] = float(value)
        elif line:
            s, e, w = line.split("\t", 2)
            n = norm(w)
            if n:
                words.append((float(s), float(e), n))
    return meta, words


def read_ref(path):
    words = []
    for line in open(path):
        s, e, w = line.rstrip("\n").split("\t", 2)
        n = norm(w)
        if n:
            words.append((float(s), float(e), n))
    return words


def align(ref, hyp):
    """Levenshtein alignment; returns (distance, [(ref_index, hyp_index)] of equal pairs,
    (substitutions, deletions, insertions))."""
    n, m = len(ref), len(hyp)
    d = [[0] * (m + 1) for _ in range(n + 1)]
    for i in range(n + 1):
        d[i][0] = i
    for j in range(m + 1):
        d[0][j] = j
    for i in range(1, n + 1):
        for j in range(1, m + 1):
            sub = d[i - 1][j - 1] + (ref[i - 1][2] != hyp[j - 1][2])
            d[i][j] = min(sub, d[i - 1][j] + 1, d[i][j - 1] + 1)
    pairs, i, j = [], n, m
    subs = dels = inss = 0
    while i > 0 or j > 0:
        if i > 0 and j > 0 and d[i][j] == d[i - 1][j - 1] + (ref[i - 1][2] != hyp[j - 1][2]):
            if ref[i - 1][2] == hyp[j - 1][2]:
                pairs.append((i - 1, j - 1))
            else:
                subs += 1
            i, j = i - 1, j - 1
        elif i > 0 and d[i][j] == d[i - 1][j] + 1:
            dels += 1
            i -= 1
        else:
            inss += 1
            j -= 1
    pairs.reverse()
    return d[n][m], pairs, (subs, dels, inss)


def pct(values, q):
    s = sorted(values)
    if not s:
        return float("nan")
    k = (len(s) - 1) * q
    lo, hi = int(k), min(int(k) + 1, len(s) - 1)
    return s[lo] + (s[hi] - s[lo]) * (k - lo)


def stats(errors, signed):
    a = [abs(e) for e in errors]
    return {
        "mean": sum(a) / len(a),
        "median": pct(a, 0.5),
        "p90": pct(a, 0.9),
        "max": max(a),
        "bias": sum(signed) / len(signed),
        "le100": sum(x <= 0.1 for x in a) / len(a),
        "le200": sum(x <= 0.2 for x in a) / len(a),
        "gross": sum(x > 1.0 for x in a),
    }


def main():
    meta, hyp = read_hyp(sys.argv[1])
    ref = read_ref(sys.argv[2])
    wrong, pairs, (subs, dels, inss) = align(ref, hyp)
    starts = [hyp[j][0] - ref[i][0] for i, j in pairs]
    ends = [hyp[j][1] - ref[i][1] for i, j in pairs]
    ss, es = stats(starts, starts), stats(ends, ends)
    print(f"reference words {len(ref)}  hypothesis words {len(hyp)}  matched {len(pairs)}")
    print(
        f"wrong {wrong}  ({100 * wrong / len(ref):.1f}% of reference: "
        f"{subs} changed, {dels} missing, {inss} extra)"
    )
    print(f"windows {int(meta.get('windows', 0))}  seconds {meta.get('seconds', 0):.1f}")
    for name, s in (("start", ss), ("end", es)):
        print(
            f"{name:5}  mean {s['mean']:.3f}  median {s['median']:.3f}  p90 {s['p90']:.3f}  "
            f"max {s['max']:.3f}  bias {s['bias']:+.3f}  <=0.1s {100 * s['le100']:.0f}%  <=0.2s {100 * s['le200']:.0f}%  >1s {s['gross']}"
        )


if __name__ == "__main__":
    main()
