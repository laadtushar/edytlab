//! Chapter files from markers (#766, scope item 4): the `format` argument of
//! `export_labels`.
//!
//! `export_labels` used to return only Audacity label text. A podcast
//! needs chapters in the forms its publishing tools read, so the same tool
//! now also returns Podlove Simple Chapters as JSON and a CUE sheet. It
//! stays read-only and writes no file: the text comes back in `labels`.
//!
//! What these tests pin:
//!
//! - with no `format`, the text is byte-for-byte what it was before, in
//!   the session's own order;
//! - each chapter format, against the exact text it should produce,
//!   sorted by start, a region starting at its start;
//! - names that would break a format (quotes, newlines, tabs) come out as
//!   valid text in it;
//! - a CUE sheet counts minutes past 99 and holds at most 99 tracks;
//! - the tool still never needs approval and appends nothing.
//!
//! Sources: <https://podlove.org/simple-chapters/> (Normal Play Time, the
//! `start` and `title` attributes) and its JSON form in
//! <https://github.com/podlove/chapters> (`[{"start":"00:00:00.000",
//! "title":"Intro"}]`); the CUE commands as GNU ccd2cue documents them,
//! <https://www.gnu.org/software/ccd2cue/manual/html_node/CUE-sheet-format.html>
//! (75 frames a second, `mm` unbounded, track numbers at most 99, titles
//! of at most 80 characters, quoted).

#[path = "support/harness.rs"]
mod harness;

use harness::{marker, region, Session};
use serde_json::{json, Value};
use session::Annotation;
use tools::{ToolDispatcher, ToolResult};

type Fixture = Session;

impl Session {
    fn with(annotations: Vec<Annotation>) -> Self {
        Self::with_annotations(annotations, 0)
    }

    /// The result of an `export_labels` call that must succeed.
    fn export(&mut self, args: Value) -> Value {
        self.ok("export_labels", args)
    }

    /// The text of a call that must succeed.
    fn text(&mut self, args: Value) -> String {
        self.export(args)["labels"]
            .as_str()
            .expect("labels is a string")
            .to_string()
    }

    /// Whether the dispatcher refuses `args` against the tool's schema.
    fn schema_refuses(&mut self, args: Value) -> bool {
        self.try_call("export_labels", args).is_err()
    }
}

/// Three chapters, out of time order, one of them a region.
fn episode() -> Vec<Annotation> {
    vec![
        marker("Outro", 3725.5),
        marker("Intro", 0.0),
        region("Interview", 62.25, 1800.0),
    ]
}

// ---------------------------------------------------------------------
// The default
// ---------------------------------------------------------------------

/// The text the tool returned before `format` existed, for `episode()`:
/// the session's order, `start TAB end TAB name`, Rust's shortest float.
const AUDACITY: &str = "3725.5\t3725.5\tOutro\n0\t0\tIntro\n62.25\t1800\tInterview\n";

#[test]
fn no_format_is_the_audacity_text_byte_for_byte() {
    let mut f = Fixture::with(episode());
    let v = f.export(json!({}));
    assert_eq!(v["labels"], json!(AUDACITY));
    assert_eq!(v["count"], json!(3));
    assert_eq!(
        v["summary"],
        json!("Exported 3 label(s) in Audacity format")
    );
    assert_eq!(v["format"], json!("audacity"));

    let named = f.export(json!({ "format": "audacity" }));
    assert_eq!(named, v, "naming the default changes nothing");
}

#[test]
fn no_markers_is_empty_text_in_every_format() {
    let mut f = Fixture::with(Vec::new());
    assert_eq!(f.text(json!({})), "");
    assert_eq!(f.text(json!({ "format": "podlove_json" })), "[]\n");
    assert_eq!(
        f.text(json!({ "format": "cue" })),
        "TITLE \"audio\"\nFILE \"audio.wav\" WAVE\n"
    );
    for format in ["audacity", "podlove_json", "cue"] {
        assert_eq!(f.export(json!({ "format": format }))["count"], json!(0));
    }
}

// ---------------------------------------------------------------------
// Podlove Simple Chapters (JSON)
// ---------------------------------------------------------------------

