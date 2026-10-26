"""The reference the Rust port (`crates/ml-diarize`) is checked against.

It is the pipeline of pyannote.audio 3.1 (`speaker-diarization-3.1`: segmentation-3.0,
WeSpeaker ResNet34-LM, agglomerative clustering with centroid linkage), run on the two ONNX
files `models.json` pins. The functions marked VERBATIM are copied from pyannote.audio 3.3.2
(https://github.com/pyannote/pyannote-audio/tree/3.3.2/pyannote/audio) with only the torch
types taken out:

    The MIT License (MIT)
    Copyright (c) 2021- CNRS
    Permission is hereby granted, free of charge, to any person obtaining a copy of this
    software and associated documentation files (the "Software"), to deal in the Software
    without restriction, including without limitation the rights to use, copy, modify, merge,
    publish, distribute, sublicense, and/or sell copies of the Software ... (the full text is
    in https://github.com/pyannote/pyannote-audio/blob/3.3.2/LICENSE)

What is not verbatim: the glue that calls the models (onnxruntime), the Kaldi fbank
(kaldi-native-fbank, the C++ the Kaldi/torchaudio feature is defined by), and the embedding
masks, which follow `ONNXWeSpeakerPretrainedSpeakerEmbedding.__call__`.

Run by `make_fixtures.py`. Needs: numpy scipy soundfile onnxruntime kaldi-native-fbank
pyannote.core (all on PyPI). The versions the committed fixtures were made with are printed
in `fixtures.json`.
"""
import math
import os
from itertools import combinations

import numpy as np
import onnxruntime as ort
import soundfile as sf
import kaldi_native_fbank as knf
from pyannote.core import Annotation, Segment, SlidingWindow, SlidingWindowFeature
from scipy.cluster.hierarchy import fcluster, linkage
from scipy.spatial.distance import cdist

SR = 16000
DURATION = 10.0
STEP = 1.0  # 10% of the window, the pipeline's `segmentation_step`
# segmentation-3.0's receptive field: 991 samples wide, 270 apart
RF = SlidingWindow(start=0.0, duration=991 / 16000, step=270 / 16000)
THRESHOLD = 0.7045654963945799  # speaker-diarization-3.1 config.yaml
MIN_CLUSTER_SIZE = 12
EXCLUDE_OVERLAP = True

_sessions = {}


def _session(kind, path, threads):
    key = (kind, path)
    if key not in _sessions:
        so = ort.SessionOptions()
        so.intra_op_num_threads = threads
        so.inter_op_num_threads = 1
        _sessions[key] = ort.InferenceSession(path, so, providers=["CPUExecutionProvider"])
    return _sessions[key]


def seg_session(path):
    return _session("seg", path, 4)


def emb_session(path):
    return _session("emb", path, 1)


def powerset_mapping(num_classes=3, max_set_size=2):
    rows = []
    for k in range(0, max_set_size + 1):
        for s in combinations(range(num_classes), k):
            r = np.zeros(num_classes, np.float32); r[list(s)] = 1; rows.append(r)
    return np.stack(rows)
MAPPING = powerset_mapping()  # Powerset.build_mapping(3, 2) in numpy; `to_multilabel` is argmax then this

# ---- VERBATIM pyannote.audio.core.inference.Inference.aggregate
def aggregate(scores, frames, warm_up=(0.0, 0.0), epsilon=1e-12, hamming=False, missing=np.nan, skip_average=False):
    num_chunks, num_frames_per_chunk, num_classes = scores.data.shape
    chunks = scores.sliding_window
    frames = SlidingWindow(start=chunks.start, duration=frames.duration, step=frames.step)
    hamming_window = np.hamming(num_frames_per_chunk).reshape(-1, 1) if hamming else np.ones((num_frames_per_chunk, 1))
    warm_up_window = np.ones((num_frames_per_chunk, 1))
    warm_up_left = round(warm_up[0] / scores.sliding_window.duration * num_frames_per_chunk)
    warm_up_window[:warm_up_left] = epsilon
    warm_up_right = round(warm_up[1] / scores.sliding_window.duration * num_frames_per_chunk)
    warm_up_window[num_frames_per_chunk - warm_up_right :] = epsilon
    num_frames = (frames.closest_frame(scores.sliding_window.start + scores.sliding_window.duration + (num_chunks - 1) * scores.sliding_window.step + 0.5 * frames.duration) + 1)
    aggregated_output = np.zeros((num_frames, num_classes), dtype=np.float32)
    overlapping_chunk_count = np.zeros((num_frames, num_classes), dtype=np.float32)
    aggregated_mask = np.zeros((num_frames, num_classes), dtype=np.float32)
    for chunk, score in scores:
        mask = 1 - np.isnan(score)
        np.nan_to_num(score, copy=False, nan=0.0)
        start_frame = frames.closest_frame(chunk.start + 0.5 * frames.duration)
        aggregated_output[start_frame : start_frame + num_frames_per_chunk] += score * mask * hamming_window * warm_up_window
        overlapping_chunk_count[start_frame : start_frame + num_frames_per_chunk] += mask * hamming_window * warm_up_window
        aggregated_mask[start_frame : start_frame + num_frames_per_chunk] = np.maximum(aggregated_mask[start_frame : start_frame + num_frames_per_chunk], mask)
    if skip_average:
        average = aggregated_output
    else:
        average = aggregated_output / np.maximum(overlapping_chunk_count, epsilon)
    average[aggregated_mask == 0.0] = missing
    return SlidingWindowFeature(average, frames)

