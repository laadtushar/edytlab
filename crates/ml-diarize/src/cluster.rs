//! Which of the speaker embeddings are the same person.
//!
//! A port of `AgglomerativeClustering` and `BaseClustering` from pyannote.audio
//! 3.3.2 (`pipelines/clustering.py`) with the settings of
//! `speaker-diarization-3.1` (centroid linkage on unit-length embeddings, a
//! distance threshold of 0.7045654963945799 and clusters of fewer than 12
//! embeddings folded into the nearest large one), and of the two SciPy
//! routines it calls (`scipy.cluster.hierarchy.linkage` with
//! `method="centroid"` and `fcluster` with `criterion="distance"`, from
//! `_hierarchy.pyx`, whose merge order and cluster numbering are copied
//! because the pipeline's tie-breaks depend on the numbers).
//!
//! ## What it does
//!
//! 1. Every embedding of a speaker that really speaks in a window is a point.
//!    Points are made unit length, so that Euclidean distance between them is
//!    a monotone function of cosine distance, which is what centroid linkage
//!    needs.
//! 2. [`linkage`] merges the two closest clusters until one is left, and the
//!    merges are cut at the threshold ([`cut`]). That is the unconstrained
//!    answer: as many people as the audio separates at that distance.
//! 3. If the caller knows how many people there are (`num_speakers`) or bounds
//!    them (`min_speakers`, `max_speakers`) and the cut gave a number outside
//!    that, the cut is moved along the merges to the one that gives the
//!    nearest number of large clusters.
//! 4. Small clusters (fewer than 12 embeddings, fewer for short audio) are
//!    noise as often as people; each goes to the large cluster whose centre
//!    is nearest.
//! 5. Every embedding, the ones that were clustered and those that were not,
//!    is then given the cluster whose centre (the mean of the raw embeddings
//!    clustered into it) is nearest in cosine distance ([`assign`]).
//!
//! Above [`MAX_CLUSTERED`] embeddings (roughly 17 to 38 minutes of audio at the
//! rates measured on real recordings), only an evenly spaced sample of them is
//! clustered, because the work grows with the square of their number; the rest
//! are assigned in step 5. pyannote.audio 3.1 never samples; its older
//! `max_num_embeddings` option did, at random.
//!
//! The fixtures are `scipy` and `pyannote.audio` run on 53 real embeddings
//! (`tests/fixtures/make_fixtures.py`); the tests below compare every merge,
//! every cut and every assignment.

/// The cut of the dendrogram: `speaker-diarization-3.1`'s `clustering.threshold`.
pub const THRESHOLD: f64 = 0.7045654963945799;
/// `clustering.min_cluster_size`.
pub const MIN_CLUSTER_SIZE: usize = 12;
/// The most embeddings that are clustered; more than this are sampled.
pub const MAX_CLUSTERED: usize = 3_000;

/// One merge of the dendrogram: the two clusters (numbers below `n` are single
/// embeddings, `n + i` is the result of merge `i`), how far apart they were,
/// and how many embeddings the new cluster holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Merge {
    pub left: usize,
    pub right: usize,
    pub distance: f64,
    pub size: usize,
}

/// `squareform` index of `(i, j)`, `i < j`, among `n`.
fn condensed(n: usize, i: usize, j: usize) -> usize {
    debug_assert!(i < j && j < n);
    n * i - i * (i + 1) / 2 + (j - i - 1)
}

