//! What the session is costing on disk, and how much of that nothing
//! points at (#98).
//!
//! Every destructive edit writes a content-addressed WAV under a
//! `derived/` directory beside its source, and nothing ever deletes one.
//! A five-minute stereo 48 kHz track is roughly 55 MB per edit, so fifty
//! edits is about 2.7 GB of files that will never be opened again.
//!
//! #98 asks for a reclamation policy, and says — correctly — not to
//! implement one before the policy question is answered, because the
//! code is easy and the wrong policy silently loses work. It also asks
//! for this first: something that measures, so a fix can be shown to
//! work rather than asserted to. This tool is only that. It deletes
//! nothing and it is the honest input to the decision, not a substitute
//! for it.
//!
//! ## What "unreferenced" means here, and what it does not
//!
//! Undo means every node is reachable by design — that is the whole
//! point of the DAG — so "unreachable from the head" is not the same as
//! "safe to delete", and a naive mark-and-sweep would free nothing. The
//! report therefore counts three separate things:
//!
//! * **live** — referenced by the current head. Never removable under
//!   any policy.
//! * **history** — referenced by some other node but not the head. This
//!   is the number the decision turns on: it is what undo is holding
//!   onto, and what options (b) and (c) in #98 would reclaim in
//!   different ways.
//! * **unreferenced** — in `derived/` and named by no node at all. Files
//!   from an interrupted edit, or from a node that was never written.
//!   The only bytes any policy could reclaim without an argument.
//!
//! Reporting them apart is the point. Collapsing them into one "reclaim
//! me" number is exactly the mistake that would make a sweep look safe.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::{Tool, ToolContext, ToolResult};

