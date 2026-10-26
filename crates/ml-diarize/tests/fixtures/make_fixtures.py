"""Writes the fixtures the ml-diarize tests compare the Rust port with, from `reference.py`.

    python3 -I make_fixtures.py <segmentation.onnx> <embedding.onnx>

where the two files are the ones `crates/ml-pipeline/models.json` pins
(`scripts/fetch-ml-model.sh speaker-diarization <dir>` fetches them, as
`<dir>/segmentation/model.onnx` and `<dir>/embedding/model.onnx`). It reads
`ami_es2004a_640_680s.wav` here and writes, next to this file:

  fixtures.json             versions, parameters, shapes, the embedding's minimum input
  fbank_ref.f32             Kaldi fbank (before mean subtraction) of samples 8000..56000
  segmentation.u8           (chunks, 589, 3) multilabel segmentation, the pipeline's input to
                            counting and embedding
  embeddings.f32            (chunks, 3, 256), NaN where a speaker has no embedding
  linkage.f64               scipy's linkage matrix of the unit-normalised train embeddings
  count.u8, discrete.u8     speaker count per frame, and the 0/1 diarization per frame and cluster,
                            for the unconstrained clustering
  cases.json                for each speaker-count setting: the cluster of every (chunk, speaker),
                            and the turns (start, end, cluster) pyannote's Binarize makes
"""
import json
import os
import sys

import numpy as np
import onnxruntime
import scipy

here = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, here)  # `-I` leaves it out
import reference as R  # noqa: E402

seg_path, emb_path = sys.argv[1], sys.argv[2]
seg, emb = R.seg_session(seg_path), R.emb_session(emb_path)

wave = R.read_audio(os.path.join(here, "ami_es2004a_640_680s.wav"))
chunks = R.chunks_of(wave)
min_samples = R.min_num_samples(emb)
min_frames = R.fbank(np.random.default_rng(1).standard_normal(min_samples).astype(np.float32)).shape[0]

seg_ml, _ = R.segment(seg, chunks)
embeddings = R.embed(emb, chunks, seg_ml, min_samples, min_frames)

# the fbank unit
lo, hi = 8000, 56000
fb = R.fbank_raw(wave[lo:hi])
fb.astype("<f4").tofile(os.path.join(here, "fbank_ref.f32"))
seg_ml.astype("u1").tofile(os.path.join(here, "segmentation.u8"))
embeddings.astype("<f4").tofile(os.path.join(here, "embeddings.f32"))

# the linkage of the train embeddings, unit-normalised as `cluster` does
active = np.sum(seg_ml, axis=1) > 0
valid = ~np.any(np.isnan(embeddings), axis=2)
ci, si = np.where(active * valid)
train = embeddings[ci, si]
unit = train / np.linalg.norm(train, axis=-1, keepdims=True)
Z = R.linkage(unit, method="centroid", metric="euclidean")
Z.astype("<f8").tofile(os.path.join(here, "linkage.f64"))

cases = []
for name, kw in [
    ("free", {}),
    ("num2", {"num_speakers": 2}),
    ("num3", {"num_speakers": 3}),
    ("num1", {"num_speakers": 1}),
    ("min3", {"min_speakers": 3}),
    ("max2", {"max_speakers": 2}),
]:
    hard, _, _, _, tc = R.cluster_all(embeddings, seg_ml, **kw)
    cap = kw.get("num_speakers") or kw.get("max_speakers") or np.inf
    count, discrete, ann = R.diarize(seg_ml, hard, max_speakers=cap)
    turns = [
        [float(s.start), float(s.end), int(label)]
        for s, _, label in ann.itertracks(yield_label=True)
    ]
    turns.sort(key=lambda t: (t[0], t[2]))
    cases.append(
        {
            "name": name,
            "args": kw,
            "train_clusters": [int(x) for x in tc],
            "hard": [[int(x) for x in row] for row in hard],
            "turns": turns,
            "frames": int(discrete.data.shape[0]),
            "clusters": int(discrete.data.shape[1]),
        }
    )
    if name == "free":
        count.data.astype("u1").tofile(os.path.join(here, "count.u8"))
        discrete.data.astype("u1").tofile(os.path.join(here, "discrete.u8"))

json.dump(
    {
        "versions": {
            "numpy": np.__version__,
            "scipy": scipy.__version__,
            "onnxruntime": onnxruntime.__version__,
        },
        "pipeline": {
            "threshold": R.THRESHOLD,
            "min_cluster_size": R.MIN_CLUSTER_SIZE,
            "exclude_overlap": R.EXCLUDE_OVERLAP,
            "segmentation_step_s": R.STEP,
        },
        "min_num_samples": int(min_samples),
        "min_num_frames": int(min_frames),
        "chunks": len(chunks),
        "segmentation_frames": int(seg_ml.shape[1]),
        "fbank": {"from": lo, "to": hi, "frames": int(fb.shape[0])},
        "train_embeddings": int(train.shape[0]),
        "cases": cases,
    },
    open(os.path.join(here, "fixtures.json"), "w"),
    indent=1,
)
print("wrote", len(cases), "cases;", train.shape[0], "train embeddings;", len(chunks), "chunks")
