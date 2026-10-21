//! Which project folders have a live [`Store`], by the folder itself (#501,
//! #534, #566).
//!
//! A project's [`Store`] is not owned by one place. The window's `AppState`
//! holds it, the agent holds the one it was built with for the length of a
//! turn, and a command or a tool can hold a clone for as long as it runs.
//! Two `Store`s on one folder are two writers, each with its own in-memory
//! head over one `.audiograph/head`: the second one's next edit writes a
//! head that skips what the first one appended. So whether a folder has a
//! live store is decided here, once, for every way of opening one, and not
//! by each caller.
//!
//! ## The folder is its identity, not its path
//!
//! Folders are told apart by [`FolderId`]: the device and inode on unix, the
//! volume serial number and file index on Windows. A path, even a canonical
//! one, is a spelling. On a case-insensitive disk (vfat, exFAT, CIFS, a
//! casefolded directory) `Music` and `music` are one folder, and two paths to
//! one folder through a bind mount stay two canonical paths. Both used to be
//! two keys, and so two live stores.
//!
//! An identity only holds while the folder is there to name: once it is
//! deleted, its inode can be given to the next folder made. So the first
//! [`Store`] opened on a folder keeps a handle to it open (the *pin*), shared
//! by every other store on the folder through [`Folder`], and the folder's
//! identity cannot be reused while any of them lives. The pin closes with the
//! last store, not with the registry's entry, so nothing stays open after the
//! project is let go.
//!
//! ## Ways to open a store
//!
//! | Open | Registry | A live store for the folder |
//! |------|----------|-----------------------------|
//! | [`Store::open_shared`] | finds or registers the one live store | handed back, shared |
//! | [`Store::open_private`] | registers a claim for as long as the store lives | [`Error::AlreadyOpen`] |
//! | [`Store::open_read_only`] | not registered | no matter: it cannot write |
//! | [`Store::open`] | not registered | no matter, and no protection: for a folder only the caller can reach |
//!
//! A shared store is `Arc<Mutex<Store>>` ([`LiveStore`]); everything that
//! holds the open project's store holds that one. A private store is for
//! code that writes to a folder of its own for a while (`batch_apply`'s
//! per-file projects) and must not do it to a folder anything else has open.
//! It holds a claim on the folder, so the folder cannot be opened shared
//! while it lives either, however the two are timed.
//!
//! Only this module makes a [`LiveStore`], so a type that asks for one
//! (`AppState`'s open project) cannot be given a store that did not come
//! through the registry.
//!
//! ## Locking
//!
//! The registry's lock is a leaf. It is taken for a lookup or an insertion
//! and dropped before the caller does anything else: the identity is worked
//! out (it touches the disk) before the lock is taken, a store is built
//! after it is dropped, and no store, engine, dispatcher or `AppState` lock
//! is taken while it is held. The only thing asked of a store under it is
//! [`Mutex::is_poisoned`], which does not lock.
//!
//! The registry is only a view. It holds [`Weak`] references, so it does not
//! keep a store alive: once the last holder lets go the entry is dead and is
//! pruned, and the next open builds a store from the folder as it is then.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::ops::Deref;
use std::path::Path;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};

use crate::store::Store;
use crate::{Error, Result};

/// What a folder is, whatever it is called: its device and inode (unix), or
/// its volume serial number and file index (Windows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FolderId {
    volume: u64,
    index: u64,
}

impl FolderId {
    /// An identity from its two halves, for an [`Identify`] test double.
    /// The real one comes from [`identify`].
    pub const fn new(volume: u64, index: u64) -> Self {
        Self { volume, index }
    }
}

/// An open handle on the folder, kept so its identity cannot be given to
/// another folder while a store is on it (see the module documentation).
#[cfg(unix)]
type Pin = fs::File;
#[cfg(windows)]
type Pin = winapi_util::Handle;

/// A folder's identity and, for the real one, the handle that pins it.
pub struct Identity {
    id: FolderId,
    pin: Option<Pin>,
}

impl Identity {
    /// Which folder this is.
    pub fn id(&self) -> FolderId {
        self.id
    }

    /// An identity with nothing to pin, for a test double.
    pub const fn unpinned(id: FolderId) -> Self {
        Self { id, pin: None }
    }
}

/// How a registry tells folders apart. A function pointer so a test can make
/// two paths one folder without a bind mount (see [`Registry::with_identity`]).
pub type Identify = fn(&Path) -> io::Result<Identity>;