# ---- VERBATIM Inference.trim
def trim(scores, warm_up=(0.1, 0.1)):
    _, num_frames, _ = scores.data.shape
    chunks = scores.sliding_window
    num_frames_left = round(num_frames * warm_up[0])
    num_frames_right = round(num_frames * warm_up[1])
    new_data = scores.data[:, num_frames_left : num_frames - num_frames_right]
    new_chunks = SlidingWindow(start=chunks.start + warm_up[0] * chunks.duration, step=chunks.step, duration=(1 - warm_up[0] - warm_up[1]) * chunks.duration)
    return SlidingWindowFeature(new_data, new_chunks)

# ---- VERBATIM SpeakerDiarizationMixin.speaker_count
def speaker_count(binarized_segmentations, frames, warm_up=(0.1, 0.1)):
    trimmed = trim(binarized_segmentations, warm_up=warm_up)
    count = aggregate(np.sum(trimmed, axis=-1, keepdims=True), frames, hamming=False, missing=0.0, skip_average=False)
    count.data = np.rint(count.data).astype(np.uint8)
    return count

# ---- VERBATIM SpeakerDiarizationMixin.to_diarization
def to_diarization(segmentations, count):
    activations = aggregate(segmentations, count.sliding_window, hamming=False, missing=0.0, skip_average=True)
    _, num_speakers = activations.data.shape
    max_speakers_per_frame = np.max(count.data)
    if num_speakers < max_speakers_per_frame:
        activations.data = np.pad(activations.data, ((0, 0), (0, max_speakers_per_frame - num_speakers)))
    extent = activations.extent & count.extent
    activations = activations.crop(extent, return_data=False)
    count = count.crop(extent, return_data=False)
    sorted_speakers = np.argsort(-activations, axis=-1)
    binary = np.zeros_like(activations.data)
    for t, ((_, c), speakers) in enumerate(zip(count, sorted_speakers)):
        for i in range(c.item()):
            binary[t, speakers[i]] = 1.0
    return SlidingWindowFeature(binary, activations.sliding_window)