/// `scipy.cluster.hierarchy.linkage(points, method="centroid", metric="euclidean")`.
///
/// `points` is `n` rows of `dim` values. Centroid linkage can make a merge
/// closer than an earlier one; the merges are in the order they were made.
pub fn linkage(points: &[f64], dim: usize) -> Vec<Merge> {
    let n = if dim == 0 { 0 } else { points.len() / dim };
    if n < 2 {
        return Vec::new();
    }
    let mut dist = vec![0.0f64; n * (n - 1) / 2];
    for i in 0..n {
        let a = &points[i * dim..(i + 1) * dim];
        for j in i + 1..n {
            let b = &points[j * dim..(j + 1) * dim];
            let squared: f64 = a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum();
            dist[condensed(n, i, j)] = squared.sqrt();
        }
    }
    let at = |a: usize, b: usize| condensed(n, a.min(b), a.max(b));

    let mut size = vec![1usize; n];
    let mut id: Vec<usize> = (0..n).collect();
    // For each cluster, a candidate for its nearest neighbour among the
    // clusters after it, and a lower bound of the distance to the true one
    // ("the generic algorithm", Mullner 2011, which is what SciPy runs).
    let mut neighbor = vec![usize::MAX; n - 1];
    let mut bound = vec![f64::INFINITY; n - 1];
    let nearest = |dist: &[f64], size: &[usize], x: usize| -> (usize, f64) {
        let (mut best, mut best_d) = (usize::MAX, f64::INFINITY);
        for y in x + 1..n {
            if size[y] != 0 && dist[condensed(n, x, y)] < best_d {
                best = y;
                best_d = dist[condensed(n, x, y)];
            }
        }
        (best, best_d)
    };
    for x in 0..n - 1 {
        (neighbor[x], bound[x]) = nearest(&dist, &size, x);
    }

    let mut merges = Vec::with_capacity(n - 1);
    for k in 0..n - 1 {
        // The cluster with the smallest bound; if the bound was out of date,
        // look again and retry.
        let (x, y, d) = loop {
            let mut x = usize::MAX;
            let mut smallest = f64::INFINITY;
            for (i, &b) in bound.iter().enumerate() {
                if size[i] != 0 && b < smallest {
                    x = i;
                    smallest = b;
                }
            }
            if x == usize::MAX {
                // Points that are not numbers: take the first two there are.
                let mut alive = (0..n).filter(|&i| size[i] != 0);
                let (a, b) = (alive.next().unwrap_or(0), alive.next().unwrap_or(1));
                break (a, b, dist[condensed(n, a, b)]);
            }
            let y = neighbor[x];
            if y != usize::MAX && size[y] != 0 && smallest == dist[condensed(n, x, y)] {
                break (x, y, smallest);
            }
            (neighbor[x], bound[x]) = nearest(&dist, &size, x);
        };
        bound[x] = f64::INFINITY;

        let (nx, ny) = (size[x], size[y]);
        let (id_x, id_y) = (id[x].min(id[y]), id[x].max(id[y]));
        merges.push(Merge {
            left: id_x,
            right: id_y,
            distance: d,
            size: nx + ny,
        });
        size[x] = 0;
        size[y] = nx + ny;
        id[y] = n + k;

        // Distances to the new cluster (kept at `y`), by the centroid rule on
        // plain distances.
        let (fx, fy) = (nx as f64, ny as f64);
        for z in 0..n {
            if size[z] == 0 || z == y {
                continue;
            }
            let (d_xi, d_yi) = (dist[at(z, x)], dist[at(z, y)]);
            let squared =
                ((fx * d_xi * d_xi + fy * d_yi * d_yi) - (fx * fy * d * d) / (fx + fy)) / (fx + fy);
            dist[at(z, y)] = squared.max(0.0).sqrt();
        }
        for z in 0..x {
            if size[z] > 0 && neighbor[z] == x {
                neighbor[z] = y;
            }
        }
        for z in 0..y {
            if size[z] != 0 && dist[condensed(n, z, y)] < bound[z] {
                neighbor[z] = y;
                bound[z] = dist[condensed(n, z, y)];
            }
        }
        if y < n - 1 {
            let (z, nd) = nearest(&dist, &size, y);
            if z != usize::MAX {
                neighbor[y] = z;
                bound[y] = nd;
            } else {
                bound[y] = f64::INFINITY;
            }
        }
    }
    merges
}

