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
//! project is let go. (It is a handle on an open project's folder: while the
//! project is open the volume cannot be ejected or unmounted, and a folder
//! that cannot be opened for reading cannot be opened as a project. Neither
//! was so before the pin; both are the price of not letting two writers
//! share a folder.)
//!
//! ## A folder replaced at the same path
//!
//! A store writes by path, and a path can come to name another folder: the
//! project is restored from a backup over itself, or trashed and made again.
//! The new folder has a new identity, so nothing in the registry names it;
//! a store opened on it would start with nothing in flight while the store
//! on the old one is still writing, by path, into it. So every store
//! registers the canonical path it reads its folder through and keeps it
//! while it lives (it changes when the project folder is renamed and the
//! store is read again through the new name), and a write open (shared,
//! private, raw) of a path that a *live* store of a different folder writes
//! through is refused with [`Error::FolderReplaced`] until that store is
//! gone. The window's own handle can be let go for it (the
//! desktop app does, when nothing else holds the store: no turn, no staged
//! edit); a turn's cannot, and the user is told to wait for it and open the
//! project again. [`Store::replaced_holder`] says who holds the old store.
//!
//! ## Ways to open a store
//!
//! | Open | Registry | A live store for the folder |
//! |------|----------|-----------------------------|
//! | [`Store::open_shared`] | finds or registers the one live store | handed back, shared |
//! | [`Store::open_private`] | registers a claim for as long as the store lives | [`Error::AlreadyOpen`] |
//! | [`Store::open_read_only`] | not registered | no matter: it cannot write |
//! | `Store::open` | not registered | no matter, and no protection: for a folder only the caller can reach (tests; a `raw-open` feature) |
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
//! The registry's lock is a leaf: nothing else is taken while it is held. It
//! is taken for a lookup or an insertion and dropped before the caller does
//! anything else. The identity is worked out (it touches the disk) before it
//! is taken, a store is built after it is dropped, and no store, engine,
//! dispatcher or `AppState` lock is taken under it. The only thing asked of
//! a store under it is [`Mutex::is_poisoned`], which does not lock, and no
//! store is *dropped* under it either: what a lookup upgrades is kept in a
//! `Keep` that outlives the guard, because letting go of a store's last
//! strong reference runs its drop.
//!
//! That is a statement about what is held *while* the registry's lock is
//! held, not about what may be held when it is asked for. The registry can
//! be asked for under a store or engine lock, and is: `batch_apply` and the
//! replay in `tools::rederive` open folders of their own from inside a tool
//! that holds the project's store. That cannot deadlock, because the
//! registry never waits for anything while it holds its lock.
//!
//! The registry is only a view. It holds [`Weak`] references, so it does not
//! keep a store alive: once the last holder lets go the entry is dead and is
//! pruned, and the next open builds a store from the folder as it is then.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};

use crate::store::Store;
use crate::{Error, Result};

/// What a folder is, whatever it is called: its device and inode (unix), or
/// its volume serial number and 128-bit file id (Windows, `FileIdInfo`; the
/// 64-bit file index that older calls return is not guaranteed unique on
/// ReFS, which Dev Drive uses).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FolderId {
    volume: u64,
    index: u128,
}

impl FolderId {
    /// An identity from its two halves, for an [`Identify`] test double.
    /// The real one comes from [`identify`].
    pub const fn new(volume: u64, index: u128) -> Self {
        Self { volume, index }
    }
}

/// An open handle on the folder, kept so its identity cannot be given to
/// another folder while a store is on it (see the module documentation).
#[cfg(unix)]
type Pin = fs::File;
#[cfg(windows)]
type Pin = fs::File;

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
/// two paths one folder without a bind mount (see `Registry::with_identity`).
pub type Identify = fn(&Path) -> io::Result<Identity>;

/// The identity of the folder at `dir`: the real [`Identify`]. Errors when
/// `dir` cannot be opened for reading, which includes it not existing.
///
/// A regular file answers as well, with the identity that tells it from every
/// other file on the machine whatever it is called: `tools::destination` asks
/// it whether a path to write is a file the project reads (#653).
#[cfg(unix)]
pub fn identify(dir: &Path) -> io::Result<Identity> {
    use std::os::unix::fs::MetadataExt;
    let pin = fs::File::open(dir)?;
    let meta = pin.metadata()?;
    Ok(Identity {
        id: FolderId::new(meta.dev(), u128::from(meta.ino())),
        pin: Some(pin),
    })
}

