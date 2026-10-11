//! The stores that are alive, by project folder (#501).
//!
//! A project's [`Store`] is not owned by one place. The window's
//! `AppState` holds it, the agent holds the one it was built with for the
//! length of a turn (it is rebuilt only once the turn ends, #498), and a
//! command or a tool can hold a clone for as long as it runs. Opening a
//! project used to build a new `Store` every time, so opening A, then B,
//! then A again during a turn on A left the turn on one `Store` and the
//! window on another: two writers on one folder, and an
//! unreferenced-file sweep on A that held a lock no tool staged under.
//!
//! This is a registry of the stores that are still alive: canonical folder
//! to [`Weak`] handle, the same idea as the in-flight counter in
//! `session::store`. Opening a folder asks it first, and a store that is
//! alive (held by anything at all) is the one the open gets. It is only a
//! view: a weak reference does not keep a store alive, so once the last
//! holder lets go the entry is dead and is pruned, and the next open builds
//! a store from the folder as it is then.
//!
//! ## Locking
//!
//! The registry's lock is a leaf. It is taken for a lookup or an
//! insertion and dropped before the caller does anything else: the
//! canonical path is worked out before it is taken, and no store, engine,
//! dispatcher or `AppState` lock is taken while it is held. Nothing a
//! caller locks afterwards can wait on it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use session::Store;

/// The live stores, one per project folder. Cloning shares the registry.
#[derive(Clone, Default)]
pub struct LiveStores {
    stores: Arc<Mutex<HashMap<PathBuf, Weak<Mutex<Store>>>>>,
}

/// The key a folder is known by: its canonical path (`std::fs::canonicalize`,
/// as for the in-flight count in `session::store`), so `a/../a` and a
/// symlink to `a` are one project. A folder that cannot be resolved is keyed
/// by the path as given; opening it creates it, so by the time a store is
/// registered it can.
///
/// What a canonical path does not do, so two keys can still be one folder:
///
/// - It does not fold case on Linux. On a disk that is case-insensitive
///   there (vfat, exFAT, CIFS, a casefolded directory) `Music` and `music`
///   are one folder under two keys. Whether another platform's
///   canonicalisation normalises case is that platform's business, and is
///   not relied on here.
/// - Two paths to one folder through a bind mount stay two keys.
///
/// Either way the outcome is the one this registry exists to prevent, two
/// live stores for a folder, and only for a user who opens one folder under
/// two such spellings while a turn holds the first. Keying by the folder's
/// identity (device and inode) would close both; it is the possible
/// follow-up, and is not done here because the in-flight count would then
/// want the same key.
fn key_of(dir: &Path) -> PathBuf {
    std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf())
}

/// Whether two spellings reach one folder, by the registry's own key.
pub(crate) fn same_folder(a: &Path, b: &Path) -> bool {
    key_of(a) == key_of(b)
}

/// Whether a store can still be locked. A tool that panicked with the lock
/// held poisons it for good, and every command on it then fails; such a
/// store is not one to hand to the next open, which builds a working one.
///
/// That only happens in a dev or test build. The release profile aborts on
/// panic (`[profile.release] panic = "abort"`), so a panicking tool takes
/// the process with it and no store is left poisoned.
fn usable(handle: &Arc<Mutex<Store>>) -> bool {
    !handle.is_poisoned()
}

impl LiveStores {
    /// The registry lock. Nothing under it can panic (it only edits the
    /// map), so a poisoned lock holds a map that is whole.
    fn map(&self) -> MutexGuard<'_, HashMap<PathBuf, Weak<Mutex<Store>>>> {
        self.stores.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The store alive for the project at `dir`, if there is one.
    ///
    /// Entries whose store is gone are dropped on the way.
    pub fn find(&self, dir: &Path) -> Option<Arc<Mutex<Store>>> {
        let key = key_of(dir);
        let mut stores = self.map();
        stores.retain(|_, store| store.strong_count() > 0);
        stores.get(&key).and_then(Weak::upgrade).filter(usable)
    }