/// `fcluster(Z, cutoff, criterion="distance") - 1`: a number from 0 for every
/// one of the `n` embeddings, such that each cluster is a subtree of the
/// dendrogram whose merges are all at `cutoff` or closer. Numbered in the
/// order SciPy numbers them (`cluster_monocrit`), which the callers' tie-breaks
/// follow.
pub fn cut(merges: &[Merge], n: usize, cutoff: f64) -> Vec<usize> {
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![0];
    }
    // The greatest distance anywhere below each merge (`get_max_dist_for_each_cluster`):
    // centroid linkage is not monotone.
    let mut deepest = vec![0.0f64; merges.len()];
    let mut stack: Vec<usize> = vec![2 * n - 2];
    let mut visited = vec![false; 2 * n - 1];
    while let Some(&node) = stack.last() {
        let root = node - n;
        let (l, r) = (merges[root].left, merges[root].right);
        if l >= n && !visited[l] {
            visited[l] = true;
            stack.push(l);
            continue;
        }
        if r >= n && !visited[r] {
            visited[r] = true;
            stack.push(r);
            continue;
        }
        let mut most = merges[root].distance;
        for child in [l, r] {
            if child >= n {
                most = most.max(deepest[child - n]);
            }
        }
        deepest[root] = most;
        stack.pop();
    }

    let mut labels = vec![0usize; n];
    let mut visited = vec![false; 2 * n - 1];
    let (mut found, mut leader) = (0usize, usize::MAX);
    let mut stack: Vec<usize> = vec![2 * n - 2];
    while let Some(&node) = stack.last() {
        let root = node - n;
        let (l, r) = (merges[root].left, merges[root].right);
        if leader == usize::MAX && deepest[root] <= cutoff {
            leader = root;
            found += 1;
        }
        if l >= n && !visited[l] {
            visited[l] = true;
            stack.push(l);
            continue;
        }
        if r >= n && !visited[r] {
            visited[r] = true;
            stack.push(r);
            continue;
        }
        for child in [l, r] {
            if child < n {
                if leader == usize::MAX {
                    found += 1; // a cluster of one
                }
                labels[child] = found - 1;
            }
        }
        if leader == root {
            leader = usize::MAX;
        }
        stack.pop();
    }
    labels
}

/// Distinct values of `labels` in ascending order, with how many each has.
fn tally(labels: &[usize]) -> Vec<(usize, usize)> {
    let mut counts: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
    for &l in labels {
        *counts.entry(l).or_default() += 1;
    }
    counts.into_iter().collect()
}

fn unit(points: &[f32], dim: usize) -> Vec<f64> {
    let mut out: Vec<f64> = points.iter().map(|&v| f64::from(v)).collect();
    for row in out.chunks_exact_mut(dim) {
        let norm = row.iter().map(|v| v * v).sum::<f64>().sqrt();
        for v in row {
            *v /= norm;
        }
    }
    out
}

fn mean_of(points: &[f64], dim: usize, labels: &[usize], label: usize) -> Vec<f64> {
    let mut sum = vec![0.0f64; dim];
    let mut count = 0usize;
    for (row, &l) in points.chunks_exact(dim).zip(labels) {
        if l == label {
            count += 1;
            for (s, v) in sum.iter_mut().zip(row) {
                *s += v;
            }
        }
    }
    for s in &mut sum {
        *s /= count as f64;
    }
    sum
}

/// `scipy.spatial.distance.cdist(a, b, "cosine")` for one pair.
fn cosine_distance(a: &[f64], b: &[f64]) -> f64 {
    let (mut dot, mut na, mut nb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    1.0 - dot / (na.sqrt() * nb.sqrt())
}

/// How many speakers to look for: `num_speakers` fixes it, otherwise
/// `min_speakers` and `max_speakers` bound it (`set_num_speakers`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeakerBounds {
    pub min: usize,
    /// `None`: as many as the audio separates.
    pub max: Option<usize>,
}

impl Default for SpeakerBounds {
    /// At least one, and no more than the audio separates.
    fn default() -> Self {
        Self { min: 1, max: None }
    }
}

/// Why the bounds make no sense.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BoundsError {
    #[error("min_speakers ({min}) is more than max_speakers ({max})")]
    MinOverMax { min: usize, max: usize },
}