/// The identity of the folder at `dir`: the real [`Identify`]. Errors when
/// `dir` cannot be opened for reading, which includes it not existing.
#[cfg(unix)]
pub fn identify(dir: &Path) -> io::Result<Identity> {
    use std::os::unix::fs::MetadataExt;
    let pin = fs::File::open(dir)?;
    let meta = pin.metadata()?;
    Ok(Identity {
        id: FolderId::new(meta.dev(), meta.ino()),
        pin: Some(pin),
    })
}

/// The identity of the folder at `dir`: the real [`Identify`]. Errors when
/// `dir` cannot be opened for reading, which includes it not existing.
#[cfg(windows)]
pub fn identify(dir: &Path) -> io::Result<Identity> {
    let pin = winapi_util::Handle::from_path_any(dir)?;
    let info = winapi_util::file::information(&pin)?;
    Ok(Identity {
        id: FolderId::new(info.volume_serial_number(), info.file_index()),
        pin: Some(pin),
    })
}

/// What every [`Store`] open on one folder in this process shares.
pub(crate) struct Folder {
    pub(crate) id: FolderId,
    /// How this folder was identified, so a store can tell whether a
    /// spelling still reaches it ([`Store::reread`]).
    pub(crate) identify: Identify,
    /// How many staged handles on the folder are alive, from any `Store`
    /// opened on it. See [`Store::staged_in_flight`].
    pub(crate) in_flight: AtomicUsize,
    _pin: Option<Pin>,
}

/// Held by a private store for as long as it lives: the registry's record
/// that the folder has a writer of its own.
pub(crate) struct Claim;

/// What stands in the registry for a folder's store.
enum Holder {
    Shared(Weak<Mutex<Store>>),
    Private(Weak<Claim>),
}

/// A holder that excludes another open, as it is now.
enum Live {
    Shared(Arc<Mutex<Store>>),
    Private,
}

impl Holder {
    fn alive(&self) -> bool {
        match self {
            Holder::Shared(store) => store.strong_count() > 0,
            Holder::Private(claim) => claim.strong_count() > 0,
        }
    }

    /// The holder, if it still excludes another open. A shared store that
    /// can no longer be locked does not: a tool that panicked with the lock
    /// held poisons it for good, and every command on it then fails, so it
    /// is not one to hand to the next open, which builds a working one.
    ///
    /// That only happens in a dev or test build. The release profile aborts
    /// on panic (`[profile.release] panic = "abort"`), so a panicking tool
    /// takes the process with it and no store is left poisoned.
    fn live(&self) -> Option<Live> {
        match self {
            Holder::Shared(store) => store
                .upgrade()
                .filter(|store| !store.is_poisoned())
                .map(Live::Shared),
            Holder::Private(claim) => (claim.strong_count() > 0).then_some(Live::Private),
        }
    }
}

#[derive(Default)]
struct Maps {
    folders: HashMap<FolderId, Weak<Folder>>,
    holders: HashMap<FolderId, Holder>,
}

impl Maps {
    fn prune(&mut self) {
        self.folders.retain(|_, folder| folder.strong_count() > 0);
        self.holders.retain(|_, holder| holder.alive());
    }

    fn live(&self, id: &FolderId) -> Option<Live> {
        self.holders.get(id).and_then(Holder::live)
    }
}

/// A store opened shared, and whether this open is the one that built it.
pub enum Opened {
    /// Nobody had the folder open: the store was built from it just now and
    /// has read it.
    New(LiveStore),
    /// A store was already alive for the folder (held by the window, the
    /// agent mid-turn, a command), and this is it. Its in-memory state is
    /// whatever it was; [`Store::reread`] reads the folder into it.
    Existing(LiveStore),
}

impl Opened {
    /// Whether the store was built by this open.
    pub fn is_new(&self) -> bool {
        matches!(self, Opened::New(_))
    }

    pub fn into_store(self) -> LiveStore {
        match self {
            Opened::New(store) | Opened::Existing(store) => store,
        }
    }
}

/// The shared handle of a project's store, as the registry made it.
///
/// This is the only way to get one: the field is private to this module, so
/// a type that holds a `LiveStore` (`AppState`'s open project) holds a store
/// the registry knows about, and a second one for the same folder cannot be
/// built around it. It is `Arc<Mutex<Store>>` underneath: [`Deref`] gives the
/// `Mutex`, and [`LiveStore::handle`] the `Arc`, for what takes one (the
/// agent, the tool dispatcher).
///
/// Building one around a store of your own does not compile:
///
/// ```compile_fail
/// use std::sync::{Arc, Mutex};
/// let dir = std::env::temp_dir();
/// let store = session::Store::open(&dir).unwrap();
/// let _live = session::LiveStore(Arc::new(Mutex::new(store)));
/// ```
///
/// What does compile is asking the registry:
///
/// ```no_run
/// let dir = std::env::temp_dir().join("a-project");
/// let live: session::LiveStore = session::Store::open_shared(&dir).unwrap();
/// let _head = live.lock().unwrap().head();
/// ```
#[derive(Clone)]
pub struct LiveStore(Arc<Mutex<Store>>);