/// The identity of the folder at `dir`: the real [`Identify`]. Errors when
/// `dir` cannot be opened for reading, which includes it not existing. A
/// regular file answers as well (see the unix variant).
///
/// `GetFileInformationByHandleEx` with `FileIdInfo` gives the 128-bit file id
/// and the 64-bit volume serial number. The older `nFileIndex` is 64 bits
/// and "not guaranteed to be unique on ReFS", which is what a Dev Drive is
/// (<https://learn.microsoft.com/en-us/windows/win32/api/fileapi/ns-fileapi-by_handle_file_information>).
/// A file system that does not answer `FileIdInfo` (FAT, exFAT) falls back
/// to the 64-bit index, which is unique there, and only on the errors that
/// say so ([`lacks_file_id_info`]): on NTFS the two calls give different keys
/// for one folder, so a transient failure that fell back would give a folder
/// two identities, and so two stores. Both calls are `windows-sys` 0.61.2's,
/// already in the lock.
#[cfg(windows)]
pub fn identify(dir: &Path) -> io::Result<Identity> {
    use std::ffi::c_void;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FileIdInfo, GetFileInformationByHandle, GetFileInformationByHandleEx,
        BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_INFO,
    };

    // A directory is opened with backup semantics. std's default share mode
    // lets it be renamed or deleted while the handle is open.
    let pin = fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(dir)?;
    let handle = pin.as_raw_handle();

    // SAFETY: `handle` is open for as long as `pin` is, which outlives both
    // calls; each buffer is a zeroed, correctly typed struct of the size
    // passed, and both are plain integers that any bytes are valid for.
    let id = unsafe {
        let mut wide: FILE_ID_INFO = std::mem::zeroed();
        let ok = GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            std::ptr::addr_of_mut!(wide).cast::<c_void>(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        );
        if ok != 0 {
            FolderId::new(
                wide.VolumeSerialNumber,
                u128::from_le_bytes(wide.FileId.Identifier),
            )
        } else {
            let failed = io::Error::last_os_error();
            if !lacks_file_id_info(failed.raw_os_error()) {
                return Err(failed);
            }
            let mut narrow: BY_HANDLE_FILE_INFORMATION = std::mem::zeroed();
            if GetFileInformationByHandle(handle, &mut narrow) == 0 {
                return Err(io::Error::last_os_error());
            }
            FolderId::new(
                u64::from(narrow.dwVolumeSerialNumber),
                (u128::from(narrow.nFileIndexHigh) << 32) | u128::from(narrow.nFileIndexLow),
            )
        }
    };
    Ok(Identity { id, pin: Some(pin) })
}

/// Whether a Win32 error from `GetFileInformationByHandleEx(FileIdInfo)` says
/// the file system or Windows has no 128-bit file ids, as opposed to the call
/// having failed: `ERROR_INVALID_FUNCTION` (1), `ERROR_NOT_SUPPORTED` (50),
/// `ERROR_INVALID_PARAMETER` (87) and `ERROR_INVALID_LEVEL` (124). Any other
/// error is the open's. Spelt as numbers, not `windows-sys` constants, so the
/// decision can be tested on every platform; a Windows test pins the numbers.
#[cfg_attr(not(windows), allow(dead_code))]
fn lacks_file_id_info(code: Option<i32>) -> bool {
    matches!(code, Some(1 | 50 | 87 | 124))
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
    /// The paths the stores on this folder write through right now.
    pub(crate) spellings: Arc<Spellings>,
    _pin: Option<Pin>,
}

/// Held by a store (and by every staged handle made from it) for as long as
/// it writes through the path it was registered under.
pub(crate) struct Spelling;

/// The canonical paths the live stores of one folder write through.
///
/// A store writes by the path it was opened with (`project_dir`), and that
/// moves: the project folder is renamed and the next open reads it through
/// its new name ([`Store::reread_into`]). The old path is then free, and
/// the new one is the one a replaced folder would have to be refused at. So
/// each store registers the path it was read under and keeps it while it
/// lives, and a staged handle keeps the one it was staged with after the
/// store has moved on, because that is the path its edit still writes
/// through. A record has no spelling of its own.
#[derive(Default)]
pub(crate) struct Spellings(Mutex<Vec<(PathBuf, Weak<Spelling>)>>);

