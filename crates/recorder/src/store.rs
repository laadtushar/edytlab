//! The latency kept for each device pair, in `audio-latency.json` (#203).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::duplex::DevicePair;

/// Where a stored latency came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Measured by the sweep.
    Measured,
    /// Typed in by the person.
    Manual,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stored {
    pub latency_ms: f64,
    /// How far apart the measurements were; absent for a typed value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spread_ms: Option<f64>,
    /// Seconds since the Unix epoch.
    pub measured_at: u64,
    pub source: Source,
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("could not read or write the latency file: {0}")]
    Io(#[from] std::io::Error),
    #[error(
        "the latency file is from a newer version of the app (format {0}); it was not changed"
    )]
    Newer(u32),
    #[error("{0} ms is not a latency this app can apply")]
    Invalid(f64),
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The file's format. Bumped only when an old reader would misread a new
/// file; a reader refuses a newer one rather than rewrite it.
const FORMAT: u32 = 1;
/// What a stored latency may be. A measurement can come out a hair
/// negative (the clock estimate's error); a value outside this is a typo
/// or a broken device, not a delay to apply.
const RANGE_MS: std::ops::RangeInclusive<f64> = -50.0..=2_000.0;

/// The entries are kept as the JSON they were read as and parsed one at a
/// time, when asked for. One of them in a shape this version cannot read
/// (a hand edit, a build that kept more) is then that pair's problem
/// only, and a save writes every other entry back exactly as it was
/// instead of replacing the file with one entry.
#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    entries: BTreeMap<String, serde_json::Value>,
}

/// Serialises read-modify-write within the process. (Two processes on one
/// file lose at worst the other's last entry, which a calibration remakes.)
static WRITE: Mutex<()> = Mutex::new(());

pub struct LatencyStore {
    path: PathBuf,
}

impl LatencyStore {
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn read(&self) -> Result<File, StoreError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(empty()),
            Err(e) => return Err(e.into()),
        };
        // Only the version is read first, so a newer file is recognised
        // even when its entries are shaped differently.
        #[derive(Deserialize)]
        struct Head {
            version: u32,
        }
        match serde_json::from_str::<Head>(&text) {
            Ok(Head { version }) if version > FORMAT => Err(StoreError::Newer(version)),
            Ok(_) => Ok(serde_json::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!("audio-latency.json entries unreadable, starting empty: {e}");
                empty()
            })),
            Err(e) => {
                // A calibration is remade in seconds; a file that cannot
                // be read must not stop the next one from being saved.
                tracing::warn!("audio-latency.json unreadable, starting empty: {e}");
                Ok(empty())
            }
        }
    }

    /// The latency kept for `pair`, if any. An entry this version cannot
    /// read counts as none.
    pub fn get(&self, pair: &DevicePair) -> Result<Option<Stored>, StoreError> {
        let key = pair.key();
        let Some(raw) = self.read()?.entries.remove(&key) else {
            return Ok(None);
        };
        match serde_json::from_value(raw) {
            Ok(entry) => Ok(Some(entry)),
            Err(e) => {
                // Read as not measured; the entry stays in the file until
                // this pair is measured again and replaces it.
                tracing::warn!("audio-latency.json entry for {key} unreadable: {e}");
                Ok(None)
            }
        }
    }

    /// Keep `entry` for `pair`, replacing what was there.
    pub fn put(&self, pair: &DevicePair, entry: Stored) -> Result<(), StoreError> {
        if !entry.latency_ms.is_finite() || !RANGE_MS.contains(&entry.latency_ms) {
            return Err(StoreError::Invalid(entry.latency_ms));
        }
        let _guard = WRITE.lock().unwrap_or_else(|p| p.into_inner());
        let mut file = self.read()?;
        file.entries.insert(
            pair.key(),
            serde_json::to_value(entry).expect("plain data serialises"),
        );
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // A temporary file renamed over the old one, so a crash or a full
        // disk leaves the old calibrations, not half of the new ones.
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(&file).expect("plain data serialises"),
        )?;
        std::fs::rename(&tmp, &self.path).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })?;
        Ok(())
    }
}

