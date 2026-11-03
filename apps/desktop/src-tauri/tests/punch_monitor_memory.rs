//! The punch monitor must not hold several full-length copies of its renders.
//!
//! This has its own test executable so its allocator does not affect any
//! other desktop tests. It measures the real punch preparation on the test
//! thread, after the project and its audio files have been made.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, RefCell};

use edytlab_desktop_lib::{punch, state::AppState};
use recorder::duplex::DevicePair;
use recorder::punch::{Progress, PunchError, Region, Take};
use recorder::store::LatencyStore;
use recorder::RecorderError;
use serde_json::json;

const TABLE_SIZE: usize = 1 << 16;

struct Counts {
    enabled: bool,
    live: usize,
    peak: usize,
    overflow: bool,
    pointers: [usize; TABLE_SIZE],
    // 0 = empty, 1 = occupied, 2 = deleted.
    states: [u8; TABLE_SIZE],
}

thread_local! {
    // Const TLS needs no allocation, including when called by the allocator.
    static COUNTS: RefCell<Counts> = const { RefCell::new(Counts {
        enabled: false,
        live: 0,
        peak: 0,
        overflow: false,
        pointers: [0; TABLE_SIZE],
        states: [0; TABLE_SIZE],
    }) };
}

impl Counts {
    fn insert(&mut self, ptr: *mut u8, size: usize) {
        let start = (ptr as usize >> 4).wrapping_mul(0x9e37_79b9) % TABLE_SIZE;
        let mut deleted = None;
        for n in 0..TABLE_SIZE {
            let i = (start + n) % TABLE_SIZE;
            match self.states[i] {
                0 => {
                    let i = deleted.unwrap_or(i);
                    self.pointers[i] = ptr as usize;
                    self.states[i] = 1;
                    self.live += size;
                    self.peak = self.peak.max(self.live);
                    return;
                }
                2 if deleted.is_none() => deleted = Some(i),
                _ => {}
            }
        }
        if let Some(i) = deleted {
            self.pointers[i] = ptr as usize;
            self.states[i] = 1;
            self.live += size;
            self.peak = self.peak.max(self.live);
        } else {
            self.overflow = true;
        }
    }

    fn remove(&mut self, ptr: *mut u8, size: usize) -> bool {
        let start = (ptr as usize >> 4).wrapping_mul(0x9e37_79b9) % TABLE_SIZE;
        for n in 0..TABLE_SIZE {
            let i = (start + n) % TABLE_SIZE;
            if self.states[i] == 0 {
                return false;
            }
            if self.states[i] == 1 && self.pointers[i] == ptr as usize {
                self.states[i] = 2;
                self.live -= size;
                return true;
            }
        }
        false
    }

    fn begin(&mut self) {
        self.live = 0;
        self.peak = 0;
        self.overflow = false;
        self.states.fill(0);
        self.enabled = true;
    }
}

fn track(f: impl FnOnce(&mut Counts)) {
    let _ = COUNTS.try_with(|counts| {
        // Panic formatting can allocate while a borrow is held.
        if let Ok(mut c) = counts.try_borrow_mut() {
            if c.enabled {
                f(&mut c);
            }
        }
    });
}

struct CountingAllocator;

// SAFETY: allocation and deallocation are delegated unchanged to System;
// accounting uses an allocation-free thread-local RefCell and skips re-entry.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            track(|c| c.insert(ptr, layout.size()));
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            track(|c| c.insert(ptr, layout.size()));
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        track(|c| {
            c.remove(ptr, layout.size());
        });
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: GlobalAlloc callers provide the live allocation and its layout.
        // On failure System leaves the old allocation live; retain its accounting.
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            track(|c| {
                c.remove(ptr, layout.size());
                // An allocation made before measurement may have grown here.
                // Count its full new size rather than subtracting old memory.
                c.insert(new_ptr, new_size);
            });
        }
        new_ptr
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

// Own explicit test allocations so unwinding always frees the original layout.
struct TestBlock {
    ptr: std::ptr::NonNull<u8>,
    layout: Layout,
}

impl TestBlock {
    fn new(layout: Layout) -> Self {
        // SAFETY: Layout has nonzero size and valid alignment at every caller.
        let ptr = unsafe { std::alloc::alloc(layout) };
        let ptr =
            std::ptr::NonNull::new(ptr).unwrap_or_else(|| std::alloc::handle_alloc_error(layout));
        Self { ptr, layout }
    }
}

impl Drop for TestBlock {
    fn drop(&mut self) {
        // SAFETY: this guard uniquely owns the allocation and original layout.
        unsafe { std::alloc::dealloc(self.ptr.as_ptr(), self.layout) };
    }
}

#[test]
fn allocator_skips_accounting_while_counts_are_borrowed() {
    let layout = Layout::from_size_align(1024, 8).unwrap();
    COUNTS.with(|counts| {
        counts.borrow_mut().begin();
        let c = counts.borrow();
        // Explicit allocator calls cannot be optimized away. Both operations
        // re-enter the tracker while the shared borrow is held.
        let block = TestBlock::new(layout);
        drop(block);
        assert_eq!(c.live, 0);
        assert_eq!(c.peak, 0);
    });
    COUNTS.with(|counts| counts.borrow_mut().enabled = false);
}