impl SpeakerBounds {
    /// From the three options a caller can give; `num_speakers` wins over the
    /// other two. A zero is no bound, as in pyannote (`num_speakers or ...`).
    pub fn new(
        num_speakers: Option<usize>,
        min_speakers: Option<usize>,
        max_speakers: Option<usize>,
    ) -> Result<Self, BoundsError> {
        let nonzero = |v: Option<usize>| v.filter(|&v| v > 0);
        let min = nonzero(num_speakers).or(nonzero(min_speakers)).unwrap_or(1);
        let max = nonzero(num_speakers).or(nonzero(max_speakers));
        if let Some(max) = max {
            if min > max {
                return Err(BoundsError::MinOverMax { min, max });
            }
        }
        Ok(Self { min, max })
    }

    /// The cap on speakers speaking at once, if any.
    pub fn cap(&self) -> Option<usize> {
        self.max
    }
}

/// `AgglomerativeClustering.cluster`: a cluster number from 0 for each of the
/// `n` embeddings (`n` rows of `dim` values), with between `min_clusters` and
/// `max_clusters` large clusters when the audio allows, or exactly
/// `num_clusters` large ones when that is given.
///
/// `threshold` and `min_cluster_size` are [`THRESHOLD`] and [`MIN_CLUSTER_SIZE`]
/// in the pipeline; they are arguments so the tests can reach every branch.
pub fn agglomerate(
    points: &[f32],
    dim: usize,
    min_clusters: usize,
    max_clusters: usize,
    num_clusters: Option<usize>,
    threshold: f64,
    min_cluster_size: usize,
) -> Vec<usize> {
    let n = points.len() / dim;
    // Fewer embeddings need fewer to make a cluster that counts. Python's
    // `round` goes half to even.
    let min_cluster_size =
        min_cluster_size.min(((0.1 * n as f64).round_ties_even() as usize).max(1));
    if n <= 1 {
        return vec![0; n];
    }
    let points = unit(points, dim);
    let merges = linkage(&points, dim);
    let mut labels = cut(&merges, n, threshold);
    let mut tallied = tally(&labels);
    let mut large = tallied
        .iter()
        .filter(|(_, c)| *c >= min_cluster_size)
        .count();

    let mut wanted = num_clusters;
    if large < min_clusters {
        wanted = Some(min_clusters);
    } else if large > max_clusters {
        wanted = Some(max_clusters);
    }
    if let Some(wanted) = wanted.filter(|&w| w != large) {
        // Cut by merge number rather than by distance, and walk from the merge
        // nearest the threshold outwards until the number of large clusters is
        // the one wanted, or as near to it as any cut gets.
        let by_index: Vec<Merge> = merges
            .iter()
            .enumerate()
            .map(|(i, m)| Merge {
                distance: i as f64,
                ..*m
            })
            .collect();
        let mut order: Vec<usize> = (0..merges.len()).collect();
        order.sort_by(|a, b| {
            (merges[*a].distance - threshold)
                .abs()
                .total_cmp(&(merges[*b].distance - threshold).abs())
        });
        let (mut best, mut best_large) = (n - 1, 1usize);
        for &iteration in &order {
            if merges[iteration].size < min_cluster_size {
                continue;
            }
            labels = cut(&by_index, n, iteration as f64);
            tallied = tally(&labels);
            large = tallied
                .iter()
                .filter(|(_, c)| *c >= min_cluster_size)
                .count();
            if large.abs_diff(wanted) < best_large.abs_diff(wanted) {
                best = iteration;
                best_large = large;
            }
            if large == wanted {
                break;
            }
        }
        if best_large != wanted {
            labels = cut(&by_index, n, best as f64);
            tallied = tally(&labels);
            large = tallied
                .iter()
                .filter(|(_, c)| *c >= min_cluster_size)
                .count();
        }
    }

    if large == 0 {
        return vec![0; n];
    }
    let small: Vec<usize> = tallied
        .iter()
        .filter(|(_, c)| *c < min_cluster_size)
        .map(|(l, _)| *l)
        .collect();
    if small.is_empty() {
        return labels;
    }
    let big: Vec<usize> = tallied
        .iter()
        .filter(|(_, c)| *c >= min_cluster_size)
        .map(|(l, _)| *l)
        .collect();
    let big_centres: Vec<Vec<f64>> = big
        .iter()
        .map(|&l| mean_of(&points, dim, &labels, l))
        .collect();
    let moves: Vec<(usize, usize)> = small
        .iter()
        .map(|&s| {
            let centre = mean_of(&points, dim, &labels, s);
            let mut best = (0usize, f64::INFINITY);
            for (i, c) in big_centres.iter().enumerate() {
                let d = cosine_distance(c, &centre);
                if d < best.1 {
                    best = (i, d);
                }
            }
            (s, big[best.0])
        })
        .collect();
    for l in &mut labels {
        if let Some((_, to)) = moves.iter().find(|(from, _)| from == l) {
            *l = *to;
        }
    }
    // Numbered again from 0, in the order of the old numbers.
    let mut distinct: Vec<usize> = labels.clone();
    distinct.sort_unstable();
    distinct.dedup();
    labels
        .iter()
        .map(|l| distinct.binary_search(l).unwrap_or(0))
        .collect()
}