#[test]
fn podlove_json_is_sorted_with_normal_play_times() {
    let mut f = Fixture::with(episode());
    let v = f.export(json!({ "format": "podlove_json" }));
    assert_eq!(
        v["labels"],
        json!(concat!(
            "[\n",
            "  {\n",
            "    \"start\": \"00:00:00.000\",\n",
            "    \"title\": \"Intro\"\n",
            "  },\n",
            "  {\n",
            "    \"start\": \"00:01:02.250\",\n",
            "    \"title\": \"Interview\"\n",
            "  },\n",
            "  {\n",
            "    \"start\": \"01:02:05.500\",\n",
            "    \"title\": \"Outro\"\n",
            "  }\n",
            "]\n",
        ))
    );
    assert_eq!(v["count"], json!(3));
    assert_eq!(v["format"], json!("podlove_json"));
}

#[test]
fn podlove_json_rounds_to_the_millisecond_and_keeps_ties_in_order() {
    let mut f = Fixture::with(vec![
        marker("d", 20.0004),
        marker("b", 10.0),
        marker("a", 10.0),
        marker("c", 0.0996),
    ]);
    let parsed: Value =
        serde_json::from_str(&f.text(json!({ "format": "podlove_json" }))).expect("valid JSON");
    assert_eq!(
        parsed,
        json!([
            { "start": "00:00:00.100", "title": "c" },
            { "start": "00:00:10.000", "title": "b" },
            { "start": "00:00:10.000", "title": "a" },
            { "start": "00:00:20.000", "title": "d" },
        ])
    );
}

#[test]
fn podlove_json_escapes_any_name() {
    let name = "Say \"hi\"\tthen\nback\\slash";
    let mut f = Fixture::with(vec![marker(name, 1.0)]);
    let text = f.text(json!({ "format": "podlove_json" }));
    let parsed: Value = serde_json::from_str(&text).expect("valid JSON");
    assert_eq!(
        parsed[0]["title"],
        json!(name),
        "the title survives: {text}"
    );
}

// ---------------------------------------------------------------------
// CUE sheet
// ---------------------------------------------------------------------

#[test]
fn cue_sheet_is_sorted_with_75_frames_a_second() {
    let mut f = Fixture::with(episode());
    let v = f.export(json!({ "format": "cue" }));
    assert_eq!(
        v["labels"],
        json!(concat!(
            "TITLE \"audio\"\n",
            "FILE \"audio.wav\" WAVE\n",
            "  TRACK 01 AUDIO\n",
            "    TITLE \"Intro\"\n",
            "    INDEX 01 00:00:00\n",
            "  TRACK 02 AUDIO\n",
            "    TITLE \"Interview\"\n",
            "    INDEX 01 01:02:19\n",
            "  TRACK 03 AUDIO\n",
            "    TITLE \"Outro\"\n",
            "    INDEX 01 62:05:38\n",
        ))
    );
    assert_eq!(v["count"], json!(3));
    assert_eq!(v["format"], json!("cue"));
}

#[test]
fn cue_file_name_sets_the_file_line_and_its_type() {
    let mut f = Fixture::with(vec![marker("Intro", 0.0)]);
    let head = |text: String| text.lines().take(2).collect::<Vec<_>>().join("\n");
    assert_eq!(
        head(f.text(json!({ "format": "cue", "file_name": "Episode 12.mp3" }))),
        "TITLE \"Episode 12\"\nFILE \"Episode 12.mp3\" MP3"
    );
    assert_eq!(
        head(f.text(json!({ "format": "cue", "file_name": "take.AIFF" }))),
        "TITLE \"take\"\nFILE \"take.AIFF\" AIFF"
    );
    // A FILE type is one of the CUE sheet's audio types; anything that is
    // not MP3 or AIFF is read as WAVE, the most common.
    assert_eq!(
        head(f.text(json!({ "format": "cue", "file_name": "show.flac" }))),
        "TITLE \"show\"\nFILE \"show.flac\" WAVE"
    );
    assert_eq!(
        head(f.text(json!({ "format": "cue", "file_name": "say \"cheese\".wav" }))),
        "TITLE \"say 'cheese'\"\nFILE \"say 'cheese'.wav\" WAVE"
    );
}