impl Spellings {
    fn list(&self) -> MutexGuard<'_, Vec<(PathBuf, Weak<Spelling>)>> {
        // Nothing under it can panic (it only edits the list).
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Record that a store reads the folder through `dir`; the store keeps
    /// the returned token for as long as it does. A leaf under the
    /// registry's lock: nothing is taken while it is held.
    pub(crate) fn register(&self, dir: &Path) -> Arc<Spelling> {
        let spelling = spelling_of(dir);
        let token = Arc::new(Spelling);
        let mut list = self.list();
        list.retain(|(_, token)| token.strong_count() > 0);
        list.push((spelling, Arc::downgrade(&token)));
        token
    }

    /// Whether a live store writes through `spelling`.
    fn writes_through(&self, spelling: &Path) -> bool {
        let mut list = self.list();
        list.retain(|(_, token)| token.strong_count() > 0);
        list.iter().any(|(path, _)| path == spelling)
    }
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

/// Stores a lookup upgraded under the registry's lock and has no further
/// use for. Declared before the guard, so it is dropped after it: letting go
/// of a store's last strong reference runs its drop, and nothing runs under
/// the registry's lock but edits to the maps.
type Keep = Vec<Arc<Mutex<Store>>>;

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
    /// is not one to hand to the next open, which builds a working one. It
    /// goes into `keep` rather than being dropped here.
    ///
    /// That only happens in a dev or test build. The release profile aborts
    /// on panic (`[profile.release] panic = "abort"`), so a panicking tool
    /// takes the process with it and no store is left poisoned.
    fn live(&self, keep: &mut Keep) -> Option<Live> {
        match self {
            Holder::Shared(store) => {
                let store = store.upgrade()?;
                if store.is_poisoned() {
                    keep.push(store);
                    return None;
                }
                Some(Live::Shared(store))
            }
            Holder::Private(claim) => (claim.strong_count() > 0).then_some(Live::Private),
        }
    }
}

/// A folder's shared record, and the paths its stores write through (see
/// "A folder replaced at the same path" above).
struct FolderEntry {
    folder: Weak<Folder>,
    spellings: Arc<Spellings>,
}

#[derive(Default)]
struct Maps {
    folders: HashMap<FolderId, FolderEntry>,
    holders: HashMap<FolderId, Holder>,
}

impl Maps {
    fn prune(&mut self) {
        self.folders
            .retain(|_, entry| entry.folder.strong_count() > 0);
        self.holders.retain(|_, holder| holder.alive());
    }

    fn live(&self, id: &FolderId, keep: &mut Keep) -> Option<Live> {
        self.holders.get(id).and_then(|holder| holder.live(keep))
    }

    /// The identities of live records of *other* folders whose stores write
    /// through `spelling`.
    fn replaced(&self, id: &FolderId, spelling: &Path) -> Vec<FolderId> {
        self.folders
            .iter()
            .filter(|(other, entry)| {
                *other != id
                    && entry.folder.strong_count() > 0
                    && entry.spellings.writes_through(spelling)
            })
            .map(|(other, _)| *other)
            .collect()
    }
}

/// Whether an open may write to the folder, and so must not share a path
/// with a live record of another folder.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Writer,
    Reader,
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
/// let dir = std::env::temp_dir().join("a-project");
/// let store = session::Store::open_private(&dir).unwrap();
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
/// `Registry::with_identity` to choose which paths are one folder.
pub struct Registry {
    identify: Identify,
    maps: Mutex<Maps>,
}

/// The canonical path of `dir`, or `dir` as given when it cannot be resolved
/// (a folder opened here exists by then, so that is the unusual case).
fn spelling_of(dir: &Path) -> PathBuf {
    fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf())
}

impl Registry {
    fn new(identify: Identify) -> Self {
        Self {
            identify,
            maps: Mutex::new(Maps::default()),
        }
    }

