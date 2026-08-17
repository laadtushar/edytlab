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

use std::collections::{BTreeMap, BTreeSet, HashMap};
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
             audio file and none are ever deleted, so a long session grows without bound. Splits \
             the derived audio three ways: files the current head needs, files only older nodes \
             need (what undo is holding onto), and files no node references at all, plus what the \
             bounded preview cache is holding. Reads only — it deletes nothing.",
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

        let live = collect_refs(std::iter::once(&head_node));
        let any = collect_refs(all.iter());

        // Which history files could be rebuilt rather than kept.
        //
        // A node's audio is rebuildable when the node records the
        // operation that produced it *and* every node between it and a
        // root does too — replay starts from a state that still exists,
        // so one missing link upstream strands everything below it.
        // Nodes written before provenance existed have no record, which
        // reads as "not known to be rebuildable": the safe direction.
        let by_id: HashMap<_, _> = all.iter().map(|n| (n.id, n)).collect();
        let mut rebuildable_paths: BTreeSet<PathBuf> = BTreeSet::new();
        for node in &all {
            let mut cur = Some(node);
            let chain_is_replayable = loop {
                let Some(n) = cur else { break true }; // reached a root
                match &n.op {
                    Some(op) if op.reproducible => {
                        cur = n.parent.and_then(|p| by_id.get(&p).copied())
                    }
                    _ => break false,
                }
            };
            if chain_is_replayable {
                for track in &node.state.tracks {
                    for clip in &track.clips {
                        rebuildable_paths.insert(key(&clip.source_path));
                    }
                }
            }
        }

        // Walk every `derived/` directory the graph knows about and
        // classify what is actually there. Files are the unit rather
        // than nodes: two nodes naming the same content-addressed file
        // is the common case, and counting it twice would overstate
        // what a sweep could free.
        let mut live_bytes = 0u64;
        let mut history_bytes = 0u64;
        let mut unref_bytes = 0u64;
        let (mut live_n, mut history_n, mut unref_n) = (0usize, 0usize, 0usize);
        // A subset of `history`: the part a sweep could reclaim and put
        // back, rather than merely delete.
        let mut rebuildable_bytes = 0u64;
        let mut rebuildable_n = 0usize;
        let mut unreferenced: Vec<(PathBuf, u64)> = Vec::new();

        for dir in derived_dirs(&all) {
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

        let total = live_bytes + history_bytes + unref_bytes;
        let mut by_category = BTreeMap::new();
        by_category.insert("live", (live_n, live_bytes));
        by_category.insert("history", (history_n, history_bytes));
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
            "unreferenced": { "files": unref_n, "bytes": unref_bytes },
            "preview_cache": {
                "files": preview_files,
                "bytes": preview_bytes,
                "dir": cache.dir().display().to_string(),
                "cap_bytes": crate::preview_cache::DEFAULT_CAP_BYTES,
            },
            "largest_unreferenced": sample,
            "summary": format!(
                "{:.1} MiB of derived audio across {} node{}: {:.1} MiB the current version \
                 needs, {:.1} MiB held only by undo history ({} file{}), {:.1} MiB referenced \
                 by nothing ({} file{}). Separately, {:.1} MiB of rendered previews ({} \
                 file{}) sit in a bounded cache that evicts itself. Nothing was deleted here — \
                 there is no reclamation policy for derived audio yet.",
                mib(total),
                all.len(),
                if all.len() == 1 { "" } else { "s" },
                mib(live_bytes),
                mib(history_bytes),
                history_n,
                if history_n == 1 { "" } else { "s" },
                mib(unref_bytes),
                unref_n,
                if unref_n == 1 { "" } else { "s" },
                mib(preview_bytes),
                preview_files,
                if preview_files == 1 { "" } else { "s" },
            ),
        })))
    }
}