    /// Record `handle` as the live store of the project at `dir`, unless
    /// one is already alive for it, and return whichever is.
    ///
    /// Two opens of a folder nothing has open can both build a store; the
    /// first to get here is the one that is registered and the other gets
    /// it back, so the caller compares with [`Arc::ptr_eq`] to see whether
    /// its own was taken. `dir` must exist by now (a [`Store::open`] makes
    /// it), or it is keyed by the spelling it was given.
    pub fn register(&self, dir: &Path, handle: Arc<Mutex<Store>>) -> Arc<Mutex<Store>> {
        let key = key_of(dir);
        let mut stores = self.map();
        stores.retain(|_, store| store.strong_count() > 0);
        if let Some(live) = stores.get(&key).and_then(Weak::upgrade).filter(usable) {
            return live;
        }
        stores.insert(key, Arc::downgrade(&handle));
        handle
    }

    /// How many entries the registry holds, dead or alive.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.map().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn store_at(dir: &Path) -> Arc<Mutex<Store>> {
        Arc::new(Mutex::new(Store::open(dir).unwrap()))
    }

    #[test]
    fn a_registered_store_is_found_by_any_spelling_of_its_folder() {
        let tmp = tempdir().unwrap();
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        std::fs::create_dir_all(&b).unwrap();
        let live = LiveStores::default();
        let held = live.register(&a, store_at(&a));

        assert!(Arc::ptr_eq(&live.find(&a).unwrap(), &held));
        assert!(Arc::ptr_eq(
            &live.find(&b.join("..").join("a")).unwrap(),
            &held
        ));
        assert!(live.find(&b).is_none(), "another folder");
    }

    #[test]
    fn a_second_store_for_a_live_folder_gets_the_first_back() {
        let tmp = tempdir().unwrap();
        let live = LiveStores::default();
        let first = live.register(tmp.path(), store_at(tmp.path()));
        let second = store_at(tmp.path());

        let got = live.register(tmp.path(), Arc::clone(&second));

        assert!(Arc::ptr_eq(&got, &first), "the first one stays registered");
        assert!(!Arc::ptr_eq(&got, &second));
    }

    #[test]
    fn the_registry_does_not_keep_a_store_alive_and_forgets_it() {
        let (a, b) = (tempdir().unwrap(), tempdir().unwrap());
        let live = LiveStores::default();
        let held = live.register(a.path(), store_at(a.path()));
        let weak = Arc::downgrade(&held);
        assert_eq!(live.len(), 1);

        drop(held);

        assert_eq!(weak.strong_count(), 0, "the registry owned the store");
        assert!(live.find(a.path()).is_none());
        assert_eq!(live.len(), 0, "the dead entry was not pruned");

        // Pruned by any later look, not only one for the same folder.
        let held = live.register(a.path(), store_at(a.path()));
        drop(held);
        assert!(live.find(b.path()).is_none());
        assert_eq!(live.len(), 0);
    }

    /// A store a tool panicked under cannot be locked again. It is not
    /// handed to the next open, which gets a working one in its place.
    #[test]
    fn a_poisoned_store_is_replaced_not_handed_out() {
        let tmp = tempdir().unwrap();
        let live = LiveStores::default();
        let broken = live.register(tmp.path(), store_at(tmp.path()));
        {
            let broken = Arc::clone(&broken);
            let _ = std::thread::spawn(move || {
                let _held = broken.lock().unwrap();
                panic!("a tool panicked with the store locked");
            })
            .join();
        }
        assert!(broken.is_poisoned());
        assert!(live.find(tmp.path()).is_none());

        let fresh = store_at(tmp.path());
        let got = live.register(tmp.path(), Arc::clone(&fresh));
        assert!(Arc::ptr_eq(&got, &fresh));
        assert!(Arc::ptr_eq(&live.find(tmp.path()).unwrap(), &fresh));
    }
}
