//! The app's own settings file, `~/.edytlab/settings.json` (#569).
//!
//! Where the app keeps a choice that is not a secret, belongs to no single
//! project and has to be known before the first message. Beside
//! `recents.json`, `mcp.json` and `memory.md`, which are the same kind of
//! thing.
//!
//! Only Plan first is in it. It is an approval gate (with it on, no edit
//! runs until the person approves it), so losing it at a restart failed in
//! the unsafe direction: the toggle came back off and the next edits
//! applied with no card.
//!
//! * **Not the keychain.** The model, effort and base URL live there, but
//!   they are read on a thread of their own because a locked Secret
//!   Service's unlock prompt has no time limit (#497). A gate cannot be
//!   unknown while the person is answering a prompt, and after a refused
//!   startup nothing is read from the keychain again, so a setting kept
//!   there would be lost exactly when the agent is built.
//! * **Not `localStorage`.** The backend has to know it before the first
//!   message, and a frontend that pushed it on mount would race the
//!   first send.
//!
//! Every read tolerates a missing, unreadable or corrupt file by returning
//! the default (off, as on a fresh install): a settings file edited into
//! nonsense must not stop the app starting. A write replaces the file
//! atomically and keeps any field it does not know, so a build that adds
//! one does not lose it to a build that does not have it.

use std::io::Write;
use std::path::Path;

use serde_json::{Map, Value};

/// The file, inside `~/.edytlab/`.
pub const SETTINGS_FILE: &str = "settings.json";

/// The field Plan first is kept in.
const PLAN_FIRST: &str = "plan_first";

/// Everything in the file, as the object it is. Empty when there is no
/// usable one; a file that exists and is not usable is logged, since it
/// means a choice the person made is not being honoured.
fn read_fields(path: &Path) -> Map<String, Value> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        // A fresh install, and nothing to report.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Map::new(),
        Err(e) => {
            tracing::warn!(
                error = %e,
                path = %path.display(),
                "settings file could not be read; using the defaults"
            );
            return Map::new();
        }
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(fields)) => fields,
        Ok(_) | Err(_) => {
            tracing::warn!(
                path = %path.display(),
                "settings file is not a JSON object; using the defaults"
            );
            Map::new()
        }
    }
}

/// Whether Plan first was left on. Off when the file is missing, cannot be
/// read, is corrupt, or holds something other than a boolean.
pub fn read_plan_first(path: &Path) -> bool {
    match read_fields(path).get(PLAN_FIRST) {
        None => false,
        Some(Value::Bool(on)) => *on,
        Some(_) => {
            tracing::warn!(
                path = %path.display(),
                "settings file has a Plan first value that is not true or false; treating it as off"
            );
            false
        }
    }
}

/// Record whether Plan first is on, creating the folder if need be and
/// replacing the file in one step: a quit or a crash in the middle leaves
/// the old file or the new one, never half of either. A file that cannot
/// be read as settings is replaced rather than refused, so a person with a
/// corrupt one can still choose.
pub fn write_plan_first(path: &Path, on: bool) -> std::io::Result<()> {
    let mut fields = read_fields(path);
    fields.insert(PLAN_FIRST.to_string(), Value::Bool(on));
    let text = serde_json::to_string_pretty(&Value::Object(fields))?;

    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    std::fs::create_dir_all(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(text.as_bytes())?;
    tmp.write_all(b"\n")?;
    tmp.flush()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn file(dir: &TempDir) -> std::path::PathBuf {
        dir.path().join("settings.json")
    }

    #[test]
    fn a_missing_file_reads_as_off() {
        let dir = TempDir::new().unwrap();
        assert!(!read_plan_first(&file(&dir)));
    }

    #[test]
    fn what_was_written_is_what_is_read() {
        let dir = TempDir::new().unwrap();
        write_plan_first(&file(&dir), true).unwrap();
        assert!(read_plan_first(&file(&dir)));
        write_plan_first(&file(&dir), false).unwrap();
        assert!(!read_plan_first(&file(&dir)));
    }

    /// The folder does not exist on a fresh install.
    #[test]
    fn writing_creates_the_folder() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".edytlab").join(SETTINGS_FILE);
        write_plan_first(&path, true).unwrap();
        assert!(read_plan_first(&path));
    }

    /// Nothing but the file is left in the folder: the temporary file the
    /// write goes through is moved onto it, not left beside it.
    #[test]
    fn a_write_leaves_nothing_else_behind() {
        let dir = TempDir::new().unwrap();
        write_plan_first(&file(&dir), true).unwrap();
        write_plan_first(&file(&dir), false).unwrap();
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, [SETTINGS_FILE]);
    }

    #[test]
    fn a_corrupt_file_reads_as_off_and_is_replaced_by_the_next_write() {
        let dir = TempDir::new().unwrap();
        std::fs::write(file(&dir), "{ not json").unwrap();
        assert!(!read_plan_first(&file(&dir)));

        write_plan_first(&file(&dir), true).unwrap();
        assert!(read_plan_first(&file(&dir)));
        let text = std::fs::read_to_string(file(&dir)).unwrap();
        assert!(serde_json::from_str::<Value>(&text).is_ok(), "{text}");
    }

    #[test]
    fn a_file_that_is_not_an_object_reads_as_off() {
        let dir = TempDir::new().unwrap();
        std::fs::write(file(&dir), "[true]").unwrap();
        assert!(!read_plan_first(&file(&dir)));
        std::fs::write(file(&dir), "true").unwrap();
        assert!(!read_plan_first(&file(&dir)));
    }

    /// Anything but a real boolean is not "on": a hand edit to `"yes"`
    /// does not turn the gate on by accident, or off by being ignored
    /// differently from how the rest of the file is.
    #[test]
    fn a_value_that_is_not_a_boolean_reads_as_off() {
        let dir = TempDir::new().unwrap();
        for value in ["\"true\"", "1", "null", "{}"] {
            std::fs::write(file(&dir), format!("{{ \"plan_first\": {value} }}")).unwrap();
            assert!(!read_plan_first(&file(&dir)), "{value}");
        }
    }

    /// A field this build does not know survives a write, so a newer
    /// build's settings are not lost to an older one.
    #[test]
    fn a_write_keeps_the_fields_it_does_not_know() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            file(&dir),
            r#"{ "plan_first": false, "from_a_later_build": { "n": 3 } }"#,
        )
        .unwrap();

        write_plan_first(&file(&dir), true).unwrap();

        let kept: Value =
            serde_json::from_str(&std::fs::read_to_string(file(&dir)).unwrap()).unwrap();
        assert_eq!(kept["plan_first"], Value::Bool(true));
        assert_eq!(kept["from_a_later_build"]["n"], 3);
    }

    /// A path that is a directory can neither be read nor replaced. The
    /// read answers off, the write says it failed, and neither panics.
    #[test]
    fn a_path_that_is_a_directory_reads_as_off_and_fails_to_write() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(SETTINGS_FILE);
        std::fs::create_dir(&path).unwrap();
        assert!(!read_plan_first(&path));
        assert!(write_plan_first(&path, true).is_err());
    }
}