impl LiveStore {
    /// A clone of the shared handle.
    pub fn handle(&self) -> Arc<Mutex<Store>> {
        Arc::clone(&self.0)
    }

    /// The shared handle, borrowed.
    pub fn as_arc(&self) -> &Arc<Mutex<Store>> {
        &self.0
    }

    /// Whether both are the one store.
    pub fn ptr_eq(&self, other: &LiveStore) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Deref for LiveStore {
    type Target = Mutex<Store>;

    fn deref(&self) -> &Mutex<Store> {
        &self.0
    }
}

impl From<LiveStore> for Arc<Mutex<Store>> {
    fn from(live: LiveStore) -> Self {
        live.0
    }
}

/// What a shared open of `dir` gets from a holder that is already there: the
/// store, or a refusal when the holder is a private writer.
fn taken(dir: &Path, live: Live) -> Result<Opened> {
    match live {
        Live::Shared(store) => Ok(Opened::Existing(LiveStore(store))),
        Live::Private => Err(Error::AlreadyOpen {
            path: dir.to_path_buf(),
        }),
    }
}

/// The stores that are alive, by folder. There is one for the process
/// ([`Registry::global`]); a test makes its own with
/// [`Registry::with_identity`] to choose which paths are one folder.
pub struct Registry {
    identify: Identify,
    maps: Mutex<Maps>,
}

impl Registry {
    /// A registry that tells folders apart with `identify`, for a test that
    /// needs two paths to be one folder (a case-insensitive disk, a bind
    /// mount) on a machine that has neither. The stores it opens share
    /// nothing with the process's own registry.
    pub fn with_identity(identify: Identify) -> Self {
        Self {
            identify,
            maps: Mutex::new(Maps::default()),
        }
    }

    /// The process's registry, with the real [`identify`]. What
    /// [`Store::open_shared`] and the other opens on [`Store`] go through.
    pub fn global() -> &'static Registry {
        static GLOBAL: OnceLock<Registry> = OnceLock::new();
        GLOBAL.get_or_init(|| Registry::with_identity(identify))
    }

    /// The registry lock. Nothing under it can panic (it only edits the
    /// maps), so a poisoned lock holds maps that are whole.
    fn maps(&self) -> MutexGuard<'_, Maps> {
        self.maps.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The record the stores on `dir`'s folder share, made if none is alive.
    fn folder(&self, dir: &Path) -> Result<Arc<Folder>> {
        let Identity { id, pin } = (self.identify)(dir)?;
        let mut maps = self.maps();
        maps.prune();
        if let Some(folder) = maps.folders.get(&id).and_then(Weak::upgrade) {
            return Ok(folder);
        }
        let folder = Arc::new(Folder {
            id,
            identify: self.identify,
            in_flight: AtomicUsize::new(0),
            _pin: pin,
        });
        maps.folders.insert(id, Arc::downgrade(&folder));
        Ok(folder)
    }

    /// The store alive for the project at `dir`, or a new one built from the
    /// folder and registered; see [`Store::open_shared`].
    pub fn open_shared(&self, dir: &Path) -> Result<LiveStore> {
        Ok(self.open_shared_status(dir)?.into_store())
    }

    /// [`Registry::open_shared`], saying whether the store is new.
    pub fn open_shared_status(&self, dir: &Path) -> Result<Opened> {
        self.open_shared_between(dir, || {})
    }

    /// [`Registry::open_shared_status`] with `between` run after the registry
    /// has said "none" and the store is built, and before it is registered:
    /// the moment a competing open can get in.
    fn open_shared_between(&self, dir: &Path, between: impl FnOnce()) -> Result<Opened> {
        fs::create_dir_all(dir)?;
        let folder = self.folder(dir)?;
        let existing = self.maps().live(&folder.id);
        if let Some(live) = existing {
            return taken(dir, live);
        }
        let built = Store::assemble(dir, Arc::clone(&folder), None, false)?;
        between();
        let built = Arc::new(Mutex::new(built));

        let mut maps = self.maps();
        maps.prune();
        if let Some(live) = maps.live(&folder.id) {
            // Two opens of a folder nothing had open can both build a store.
            // The first to get here is the one that is registered; this one
            // takes it and its own is dropped.
            drop(maps);
            return taken(dir, live);
        }
        maps.holders
            .insert(folder.id, Holder::Shared(Arc::downgrade(&built)));
        Ok(Opened::New(LiveStore(built)))
    }

