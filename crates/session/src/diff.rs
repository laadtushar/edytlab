//! Branching primitives. Phase 2 fills these in; Phase 1 ships the names so
//! the public surface and on-disk schema stay forward-compatible.

use crate::node::{NodeId, SessionNode};
use crate::store::Store;
use crate::Result;

#[allow(dead_code)]
pub fn fork(_store: &mut Store, _from: NodeId) -> Result<NodeId> {
    unimplemented!("fork is a Phase 2 operation")
}

#[allow(dead_code)]
pub fn diff(_store: &Store, _a: NodeId, _b: NodeId) -> Result<SessionDiff> {
    unimplemented!("diff is a Phase 2 operation")
}

#[allow(dead_code)]
pub fn compare(_store: &Store, _a: NodeId, _b: NodeId) -> Result<SessionComparison> {
    unimplemented!("compare is a Phase 2 operation")
}

#[allow(dead_code)]
pub fn merge(_store: &mut Store, _a: NodeId, _b: NodeId) -> Result<SessionNode> {
    unimplemented!("merge is a Phase 2 operation")
}

// Placeholder result types so call sites in later milestones can compile
// against the eventual shape without us locking semantics today.
#[allow(dead_code)]
pub struct SessionDiff;

#[allow(dead_code)]
pub struct SessionComparison;