    /// A registry that tells folders apart with `identify`, for a test that
    /// needs two paths to be one folder (a case-insensitive disk, a bind
    /// mount) on a machine that has neither. The stores it opens share
    /// nothing with the process's own registry, so a second registry is a
    /// second writer on a folder: it exists only in a test build or with the
    /// `raw-open` feature, like the raw open.
    #[cfg(any(test, feature = "raw-open"))]
    pub fn with_identity(identify: Identify) -> Self {
        Self::new(identify)
    }

    /// The process's registry, with the real [`identify`]. What
    /// [`Store::open_shared`] and the other opens on [`Store`] go through.
    pub fn global() -> &'static Registry {
        static GLOBAL: OnceLock<Registry> = OnceLock::new();
        GLOBAL.get_or_init(|| Registry::new(identify))
    }

    /// The registry lock. Nothing under it can panic (it only edits the
    /// maps), so a poisoned lock holds maps that are whole.
    fn maps(&self) -> MutexGuard<'_, Maps> {
        self.maps.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The record the stores on `dir`'s folder share, made if none is alive.
    /// For a writer, [`Error::FolderReplaced`] when a live store of another
    /// folder writes through the same path.
    fn folder(&self, dir: &Path, scope: Scope) -> Result<Arc<Folder>> {
        let Identity { id, pin } = (self.identify)(dir)?;
        let spelling = spelling_of(dir);
        let mut maps = self.maps();
        maps.prune();
        if scope == Scope::Writer && !maps.replaced(&id, &spelling).is_empty() {
            return Err(Error::FolderReplaced {
                path: dir.to_path_buf(),
            });
        }
        if let Some(folder) = maps.folders.get(&id).and_then(|e| e.folder.upgrade()) {
            return Ok(folder);
        }
        let spellings = Arc::new(Spellings::default());
        let folder = Arc::new(Folder {
            id,
            identify: self.identify,
            in_flight: AtomicUsize::new(0),
            spellings: Arc::clone(&spellings),
            _pin: pin,
        });
        maps.folders.insert(
            id,
            FolderEntry {
                folder: Arc::downgrade(&folder),
                spellings,
            },
        );
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
        let folder = self.folder(dir, Scope::Writer)?;
        let mut keep = Keep::new();
        let existing = self.maps().live(&folder.id, &mut keep);
        if let Some(live) = existing {
            return taken(dir, live);
        }
        let built = Store::assemble(dir, Arc::clone(&folder), None, false)?;
        between();
        let built = Arc::new(Mutex::new(built));

        let lost = {
            let mut maps = self.maps();
            maps.prune();
            match maps.live(&folder.id, &mut keep) {
                // Two opens of a folder nothing had open can both build a
                // store. The first to get here is the one that is
                // registered; this one takes it and its own is dropped.
                Some(live) => Some(live),
                None => {
                    maps.holders
                        .insert(folder.id, Holder::Shared(Arc::downgrade(&built)));
                    None
                }
            }
        };
        match lost {
            Some(live) => taken(dir, live),
            None => Ok(Opened::New(LiveStore(built))),
        }
    }

    /// A store for the project at `dir` that nothing else will use, or
    /// [`Error::AlreadyOpen`] when the folder has a live store, shared or
    /// private; see [`Store::open_private`].
    pub fn open_private(&self, dir: &Path) -> Result<Store> {
        fs::create_dir_all(dir)?;
        let folder = self.folder(dir, Scope::Writer)?;
        let claim = Arc::new(Claim);
        let mut keep = Keep::new();
        let refused = {
            let mut maps = self.maps();
            maps.prune();
            let live = maps.live(&folder.id, &mut keep);
            if live.is_none() {
                maps.holders
                    .insert(folder.id, Holder::Private(Arc::downgrade(&claim)));
            }
            live
        };
        if refused.is_some() {
            return Err(Error::AlreadyOpen {
                path: dir.to_path_buf(),
            });
        }
        // A failure to read the folder drops the claim with it.
        Store::assemble(dir, folder, Some(claim), false)
    }

    /// A store that reads the project at `dir` and cannot write; see
    /// [`Store::open_read_only`].
    pub fn open_read_only(&self, dir: &Path) -> Result<Store> {
        let folder = self.folder(dir, Scope::Reader)?;
        Store::assemble(dir, folder, None, true)
    }

    /// A store that is not in the registry; see `Store::open`.
    #[cfg(any(test, feature = "raw-open"))]
    pub fn open_unregistered(&self, dir: &Path) -> Result<Store> {
        fs::create_dir_all(dir)?;
        let folder = self.folder(dir, Scope::Writer)?;
        Store::assemble(dir, folder, None, false)
    }

    /// Whether a store is live for the folder at `dir`: a shared one that
    /// can still be locked, or a private one. A folder that is not there has
    /// none, and nothing is made.
    pub fn is_open(&self, dir: &Path) -> bool {
        let Ok(Identity { id, .. }) = (self.identify)(dir) else {
            return false;
        };
        let mut keep = Keep::new();
        let live = {
            let mut maps = self.maps();
            maps.prune();
            maps.live(&id, &mut keep)
        };
        live.is_some()
    }

    /// The shared store that is alive for the folder *that used to be* at
    /// `dir`, if the path now names another folder; see
    /// [`Store::replaced_holder`].
    pub fn replaced_holder(&self, dir: &Path) -> Option<LiveStore> {
        let Identity { id, .. } = (self.identify)(dir).ok()?;
        let spelling = spelling_of(dir);
        let mut keep = Keep::new();
        let found = {
            let mut maps = self.maps();
            maps.prune();
            let others = maps.replaced(&id, &spelling);
            others
                .iter()
                .find_map(|other| match maps.live(other, &mut keep) {
                    Some(Live::Shared(store)) => Some(store),
                    _ => None,
                })
        };
        found.map(LiveStore)
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
        Ok(Identity::unpinned(FolderId::new(0, u128::from(hash))))
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

    /// A lookup under the registry's lock must not be the one to let go of a
    /// store: if its upgrade turns out to be the last strong reference, the
    /// store's drop would run under the lock. A poisoned store, which a
    /// lookup passes over, is handed to the caller's `keep`, which outlives
    /// the guard, instead of being dropped where it was found.
    #[test]
    fn a_poisoned_store_a_lookup_passes_over_is_kept_not_dropped() {
        let tmp = tempdir().unwrap();
        let registry = Registry::with_identity(identify);
        let broken = registry.open_shared(tmp.path()).unwrap();
        {
            let broken = broken.handle();
            let _ = std::thread::spawn(move || {
                let _held = broken.lock().unwrap();
                panic!("a tool panicked with the store locked");
            })
            .join();
        }
        assert!(broken.is_poisoned());

        let id = (registry.identify)(tmp.path()).unwrap().id();
        let mut keep = Keep::new();
        let live = registry.maps().live(&id, &mut keep);

        assert!(live.is_none(), "a poisoned store is not live");
        assert_eq!(keep.len(), 1, "the upgrade was dropped where it was made");
        assert!(Arc::ptr_eq(&keep[0], broken.as_arc()));
    }

    /// The fallback from a folder's 128-bit id to its 64-bit index is for a
    /// file system that cannot give the first. A failure that is not that (a
    /// handle gone bad, access denied, an I/O error) would give the same
    /// folder a different key from the one it had, and so a second store.
    #[test]
    fn only_the_not_supported_errors_fall_back_to_the_narrow_file_index() {
        for code in [1, 50, 87, 124] {
            assert!(lacks_file_id_info(Some(code)), "error {code}");
        }
        for code in [2, 3, 5, 6, 32, 123, 1117, 1450] {
            assert!(!lacks_file_id_info(Some(code)), "error {code}");
        }
        assert!(!lacks_file_id_info(None), "an error with no OS code");
    }

    #[cfg(windows)]
    #[test]
    fn the_not_supported_codes_are_win32s() {
        use windows_sys::Win32::Foundation::{
            ERROR_INVALID_FUNCTION, ERROR_INVALID_LEVEL, ERROR_INVALID_PARAMETER,
            ERROR_NOT_SUPPORTED,
        };
        for code in [
            ERROR_INVALID_FUNCTION,
            ERROR_NOT_SUPPORTED,
            ERROR_INVALID_PARAMETER,
            ERROR_INVALID_LEVEL,
        ] {
            assert!(lacks_file_id_info(Some(code as i32)), "error {code}");
        }
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