# ---- VERBATIM AgglomerativeClustering.cluster (threshold/method fixed to the 3.1 values)
def agglo_cluster(embeddings, min_clusters, max_clusters, num_clusters=None, threshold=0.7045654963945799, method="centroid", min_cluster_size_p=12, metric="cosine"):
    num_embeddings, _ = embeddings.shape
    min_cluster_size = min(min_cluster_size_p, max(1, round(0.1 * num_embeddings)))
    if num_embeddings == 1:
        return np.zeros((1,), dtype=np.uint8)
    if metric == "cosine" and method in ["centroid", "median", "ward"]:
        with np.errstate(divide="ignore", invalid="ignore"):
            embeddings = embeddings / np.linalg.norm(embeddings, axis=-1, keepdims=True)
        dendrogram = linkage(embeddings, method=method, metric="euclidean")
    else:
        dendrogram = linkage(embeddings, method=method, metric=metric)
    clusters = fcluster(dendrogram, threshold, criterion="distance") - 1
    cluster_unique, cluster_counts = np.unique(clusters, return_counts=True)
    large_clusters = cluster_unique[cluster_counts >= min_cluster_size]
    num_large_clusters = len(large_clusters)
    if num_large_clusters < min_clusters:
        num_clusters = min_clusters
    elif num_large_clusters > max_clusters:
        num_clusters = max_clusters
    if num_clusters is not None and num_large_clusters != num_clusters:
        _dendrogram = np.copy(dendrogram)
        _dendrogram[:, 2] = np.arange(num_embeddings - 1)
        best_iteration = num_embeddings - 1
        best_num_large_clusters = 1
        for iteration in np.argsort(np.abs(dendrogram[:, 2] - threshold)):
            new_cluster_size = _dendrogram[iteration, 3]
            if new_cluster_size < min_cluster_size:
                continue
            clusters = fcluster(_dendrogram, iteration, criterion="distance") - 1
            cluster_unique, cluster_counts = np.unique(clusters, return_counts=True)
            large_clusters = cluster_unique[cluster_counts >= min_cluster_size]
            num_large_clusters = len(large_clusters)
            if abs(num_large_clusters - num_clusters) < abs(best_num_large_clusters - num_clusters):
                best_iteration = iteration
                best_num_large_clusters = num_large_clusters
            if num_large_clusters == num_clusters:
                break
        if best_num_large_clusters != num_clusters:
            clusters = fcluster(_dendrogram, best_iteration, criterion="distance") - 1
            cluster_unique, cluster_counts = np.unique(clusters, return_counts=True)
            large_clusters = cluster_unique[cluster_counts >= min_cluster_size]
            num_large_clusters = len(large_clusters)
    if num_large_clusters == 0:
        clusters[:] = 0
        return clusters
    small_clusters = cluster_unique[cluster_counts < min_cluster_size]
    if len(small_clusters) == 0:
        return clusters
    large_centroids = np.vstack([np.mean(embeddings[clusters == k], axis=0) for k in large_clusters])
    small_centroids = np.vstack([np.mean(embeddings[clusters == k], axis=0) for k in small_clusters])
    centroids_cdist = cdist(large_centroids, small_centroids, metric=metric)
    for small_k, large_k in enumerate(np.argmin(centroids_cdist, axis=0)):
        clusters[clusters == small_clusters[small_k]] = large_clusters[large_k]
    _, clusters = np.unique(clusters, return_inverse=True)
    return clusters


def fbank_raw(wave):
    """Kaldi fbank as torchaudio.compliance.kaldi.fbank(wave * 32768, ...) in
    ONNXWeSpeakerPretrainedSpeakerEmbedding.compute_fbank, without the mean subtraction."""
    opts = knf.FbankOptions()
    opts.frame_opts.dither = 0.0
    opts.frame_opts.window_type = "hamming"
    opts.frame_opts.samp_freq = 16000
    opts.frame_opts.snip_edges = True
    opts.mel_opts.num_bins = 80
    opts.mel_opts.low_freq = 20.0
    opts.mel_opts.high_freq = 0.0
    opts.use_energy = False
    f = knf.OnlineFbank(opts)
    f.accept_waveform(16000, (np.asarray(wave, dtype=np.float32) * 32768.0).astype(np.float32))
    f.input_finished()
    return np.stack([f.get_frame(i) for i in range(f.num_frames_ready)]).astype(np.float32)


def fbank(wave):
    feats = fbank_raw(wave)
    return feats - feats.mean(axis=0, keepdims=True)


def min_num_samples(emb):
    """ONNXWeSpeakerPretrainedSpeakerEmbedding.min_num_samples: bisect for the shortest input
    whose embedding is not NaN."""
    lower, upper = 2, round(0.5 * SR)
    middle = (lower + upper) // 2
    rng = np.random.default_rng(0)
    while lower + 1 < upper:
        w = rng.standard_normal(middle).astype(np.float32)
        feats = fbank(w)
        if feats.shape[0] == 0:
            lower = middle
            middle = (lower + upper) // 2
            continue
        e = emb.run(None, {"feats": feats[None]})[0]
        if np.any(np.isnan(e)):
            lower = middle
        else:
            upper = middle
        middle = (lower + upper) // 2
    return upper


def read_audio(path):
    wave, sr = sf.read(path, dtype="float32")
    if wave.ndim > 1:
        wave = wave.mean(axis=1)
    assert sr == SR, sr
    return wave


def chunks_of(wave):
    n = len(wave)
    window = int(DURATION * SR)
    step = round(STEP * SR)
    out = []
    num_chunks = (n - window) // step + 1 if n >= window else 0
    for c in range(num_chunks):
        out.append(wave[c * step : c * step + window])
    if n < window or (n - window) % step > 0:
        last = wave[num_chunks * step :]
        out.append(np.pad(last, (0, window - len(last))))
    return out


def segment(seg, chunks):
    """(chunks, frames, 3) multilabel and the (chunks, frames, 7) logits."""
    mapping = powerset_mapping()
    multilabel, logits = [], []
    for ch in chunks:
        lg = seg.run(None, {"input_values": ch[None, None, :]})[0][0]
        logits.append(lg)
        multilabel.append(np.eye(7, dtype=np.float32)[np.argmax(lg, axis=-1)] @ mapping)
    return np.stack(multilabel), np.stack(logits)