/// A file's size, or zero if it cannot be read. A stat that fails is
/// not worth failing a read-only report over.
fn size_of(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// Canonical form for comparing a path a node names against a path found
/// on disk. Falls back to the path as given when the file is gone —
/// a node may reference something already deleted by hand, and that
/// should not make it match a different file.
fn key(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Every `derived/` directory named by any clip in any node.
///
/// Derived files live beside their source rather than in one place, so
/// there is no single directory to scan — the set has to be discovered
/// from the graph.
fn derived_dirs(all: &[session::SessionNode]) -> BTreeSet<PathBuf> {
    let mut dirs = BTreeSet::new();
    for node in all {
        for track in &node.state.tracks {
            for clip in &track.clips {
                if let Some(parent) = clip.source_path.parent() {
                    if parent.file_name().and_then(|s| s.to_str()) == Some("derived") {
                        dirs.insert(parent.to_path_buf());
                    }
                }
            }
        }
    }
    dirs
}

/// Every path the given nodes' clips point at, canonicalised.
fn collect_refs<'a>(nodes: impl Iterator<Item = &'a session::SessionNode>) -> BTreeSet<PathBuf> {
    let mut out = BTreeSet::new();
    for node in nodes {
        for track in &node.state.tracks {
            for clip in &track.clips {
                out.insert(key(&clip.source_path));
            }
        }
    }
    out
}

fn mib(bytes: u64) -> f64 {
    (bytes as f64) / (1024.0 * 1024.0)
}

pub struct StorageReportTool;

impl Tool for StorageReportTool {
    fn name(&self) -> &'static str {
        "storage_report"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "storage_report",
            "Report what this session is costing on disk. Every destructive edit writes a new \
             audio file. Splits the derived audio three ways: files the current head needs, files \
             only older nodes need (what undo is holding onto), and files no node references at \
             all, plus what the bounded preview cache is holding. Reads only — it deletes nothing. \
             Audio no node references is removed when the project is opened, and once a \
             project's derived audio passes 2 GiB, audio only undo history holds is swept \
             automatically in the background, oldest first — but only files that replaying their \
             edits rebuilds, and undoing back to one rebuilds it. Audio nothing can rebuild is \
             never swept: `compact_session` reclaims that, at the cost of dropping undo history \
             permanently.",
            json!({ "type": "object", "properties": {}, "required": [] }),
        )
    }

    fn invoke(&self, _args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let head = match ctx.store.head() {
            Some(h) => h,
            None => {
                return Ok(ToolResult::Error(
                    "no session loaded; call `load` first".to_string(),
                ))
            }
        };
        let all = match ctx.store.list_nodes() {
            Ok(n) => n,
            Err(e) => return Ok(ToolResult::Error(format!("failed to list nodes: {e}"))),
        };
        let head_node = match ctx.store.get(head) {
            Ok(n) => n,
            Err(e) => return Ok(ToolResult::Error(format!("failed to read head node: {e}"))),
        };

        // The head's lane copies — the flattened `track-<hash>.wav` files
        // the timeline is showing — are the head's audio as much as its
        // clips' sources are. No clip names them, so without this they
        // were counted as referenced by nothing (#374). The set is the
        // sweep's own, so the report and the sweep cannot disagree.
        let project_dir = ctx.store.project_dir().to_path_buf();
        let mut live = collect_refs(std::iter::once(&head_node));
        live.extend(crate::reclaim::lane_copies(
            &project_dir,
            std::slice::from_ref(&head_node),
        ));
        let any = collect_refs(all.iter());
        let lanes = crate::reclaim::lane_copies(&project_dir, &all);

        // Which history files could be rebuilt rather than kept: the
        // sweep's own pre-filter, so the report and the sweep agree.
        let rebuildable_paths = crate::reclaim::rebuildable_paths(&all);

        // Walk every `derived/` directory the graph knows about and
        // classify what is actually there. Files are the unit rather
        // than nodes: two nodes naming the same content-addressed file
        // is the common case, and counting it twice would overstate
        // what a sweep could free.
        let mut live_bytes = 0u64;
        let mut history_bytes = 0u64;
        let mut unref_bytes = 0u64;
        let (mut live_n, mut history_n, mut unref_n) = (0usize, 0usize, 0usize);
        // Lane copies of clip lists only older nodes hold: a cache the
        // timeline writes again from its sources when it shows that
        // version, and the first thing the sweep removes over its cap.
        let (mut lane_n, mut lane_bytes) = (0usize, 0u64);
        // A subset of `history`: the part a sweep could reclaim and put
        // back, rather than merely delete.
        let mut rebuildable_bytes = 0u64;
        let mut rebuildable_n = 0usize;
        let mut unreferenced: Vec<(PathBuf, u64)> = Vec::new();

        // The project's own `derived/` always, even when no clip names a
        // file in it: lane copies and orphans live there too, and a report
        // that only looked where clips point would miss them.
        // By canonical path, so one directory spelled two ways (a
        // symlinked temp dir) is not counted twice.
        let mut dirs: BTreeSet<PathBuf> = derived_dirs(&all).iter().map(|d| key(d)).collect();
        dirs.insert(key(&crate::provenance::derived_dir(&project_dir)));
        for dir in dirs {
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                // A directory the graph names but that is no longer
                // there is not an error for a report to raise.
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let bytes = size_of(&path);
                let k = key(&path);
                if live.contains(&k) {
                    live_bytes += bytes;
                    live_n += 1;
                } else if any.contains(&k) {
                    history_bytes += bytes;
                    history_n += 1;
                    if rebuildable_paths.contains(&k) {
                        rebuildable_bytes += bytes;
                        rebuildable_n += 1;
                    }
                } else if lanes.contains(&k) {
                    lane_bytes += bytes;
                    lane_n += 1;
                } else {
                    unref_bytes += bytes;
                    unref_n += 1;
                    unreferenced.push((path, bytes));
                }
            }
        }

        // Largest first: if this list is ever shown to a person, the
        // first few lines are the ones worth their attention.
        unreferenced.sort_by(|a, b| b.1.cmp(&a.1));
        let sample: Vec<Value> = unreferenced
            .iter()
            .take(10)
            .map(|(p, b)| json!({ "path": p.display().to_string(), "bytes": b }))
            .collect();

        // Rendered previews (#164) are a fourth, separate thing: not
        // edit history at all, and the one category that is *designed*
        // to be thrown away — every entry is re-derivable byte-for-byte
        // from the node it is named for. Reported apart so the number a
        // person sees for "history" is not inflated by a cache.
        let cache = crate::PreviewCache::new(ctx.store.project_dir());
        let preview_files = cache.len();
        let preview_bytes = cache.size_bytes();

        // Clipboard blobs (#163) are a fifth thing, and the one category
        // that must never be swept: a pasted region's audio exists
        // nowhere else once the derived file it went into is gone.
        let (clip_files, clip_bytes) = {
            let dir = crate::provenance::clipboard_dir(ctx.store.project_dir());
            match std::fs::read_dir(&dir) {
                Ok(entries) => {
                    let mut n = 0usize;
                    let mut bytes = 0u64;
                    for e in entries.flatten() {
                        if e.path().is_file() {
                            n += 1;
                            bytes += size_of(&e.path());
                        }
                    }
                    (n, bytes)
                }
                Err(_) => (0, 0),
            }
        };

        let total = live_bytes + history_bytes + lane_bytes + unref_bytes;
        let mut by_category = BTreeMap::new();
        by_category.insert("live", (live_n, live_bytes));
        by_category.insert("history", (history_n, history_bytes));
        by_category.insert("lane_copies", (lane_n, lane_bytes));
        by_category.insert("unreferenced", (unref_n, unref_bytes));

        Ok(ToolResult::Ok(json!({
            "node_count": all.len(),
            "total_bytes": total,
            "total_mib": (mib(total) * 100.0).round() / 100.0,
            "live": { "files": live_n, "bytes": live_bytes },
            "history": {
                "files": history_n,
                "bytes": history_bytes,
                "rebuildable_files": rebuildable_n,
                "rebuildable_bytes": rebuildable_bytes,
            },
            "lane_copies": { "files": lane_n, "bytes": lane_bytes },
            "unreferenced": { "files": unref_n, "bytes": unref_bytes },
            "derived_cap_bytes": crate::reclaim::DEFAULT_DERIVED_CAP_BYTES,
            "preview_cache": {
                "files": preview_files,
                "bytes": preview_bytes,
                "dir": cache.dir().display().to_string(),
                "cap_bytes": crate::preview_cache::DEFAULT_CAP_BYTES,
            },
            "clipboard_blobs": { "files": clip_files, "bytes": clip_bytes },
            "largest_unreferenced": sample,
            "summary": format!(
                "{:.1} MiB of derived audio across {} node{}: {:.1} MiB the current version \
                 needs, {:.1} MiB held only by undo history ({} file{}), {:.1} MiB of timeline \
                 copies of older versions ({} file{}, rewritten from their sources when shown), \
                 {:.1} MiB referenced by nothing ({} file{}). Separately, {:.1} MiB of rendered previews ({} \
                 file{}) sit in a bounded cache that evicts itself. Nothing was deleted here. \
                 Audio referenced by nothing is removed when the project is next opened. Past \
                 {:.0} MiB of derived audio, what undo history holds is swept automatically, \
                 oldest first — only files a replay of their edits rebuilds, and undoing back \
                 to one rebuilds it. What cannot be rebuilt is kept; `compact_session` \
                 reclaims it by dropping undo history permanently.",
                mib(total),
                all.len(),
                if all.len() == 1 { "" } else { "s" },
                mib(live_bytes),
                mib(history_bytes),
                history_n,
                if history_n == 1 { "" } else { "s" },
                mib(lane_bytes),
                lane_n,
                if lane_n == 1 { "" } else { "s" },
                mib(unref_bytes),
                unref_n,
                if unref_n == 1 { "" } else { "s" },
                mib(preview_bytes),
                preview_files,
                if preview_files == 1 { "" } else { "s" },
                mib(crate::reclaim::DEFAULT_DERIVED_CAP_BYTES),
            ),
        })))
    }
}