fn empty() -> File {
    File {
        version: FORMAT,
        entries: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(out: &str) -> DevicePair {
        DevicePair {
            host: "ALSA".into(),
            output: out.into(),
            input: "Mic".into(),
            out_sr: 48_000,
            out_ch: 2,
            in_sr: 48_000,
            in_ch: 1,
        }
    }

    fn entry(ms: f64, source: Source) -> Stored {
        Stored {
            latency_ms: ms,
            spread_ms: Some(0.2),
            measured_at: 1_700_000_000,
            source,
        }
    }

    fn store() -> (tempfile::TempDir, LatencyStore) {
        let dir = tempfile::tempdir().unwrap();
        let s = LatencyStore::at(dir.path().join("audio-latency.json"));
        (dir, s)
    }

    #[test]
    fn nothing_is_stored_until_something_is() {
        let (_d, s) = store();
        assert_eq!(s.get(&pair("Speakers")).unwrap(), None, "no file yet");
    }

    #[test]
    fn a_value_comes_back_for_its_pair_and_only_its_pair() {
        let (_d, s) = store();
        s.put(&pair("Speakers"), entry(42.5, Source::Measured))
            .unwrap();
        s.put(&pair("Bluetooth"), entry(210.0, Source::Manual))
            .unwrap();
        assert_eq!(s.get(&pair("Speakers")).unwrap().unwrap().latency_ms, 42.5);
        let b = s.get(&pair("Bluetooth")).unwrap().unwrap();
        assert_eq!((b.latency_ms, b.source), (210.0, Source::Manual));
        assert_eq!(s.get(&pair("Headset")).unwrap(), None);
    }

    #[test]
    fn the_rates_are_part_of_the_pair() {
        let (_d, s) = store();
        s.put(&pair("Speakers"), entry(42.5, Source::Measured))
            .unwrap();
        let mut other = pair("Speakers");
        other.out_sr = 44_100;
        assert_eq!(s.get(&other).unwrap(), None);
    }

    #[test]
    fn the_newest_write_for_a_pair_wins() {
        let (_d, s) = store();
        s.put(&pair("Speakers"), entry(42.5, Source::Measured))
            .unwrap();
        s.put(&pair("Speakers"), entry(50.0, Source::Manual))
            .unwrap();
        let e = s.get(&pair("Speakers")).unwrap().unwrap();
        assert_eq!((e.latency_ms, e.source), (50.0, Source::Manual));
    }

    #[test]
    fn the_file_is_plain_json_with_a_version() {
        let (d, s) = store();
        s.put(&pair("Speakers"), entry(42.5, Source::Measured))
            .unwrap();
        let text = std::fs::read_to_string(d.path().join("audio-latency.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["version"], 1);
        let e = &v["entries"]["ALSA|Speakers|Mic|48000|48000"];
        assert_eq!(e["latency_ms"], 42.5);
        assert_eq!(e["source"], "measured");
        assert_eq!(e["measured_at"], 1_700_000_000u64);
    }

    #[test]
    fn a_save_leaves_no_temporary_file_behind() {
        let (d, s) = store();
        s.put(&pair("Speakers"), entry(42.5, Source::Measured))
            .unwrap();
        let names: Vec<String> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["audio-latency.json"]);
    }

    #[test]
    fn a_damaged_file_reads_as_empty_and_is_replaced_by_the_next_save() {
        let (d, s) = store();
        let path = d.path().join("audio-latency.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(s.get(&pair("Speakers")).unwrap(), None);
        s.put(&pair("Speakers"), entry(42.5, Source::Measured))
            .unwrap();
        assert_eq!(s.get(&pair("Speakers")).unwrap().unwrap().latency_ms, 42.5);
    }

    #[test]
    fn a_newer_format_is_left_alone() {
        let (d, s) = store();
        let path = d.path().join("audio-latency.json");
        let newer = r#"{"version":2,"entries":{}}"#;
        std::fs::write(&path, newer).unwrap();
        assert!(matches!(
            s.get(&pair("Speakers")),
            Err(StoreError::Newer(2))
        ));
        assert!(matches!(
            s.put(&pair("Speakers"), entry(1.0, Source::Manual)),
            Err(StoreError::Newer(2))
        ));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            newer,
            "not rewritten"
        );
    }

    #[test]
    fn a_latency_that_cannot_be_applied_is_refused() {
        let (_d, s) = store();
        for bad in [f64::NAN, f64::INFINITY, -500.0, 5_000.0] {
            assert!(matches!(
                s.put(&pair("Speakers"), entry(bad, Source::Manual)),
                Err(StoreError::Invalid(_))
            ));
        }
        // A measurement a hair under zero is the clock estimate's error.
        s.put(&pair("Speakers"), entry(-0.3, Source::Measured))
            .unwrap();
    }

    #[test]
    fn the_folder_is_made_when_it_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let s = LatencyStore::at(dir.path().join(".edytlab").join("audio-latency.json"));
        s.put(&pair("Speakers"), entry(42.5, Source::Measured))
            .unwrap();
        assert!(s.get(&pair("Speakers")).unwrap().is_some());
    }

    /// One entry in a shape this version cannot read (written by a hand
    /// edit, or a build that kept more), beside one that it can.
    const MIXED: &str = r#"{
      "version": 1,
      "entries": {
        "ALSA|bad|Mic|48000|48000": { "latency_ms": "soon" },
        "ALSA|Speakers|Mic|48000|48000": {
          "latency_ms": 42.5, "spread_ms": 0.2, "measured_at": 1700000000, "source": "measured"
        },
        "ALSA|odd|Mic|48000|48000": { "latency_ms": 7.0, "measured_at": 1, "source": "measured", "extra": [1, 2] }
      }
    }"#;

    #[test]
    fn one_bad_entry_does_not_hide_the_good_ones() {
        let (d, s) = store();
        std::fs::write(d.path().join("audio-latency.json"), MIXED).unwrap();
        assert_eq!(s.get(&pair("Speakers")).unwrap().unwrap().latency_ms, 42.5);
        assert_eq!(s.get(&pair("odd")).unwrap().unwrap().latency_ms, 7.0);
        assert_eq!(
            s.get(&pair("bad")).unwrap(),
            None,
            "unreadable reads as not measured"
        );
    }

    #[test]
    fn a_save_keeps_every_other_entry_including_the_one_it_cannot_read() {
        let (d, s) = store();
        let path = d.path().join("audio-latency.json");
        std::fs::write(&path, MIXED).unwrap();
        s.put(&pair("Headset"), entry(210.0, Source::Measured))
            .unwrap();

        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let before: serde_json::Value = serde_json::from_str(MIXED).unwrap();
        for key in [
            "ALSA|bad|Mic|48000|48000",
            "ALSA|Speakers|Mic|48000|48000",
            "ALSA|odd|Mic|48000|48000",
        ] {
            assert_eq!(v["entries"][key], before["entries"][key], "{key} changed");
        }
        assert_eq!(
            v["entries"]["ALSA|Headset|Mic|48000|48000"]["latency_ms"],
            210.0
        );
        assert_eq!(s.get(&pair("Speakers")).unwrap().unwrap().latency_ms, 42.5);
    }

    #[test]
    fn replacing_the_pair_whose_entry_was_unreadable_works() {
        let (d, s) = store();
        std::fs::write(d.path().join("audio-latency.json"), MIXED).unwrap();
        s.put(&pair("bad"), entry(12.0, Source::Manual)).unwrap();
        assert_eq!(s.get(&pair("bad")).unwrap().unwrap().latency_ms, 12.0);
    }
}