def embed(emb, chunks, seg_ml, min_samples, min_frames):
    """Embeddings of every (chunk, local speaker): (chunks, 3, 256), NaN where there is none."""
    num_chunks, num_frames, num_spk = seg_ml.shape
    window = int(DURATION * SR)
    min_num_frames = math.ceil(num_frames * min_samples / window)
    clean = seg_ml * (1.0 * (np.sum(seg_ml, axis=2, keepdims=True) < 2))
    out = np.full((num_chunks, num_spk, 256), np.nan)
    for c in range(num_chunks):
        feats = fbank(chunks[c])
        nf = feats.shape[0]
        idx = (np.arange(nf) * num_frames / nf).astype(int)  # torch F.interpolate(mode="nearest")
        for s in range(num_spk):
            m, cm = seg_ml[c, :, s], clean[c, :, s]
            used = cm if (EXCLUDE_OVERLAP and np.sum(cm) > min_num_frames) else m
            if np.sum(m) == 0:
                continue
            masked = feats[used[idx] > 0.5]
            if masked.shape[0] < min_frames:
                continue
            out[c, s] = emb.run(None, {"feats": masked[None]})[0][0]
    return out


def cluster_all(embeddings, seg_ml, num_speakers=None, min_speakers=None, max_speakers=None):
    """BaseClustering.__call__ + assign_embeddings (unconstrained). Returns hard_clusters."""
    num_chunks, num_spk, _ = embeddings.shape
    active = np.sum(seg_ml, axis=1) > 0
    valid = ~np.any(np.isnan(embeddings), axis=2)
    ci, si = np.where(active * valid)
    train = embeddings[ci, si]
    ne = train.shape[0]
    mn = num_speakers or min_speakers or 1
    mx = num_speakers or max_speakers or ne
    mn = max(1, min(ne, mn))
    mx = max(1, min(ne, mx))
    nc = mn if mn == mx else None
    if mx < 2:
        return np.zeros((num_chunks, num_spk), dtype=np.int64), train, ci, si, np.zeros(ne, dtype=np.int64)
    tc = agglo_cluster(train.copy(), mn, mx, num_clusters=nc)
    k = int(tc.max()) + 1
    centroids = np.vstack([np.mean(train[tc == i], axis=0) for i in range(k)])
    flat = np.nan_to_num(embeddings.reshape(-1, 256), nan=0.0)
    soft = (2 - cdist(flat, centroids, metric="cosine")).reshape(num_chunks, num_spk, k)
    return np.argmax(soft, axis=2), train, ci, si, tc


def diarize(seg_ml, hard, max_speakers=np.inf):
    """SpeakerDiarization.apply from the clustering on: count, reconstruct, to_diarization,
    to_annotation. Returns (count, discrete, annotation)."""
    num_chunks, num_frames, _ = seg_ml.shape
    chunk_sw = SlidingWindow(start=0.0, duration=DURATION, step=STEP)
    segmentations = SlidingWindowFeature(seg_ml, chunk_sw)
    count = speaker_count(segmentations, RF, warm_up=(0.0, 0.0))
    count.data = np.minimum(count.data, max_speakers).astype(np.int8)
    hard = hard.copy()
    hard[np.sum(seg_ml, axis=1) == 0] = -2
    ncl = hard.max() + 1
    cs = np.nan * np.zeros((num_chunks, num_frames, ncl))
    for c, (cluster, (_, s)) in enumerate(zip(hard, segmentations)):
        for k in np.unique(cluster):
            if k == -2:
                continue
            cs[c, :, k] = np.max(s[:, cluster == k], axis=1)
    discrete = to_diarization(SlidingWindowFeature(cs, chunk_sw), count)
    return count, discrete, binarize(discrete)


def binarize(scores, onset=0.5, offset=0.5):
    """pyannote.audio.utils.signal.Binarize.__call__ (min_duration_on = min_duration_off = 0,
    which is the 3.1 pipeline's setting)."""
    num_frames, _ = scores.data.shape
    frames = scores.sliding_window
    timestamps = [frames[i].middle for i in range(num_frames)]
    active = Annotation()
    for k, k_scores in enumerate(scores.data.T):
        start = timestamps[0]
        is_active = k_scores[0] > onset
        for t, y in zip(timestamps[1:], k_scores[1:]):
            if is_active:
                if y < offset:
                    active[Segment(start, t), k] = k
                    start = t
                    is_active = False
            else:
                if y > onset:
                    start = t
                    is_active = True
        if is_active:
            active[Segment(start, t), k] = k
    return active