    /// A store for the project at `dir` that nothing else will use, or
    /// [`Error::AlreadyOpen`] when the folder has a live store, shared or
    /// private; see [`Store::open_private`].
    pub fn open_private(&self, dir: &Path) -> Result<Store> {
        fs::create_dir_all(dir)?;
        let folder = self.folder(dir)?;
        let claim = Arc::new(Claim);
        {
            let mut maps = self.maps();
            maps.prune();
            if maps.live(&folder.id).is_some() {
                return Err(Error::AlreadyOpen {
                    path: dir.to_path_buf(),
                });
            }
            maps.holders
                .insert(folder.id, Holder::Private(Arc::downgrade(&claim)));
        }
        // A failure to read the folder drops the claim with it.
        Store::assemble(dir, folder, Some(claim), false)
    }

    /// A store that reads the project at `dir` and cannot write; see
    /// [`Store::open_read_only`].
    pub fn open_read_only(&self, dir: &Path) -> Result<Store> {
        let folder = self.folder(dir)?;
        Store::assemble(dir, folder, None, true)
    }

    /// A store that is not in the registry; see [`Store::open`].
    pub fn open_unregistered(&self, dir: &Path) -> Result<Store> {
        fs::create_dir_all(dir)?;
        let folder = self.folder(dir)?;
        Store::assemble(dir, folder, None, false)
    }

    /// Whether a store is live for the folder at `dir`: a shared one that
    /// can still be locked, or a private one. A folder that is not there has
    /// none, and nothing is made.
    pub fn is_open(&self, dir: &Path) -> bool {
        let Ok(Identity { id, .. }) = (self.identify)(dir) else {
            return false;
        };
        let mut maps = self.maps();
        maps.prune();
        maps.live(&id).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// A disk that does not tell `Music` from `music`: the folder is named
    /// by its last component, folded to lower case. The two spellings are
    /// two real folders on the machine running the test; the registry is
    /// the one that has to take them for one.
    fn casefolded(dir: &Path) -> io::Result<Identity> {
        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_lowercase();
        let hash = name
            .bytes()
            .fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(u64::from(b)));
        Ok(Identity::unpinned(FolderId::new(0, hash)))
    }

    /// Two opens of a folder nothing has open can both find no live store
    /// and both build one. The registry keeps whichever registers first, and
    /// the other open must take that one instead of returning its own: its
    /// own would be a second live store for the folder.
    ///
    /// The competing open runs exactly between the registry saying "none"
    /// and this one registering.
    #[test]
    fn an_open_that_loses_the_race_to_register_takes_the_winners_store() {
        let tmp = tempdir().unwrap();
        let registry = Registry::with_identity(identify);
        let mut competitor = None;

        let got = registry
            .open_shared_between(tmp.path(), || {
                let won = registry.open_shared_status(tmp.path()).unwrap();
                assert!(won.is_new(), "the competitor registers first");
                competitor = Some(won.into_store());
            })
            .unwrap();

        let competitor = competitor.unwrap();
        assert!(
            !got.is_new(),
            "a store another open registered is not new: it has to be read"
        );
        assert!(
            got.into_store().ptr_eq(&competitor),
            "the open that lost the race went on with a store of its own"
        );
        assert!(registry
            .open_shared(tmp.path())
            .unwrap()
            .ptr_eq(&competitor));
    }

    #[test]
    fn a_dead_entry_is_pruned_by_any_later_look() {
        let (a, b) = (tempdir().unwrap(), tempdir().unwrap());
        let registry = Registry::with_identity(identify);
        let held = registry.open_shared(a.path()).unwrap();
        assert_eq!(registry.maps().holders.len(), 1);
        assert_eq!(registry.maps().folders.len(), 1);

        drop(held);
        assert!(!registry.is_open(b.path()), "another folder is not open");

        let maps = registry.maps();
        assert_eq!(maps.holders.len(), 0, "the dead holder was not pruned");
        assert_eq!(maps.folders.len(), 0, "the dead folder was not pruned");
    }

    #[test]
    fn two_names_for_one_identity_are_one_folder_to_the_registry() {
        let tmp = tempdir().unwrap();
        let registry = Registry::with_identity(casefolded);

        let upper = registry.open_shared(&tmp.path().join("Music")).unwrap();
        let lower = registry
            .open_shared_status(&tmp.path().join("music"))
            .unwrap();

        assert!(!lower.is_new());
        assert!(upper.ptr_eq(&lower.into_store()));
        assert!(registry.is_open(&tmp.path().join("MUSIC")));
    }
}