fn pair() -> DevicePair {
    DevicePair {
        host: "Fake".into(),
        output: "Speakers".into(),
        input: "Mic".into(),
        out_sr: 44_100,
        out_ch: 2,
        in_sr: 48_000,
        in_ch: 1,
    }
}

struct StopAtRecording {
    monitor_samples: Cell<usize>,
}

impl punch::Audio for StopAtRecording {
    fn default_pair(&self) -> Result<DevicePair, RecorderError> {
        Ok(pair())
    }

    fn record(
        &self,
        _pair: &DevicePair,
        monitor: &[f32],
        _region: Region,
        _latency_sec: f64,
        _cancel: &dyn Fn() -> bool,
        _on_progress: &dyn Fn(Progress),
    ) -> Result<Take, PunchError> {
        self.monitor_samples.set(monitor.len());
        COUNTS.with(|counts| counts.borrow_mut().enabled = false);
        Err(PunchError::Cancelled)
    }
}

#[test]
fn preparing_a_long_punch_does_not_keep_multiple_full_length_audio_buffers() {
    const SECONDS: usize = 40;
    const SOURCE_RATE: usize = 48_000;
    let dir = tempfile::tempdir().unwrap();
    let project_dir = dir.path().join("project");
    std::fs::create_dir_all(&project_dir).unwrap();
    let lead = dir.path().join("lead.wav");
    let bed = dir.path().join("bed.wav");
    let samples = vec![0.25; SECONDS * SOURCE_RATE * 2];
    recorder::punch::write_wav(&lead, &samples, SOURCE_RATE as u32, 2).unwrap();
    drop(samples);
    let samples = vec![0.1; SECONDS * SOURCE_RATE];
    recorder::punch::write_wav(&bed, &samples, SOURCE_RATE as u32, 1).unwrap();
    drop(samples);

    let live = session::Store::open_shared(&project_dir).unwrap();
    {
        let handle = live.handle();
        let mut store = handle.lock().unwrap();
        let mut engine = audio_engine::Engine::new();
        let mut clipboard = None;
        let dispatcher = tools::ToolDispatcher::default_dispatcher();
        for path in [&lead, &bed] {
            let mut ctx = tools::ToolContext {
                store: &mut store,
                engine: &mut engine,
                user_message: "",
                clipboard: &mut clipboard,
                allowed_tools: None,
            };
            match dispatcher
                .invoke("load", json!({ "path": path }), &mut ctx)
                .unwrap()
            {
                tools::ToolResult::Ok(_) => {}
                tools::ToolResult::Error(message) => panic!("load failed: {message}"),
            }
        }
    }
    let state = AppState::new();
    state.set_open_project(live, project_dir);
    let latency = LatencyStore::at(dir.path().join("latency.json"));
    latency.put_manual(&pair(), 45.0).unwrap();
    let request = punch::Request::new(0, 0.0, SECONDS as f64, Some(0.0), Some(0.0)).unwrap();
    let audio = StopAtRecording {
        monitor_samples: Cell::new(0),
    };

    // A block from before the window must not cancel new allocations when it
    // is freed during the window. Use the global allocator explicitly so the
    // optimiser cannot eliminate the allocations under test.
    let layout = Layout::from_size_align(1024, 8).unwrap();
    let old = TestBlock::new(layout);
    COUNTS.with(|counts| counts.borrow_mut().begin());
    drop(old);
    let fresh = TestBlock::new(layout);
    COUNTS.with(|counts| {
        let c = counts.borrow();
        assert_eq!(c.live, 1024);
        assert!(!c.overflow);
    });
    drop(fresh);
    COUNTS.with(|counts| {
        let mut c = counts.borrow_mut();
        assert_eq!(c.live, 0);
        c.begin();
    });
    let result = punch::run(&state, &audio, &latency, &request, &|| false, &|_| {});
    let (peak, overflow) = COUNTS.with(|counts| {
        let mut c = counts.borrow_mut();
        c.enabled = false;
        (c.peak, c.overflow)
    });

    assert_eq!(result.unwrap(), json!({ "cancelled": true }));
    assert!(!overflow, "allocation tracker pointer table filled");
    let expected_samples = SECONDS * 44_100 * 2;
    assert_eq!(audio.monitor_samples.get(), expected_samples);
    let monitor_bytes = expected_samples * std::mem::size_of::<f32>();
    // One monitor plus bounded scratch. Even one extra full source render
    // (15,360,000 bytes here) must exceed this allowance.
    let limit = monitor_bytes + 8 * 1024 * 1024;
    eprintln!(
        "punch preparation: peak={peak} bytes, monitor={monitor_bytes} bytes, limit={limit} bytes"
    );
    assert!(
        peak < limit,
        "preparation peaked at {peak} allocated bytes; limit is {limit}, monitor is {monitor_bytes}"
    );
}