#[test]
fn cue_titles_have_no_quote_or_line_break_and_at_most_80_characters() {
    let long = "é".repeat(100);
    let mut f = Fixture::with(vec![
        marker("Say \"hi\"\tthen\nback\r", 1.0),
        marker(&long, 2.0),
    ]);
    let text = f.text(json!({ "format": "cue" }));
    let titles: Vec<&str> = text
        .lines()
        .filter(|l| l.starts_with("    TITLE "))
        .collect();
    assert_eq!(
        titles,
        vec![
            "    TITLE \"Say 'hi' then back \"".to_string(),
            format!("    TITLE \"{}\"", "é".repeat(80)),
        ]
    );
    assert_eq!(
        text.lines().count(),
        2 + 2 * 3,
        "one line per command: {text}"
    );
}

#[test]
fn cue_minutes_run_past_99() {
    // 2 h 2 min 2 s and 3 frames: 7,322.04 s is 549,153 frames.
    let mut f = Fixture::with(vec![marker("late", 7322.04)]);
    let text = f.text(json!({ "format": "cue" }));
    assert!(
        text.ends_with("    INDEX 01 122:02:03\n"),
        "the minutes field is not capped at 99: {text}"
    );
}

#[test]
fn cue_holds_at_most_99_tracks_and_says_what_it_left_out() {
    let mut f = Fixture::with(
        (0..101)
            .rev()
            .map(|i| marker(&format!("m{i}"), f64::from(i)))
            .collect(),
    );
    let v = f.export(json!({ "format": "cue" }));
    let text = v["labels"].as_str().expect("text");
    assert_eq!(text.matches("TRACK ").count(), 99);
    assert!(
        text.contains("  TRACK 99 AUDIO\n    TITLE \"m98\"\n"),
        "{text}"
    );
    assert!(!text.contains("\"m99\""), "{text}");
    assert_eq!(v["count"], json!(99));
    assert_eq!(v["omitted"], json!(2));
    let summary = v["summary"].as_str().expect("summary");
    assert!(
        summary.contains("the last 2") && summary.contains("99 tracks"),
        "{summary}"
    );
}

// ---------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------

#[test]
fn an_unknown_format_is_refused() {
    let mut f = Fixture::with(episode());
    assert!(f.schema_refuses(json!({ "format": "mp4chaps" })));
}

#[test]
fn file_name_belongs_to_cue_only() {
    let mut f = Fixture::with(episode());
    for format in [None, Some("audacity"), Some("podlove_json")] {
        let mut args = json!({ "file_name": "a.wav" });
        if let Some(format) = format {
            args["format"] = json!(format);
        }
        match f.call("export_labels", args.clone()) {
            ToolResult::Error(e) => assert!(e.contains("cue"), "{e}"),
            ToolResult::Ok(v) => panic!("{args} was accepted: {v}"),
        }
    }
    match f.call(
        "export_labels",
        json!({ "format": "cue", "file_name": " \n" }),
    ) {
        ToolResult::Error(e) => assert!(e.contains("file_name"), "{e}"),
        ToolResult::Ok(v) => panic!("a blank file_name was accepted: {v}"),
    }
}

// ---------------------------------------------------------------------
// Read-only
// ---------------------------------------------------------------------

#[test]
fn every_format_is_read_only_and_writes_nothing() {
    let d = ToolDispatcher::default_dispatcher();
    assert!(!d.get("export_labels").expect("registered").mutates());

    let mut f = Fixture::with(episode());
    let listing = |dir: &std::path::Path| {
        let mut names: Vec<_> = walk(dir);
        names.sort();
        names
    };
    let before_head = f.store.head();
    let before_files = listing(f.dir.path());
    for args in [
        json!({}),
        json!({ "format": "podlove_json" }),
        json!({ "format": "cue", "file_name": "episode.wav" }),
    ] {
        f.export(args);
    }
    assert_eq!(f.store.head(), before_head, "a node was appended");
    assert_eq!(listing(f.dir.path()), before_files, "a file was written");
}

fn walk(dir: &std::path::Path) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).expect("read_dir") {
        let entry = entry.expect("entry");
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            let len = entry.metadata().map_or(0, |m| m.len());
            out.push((path.display().to_string(), len));
        }
    }
    out
}