/// The clustering's answer for every (window, local speaker) of a recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clustered {
    /// Per (window, speaker), window first: the cluster whose centre is
    /// nearest. A speaker without an embedding gets cluster 0, as in pyannote
    /// (an all-`NaN` row's `argmax` is 0); the caller leaves out the ones that
    /// do not speak.
    pub clusters: Vec<usize>,
    /// How many clusters there are.
    pub count: usize,
}

/// `BaseClustering.__call__` and `assign_embeddings`, unconstrained.
///
/// `embeddings` is `rows` rows of `dim` values (a row per window and local
/// speaker, window first); `usable[r]` says whether row `r` is the embedding
/// of a speaker that speaks in its window and has one.
pub fn cluster_embeddings(
    embeddings: &[f32],
    dim: usize,
    usable: &[bool],
    bounds: SpeakerBounds,
) -> Clustered {
    let rows = usable.len();
    let mut train: Vec<usize> = (0..rows).filter(|&r| usable[r]).collect();
    if train.len() > MAX_CLUSTERED {
        let total = train.len();
        train = (0..MAX_CLUSTERED)
            .map(|i| train[i * total / MAX_CLUSTERED])
            .collect();
    }
    let n = train.len();
    if n == 0 {
        return Clustered {
            clusters: vec![0; rows],
            count: 1,
        };
    }
    // `set_num_clusters`
    let min = bounds.min.clamp(1, n);
    let max = bounds.max.unwrap_or(n).clamp(1, n);
    let num = (min == max).then_some(min);
    if max < 2 {
        return Clustered {
            clusters: vec![0; rows],
            count: 1,
        };
    }

    let mut points = Vec::with_capacity(n * dim);
    for &r in &train {
        points.extend_from_slice(&embeddings[r * dim..(r + 1) * dim]);
    }
    let labels = agglomerate(&points, dim, min, max, num, THRESHOLD, MIN_CLUSTER_SIZE);
    let count = labels.iter().max().map_or(1, |m| m + 1);

    // The centre of a cluster is the mean of its raw (not unit-length)
    // embeddings.
    let raw: Vec<f64> = points.iter().map(|&v| f64::from(v)).collect();
    let centres: Vec<Vec<f64>> = (0..count).map(|k| mean_of(&raw, dim, &labels, k)).collect();
    let clusters = (0..rows)
        .map(|r| {
            let row: Vec<f64> = embeddings[r * dim..(r + 1) * dim]
                .iter()
                .map(|&v| f64::from(v))
                .collect();
            let mut best = (0usize, f64::NEG_INFINITY);
            for (k, centre) in centres.iter().enumerate() {
                let similarity = 2.0 - cosine_distance(&row, centre);
                if similarity.is_nan() {
                    return 0;
                }
                if similarity > best.1 {
                    best = (k, similarity);
                }
            }
            best.0
        })
        .collect();
    Clustered { clusters, count }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::tests::{case, fixture_segmentation};

    const DIM: usize = 256;

    fn floats(bytes: &[u8]) -> Vec<f32> {
        bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect()
    }

    fn doubles(bytes: &[u8]) -> Vec<f64> {
        bytes
            .chunks_exact(8)
            .map(|b| f64::from_le_bytes(b.try_into().unwrap()))
            .collect()
    }

    /// The embeddings of the recording, a row per (window, speaker), and which
    /// rows are of a speaker that speaks and has one.
    fn embeddings() -> (Vec<f32>, Vec<bool>) {
        let all = floats(include_bytes!("../tests/fixtures/embeddings.f32"));
        let seg = fixture_segmentation();
        let usable = (0..seg.chunks * seg.speakers)
            .map(|r| {
                seg.active(r / seg.speakers, r % seg.speakers)
                    && !all[r * DIM..(r + 1) * DIM].iter().any(|v| v.is_nan())
            })
            .collect();
        (all, usable)
    }

    fn train() -> Vec<f32> {
        let (all, usable) = embeddings();
        (0..usable.len())
            .filter(|&r| usable[r])
            .flat_map(|r| all[r * DIM..(r + 1) * DIM].to_vec())
            .collect()
    }

    fn bounds_of(case: &serde_json::Value) -> SpeakerBounds {
        let get = |k: &str| case["args"][k].as_u64().map(|v| v as usize);
        SpeakerBounds::new(
            get("num_speakers"),
            get("min_speakers"),
            get("max_speakers"),
        )
        .unwrap()
    }

    #[test]
    fn every_merge_of_the_dendrogram_is_scipys() {
        let reference = doubles(include_bytes!("../tests/fixtures/linkage.f64"));
        let points = unit(&train(), DIM);
        let merges = linkage(&points, DIM);
        assert_eq!(merges.len() * 4, reference.len());
        for (i, (m, z)) in merges.iter().zip(reference.chunks_exact(4)).enumerate() {
            assert_eq!((m.left as f64, m.right as f64), (z[0], z[1]), "merge {i}");
            assert_eq!(m.size as f64, z[3], "merge {i}");
            assert!(
                (m.distance - z[2]).abs() < 1e-9,
                "merge {i}: {} vs {}",
                m.distance,
                z[2]
            );
        }
        // Centroid linkage is not monotone, and these embeddings show it, so
        // the cut's "greatest distance below" matters.
        assert!(
            merges.windows(2).any(|w| w[1].distance < w[0].distance),
            "the fixture should contain an inversion, or the test proves less than it says"
        );
    }

    #[test]
    fn every_setting_gives_the_clusters_pyannote_gives() {
        let (all, usable) = embeddings();
        let train_rows = usable.iter().filter(|u| **u).count();
        assert_eq!(train_rows, 53);
        for name in ["free", "num2", "num3", "num1", "min3", "max2"] {
            let c = case(name);
            let got = cluster_embeddings(&all, DIM, &usable, bounds_of(&c));
            let want: Vec<usize> = c["hard"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|row| {
                    row.as_array()
                        .unwrap()
                        .iter()
                        .map(|k| k.as_u64().unwrap() as usize)
                })
                .collect();
            assert_eq!(got.clusters, want, "{name}");
            assert_eq!(got.count as u64, c["clusters"].as_u64().unwrap(), "{name}");
        }
    }

    #[test]
    fn the_clustered_embeddings_are_numbered_as_scipy_numbers_them() {
        let points = train();
        for name in ["free", "num2", "num3", "min3", "max2"] {
            let c = case(name);
            let b = bounds_of(&c);
            let n = points.len() / DIM;
            let min = b.min.clamp(1, n);
            let max = b.max.unwrap_or(n).clamp(1, n);
            let got = agglomerate(
                &points,
                DIM,
                min,
                max,
                (min == max).then_some(min),
                THRESHOLD,
                MIN_CLUSTER_SIZE,
            );
            let want: Vec<usize> = c["train_clusters"]
                .as_array()
                .unwrap()
                .iter()
                .map(|k| k.as_u64().unwrap() as usize)
                .collect();
            assert_eq!(got, want, "{name}");
        }
    }

    /// Three well separated groups of 20 and one stray point.
    fn blobs() -> Vec<f32> {
        let mut out = Vec::new();
        for g in 0..3 {
            for i in 0..20 {
                let mut v = vec![0.0f32; 8];
                v[g] = 1.0;
                v[3 + (i % 5)] = 0.02 * i as f32;
                out.extend(v);
            }
        }
        let mut stray = vec![0.0f32; 8];
        stray[0] = 1.0;
        stray[7] = 0.9;
        out.extend(stray);
        out
    }

    #[test]
    fn well_separated_groups_are_found_and_a_stray_joins_the_nearest() {
        let points = blobs();
        let labels = agglomerate(&points, 8, 1, 61, None, 0.7, 12);
        assert_eq!(*labels.iter().max().unwrap(), 2, "{labels:?}");
        for g in 0..3 {
            assert!(labels[g * 20..(g + 1) * 20]
                .iter()
                .all(|l| *l == labels[g * 20]));
        }
        assert_eq!(
            labels[60], labels[0],
            "the stray is nearest the first group"
        );
    }

    #[test]
    fn a_wanted_number_moves_the_cut_along_the_merges() {
        let points = blobs();
        for wanted in [1usize, 2, 3] {
            let labels = agglomerate(&points, 8, wanted, wanted, Some(wanted), 0.7, 12);
            assert_eq!(*labels.iter().max().unwrap() + 1, wanted, "{labels:?}");
        }
        // More than the audio holds: as many as there are large clusters to find.
        let labels = agglomerate(&points, 8, 5, 5, Some(5), 0.7, 12);
        assert!(*labels.iter().max().unwrap() + 1 >= 3);
    }

    #[test]
    fn one_embedding_is_one_cluster_and_none_is_none() {
        assert_eq!(agglomerate(&[1.0, 0.0], 2, 1, 1, None, 0.7, 12), vec![0]);
        assert!(agglomerate(&[], 2, 1, 1, None, 0.7, 12).is_empty());
        let c = cluster_embeddings(&[0.0; 4], 2, &[false, false], SpeakerBounds::default());
        assert_eq!(
            c,
            Clustered {
                clusters: vec![0, 0],
                count: 1
            }
        );
    }

    #[test]
    fn bounds_follow_pyannote_set_num_speakers() {
        let b = SpeakerBounds::new(Some(2), Some(5), Some(9)).unwrap();
        assert_eq!((b.min, b.max), (2, Some(2)), "num_speakers wins");
        let b = SpeakerBounds::new(None, Some(2), None).unwrap();
        assert_eq!((b.min, b.max), (2, None));
        let b = SpeakerBounds::new(None, None, None).unwrap();
        assert_eq!((b.min, b.max), (1, None));
        assert_eq!(
            SpeakerBounds::new(None, Some(4), Some(2)),
            Err(BoundsError::MinOverMax { min: 4, max: 2 })
        );
        assert_eq!(
            SpeakerBounds::new(Some(0), Some(0), Some(0)).unwrap(),
            SpeakerBounds::default()
        );
    }

    #[test]
    fn a_very_long_recording_is_clustered_on_a_sample_and_assigned_whole() {
        // 4,000 embeddings of two directions: more than are clustered.
        let mut data = Vec::new();
        for i in 0..4_000 {
            let mut v = [0.0f32; 4];
            v[i % 2] = 1.0;
            v[2 + (i % 7) / 4] = 0.01 * (i % 5) as f32;
            data.extend(v);
        }
        let usable = vec![true; 4_000];
        let c = cluster_embeddings(&data, 4, &usable, SpeakerBounds::default());
        assert_eq!(c.count, 2);
        assert!((0..4_000).all(|i| c.clusters[i] == c.clusters[i % 2]));
        assert_ne!(c.clusters[0], c.clusters[1]);
    }
}
