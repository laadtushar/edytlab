//! Tags on an exported file (#170).
//!
//! A podcast episode used to ship with no title, artist or album,
//! because neither encoder wrote any: `write_flac` emitted no Vorbis
//! comment block and `write_mp3` no ID3v2. Every export therefore
//! needed a second tool to finish it.
//!
//! ## Two containers, two mechanisms
//!
//! **FLAC** carries metadata as blocks in the stream, and `flac-codec`
//! can rewrite them in place after the fact — so tags are applied to
//! the finished file rather than threaded through the encoder, and the
//! audio is untouched by construction.
//!
//! **MP3** has no container: an ID3v2 tag is simply prepended to the
//! frames, and every decoder skips it by reading the length out of the
//! header. Written here by hand rather than with a crate — ID3v2.3 is
//! ten lines of framing and a synchsafe integer, and the alternative
//! was another dependency for that.
//!
//! **WAV** is deliberately not covered. Its tag containers (`LIST INFO`,
//! or an ID3 chunk) are inconsistently read, and the format's job in
//! this app is to be the lossless intermediate rather than the thing
//! anyone ships.

use std::path::Path;

use crate::{Error, Result};

/// What a person would want written on a file they are sending
/// somewhere. Every field is optional; absent ones are simply not
/// written rather than written empty.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// Four-digit year. Kept as a string because that is what both
    /// containers store and because "1998" and "1998-03" are both
    /// things people write.
    pub year: Option<String>,
    pub comment: Option<String>,
    /// Named positions in the audio, for a podcast's chapters. Seconds
    /// from the start, with the name the marker carried.
    pub chapters: Vec<Chapter>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chapter {
    pub start_sec: f64,
    pub title: String,
}

impl Tags {
    /// True when there is nothing to write. Callers skip the whole
    /// tagging step rather than write an empty tag, which some players
    /// display as a blank title.
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.artist.is_none()
            && self.album.is_none()
            && self.year.is_none()
            && self.comment.is_none()
            && self.chapters.is_empty()
    }
}

// ---------------------------------------------------------------------------
// FLAC — Vorbis comments, written into the finished file
// ---------------------------------------------------------------------------

/// Field names are the Xiph recommendations, which is what every player
/// looks for. `DESCRIPTION` rather than `COMMENT` for the same reason.
fn vorbis_fields(tags: &Tags) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    if let Some(v) = &tags.title {
        out.push(("TITLE", v.clone()));
    }
    if let Some(v) = &tags.artist {
        out.push(("ARTIST", v.clone()));
    }
    if let Some(v) = &tags.album {
        out.push(("ALBUM", v.clone()));
    }
    if let Some(v) = &tags.year {
        out.push(("DATE", v.clone()));
    }
    if let Some(v) = &tags.comment {
        out.push(("DESCRIPTION", v.clone()));
    }
    // Chapters as CHAPTERnnn / CHAPTERnnnNAME, the de-facto convention
    // Vorbis has for them. Not a standard, but it is the one thing
    // that reads them back.
    for (i, ch) in tags.chapters.iter().enumerate() {
        let key = format!("CHAPTER{:03}", i + 1);
        out.push((
            Box::leak(key.clone().into_boxed_str()),
            timestamp(ch.start_sec),
        ));
        out.push((
            Box::leak(format!("{key}NAME").into_boxed_str()),
            ch.title.clone(),
        ));
    }
    out
}

/// `HH:MM:SS.mmm`, which is what the chapter convention expects.
fn timestamp(seconds: f64) -> String {
    let total_ms = (seconds.max(0.0) * 1000.0).round() as u64;
    let ms = total_ms % 1000;
    let total_s = total_ms / 1000;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        total_s / 3600,
        (total_s % 3600) / 60,
        total_s % 60,
        ms
    )
}

/// Write `tags` into an existing FLAC file.
///
/// The audio is not re-encoded: only the metadata blocks are rewritten,
/// so tagging a file cannot change how it sounds.
pub fn tag_flac(path: &Path, tags: &Tags) -> Result<()> {
    use flac_codec::metadata::VorbisComment;

    if tags.is_empty() {
        return Ok(());
    }

    flac_codec::metadata::update::<_, flac_codec::Error>(path, |blocks| {
        blocks.update::<VorbisComment>(|comment| {
            for (key, value) in vorbis_fields(tags) {
                comment.insert(key, value);
            }
        });
        Ok(())
    })
    .map(|_| ())
    .map_err(|e| Error::Encode(format!("failed to tag {}: {e}", path.display())))
}

/// Read the Vorbis comments back out of a FLAC file.
///
/// Exists so a caller can check what is actually on a file rather than
/// what it believes it wrote — which is the difference between a test
/// that proves something and one that restates the code.
pub fn read_flac_tags(path: &Path) -> Result<Vec<(String, String)>> {
    use flac_codec::metadata::VorbisComment;

    let comment: Option<VorbisComment> = flac_codec::metadata::block(path)
        .map_err(|e| Error::Encode(format!("failed to read {}: {e}", path.display())))?;

    Ok(comment
        .map(|c| {
            c.fields
                .iter()
                .filter_map(|f| f.split_once('='))
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        })
        .unwrap_or_default())
}

// ---------------------------------------------------------------------------
// MP3 — an ID3v2.3 tag, prepended
// ---------------------------------------------------------------------------

/// ID3v2 sizes are "synchsafe": seven bits per byte, so the length can
/// never contain a byte that looks like the start of an MP3 frame.
fn synchsafe(mut n: u32) -> [u8; 4] {
    let mut out = [0u8; 4];
    for i in (0..4).rev() {
        out[i] = (n & 0x7f) as u8;
        n >>= 7;
    }
    out
}

/// One ID3v2.3 text frame: four-byte id, four-byte size, two flag
/// bytes, then an encoding byte and the text.
fn text_frame(id: &[u8; 4], text: &str) -> Vec<u8> {
    // Encoding 3 is UTF-8. ID3v2.3 nominally allows only Latin-1 and
    // UTF-16, but UTF-8 is universally read and the alternative is
    // silently mangling any title with an accent in it.
    let mut payload = vec![3u8];
    payload.extend_from_slice(text.as_bytes());
    payload.push(0);

    let mut frame = Vec::with_capacity(10 + payload.len());
    frame.extend_from_slice(id);
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(&[0, 0]);
    frame.extend_from_slice(&payload);
    frame
}

/// A COMM frame, which needs a language and a description before its
/// text.
fn comment_frame(text: &str) -> Vec<u8> {
    let mut payload = vec![3u8];
    payload.extend_from_slice(b"eng");
    payload.push(0); // empty short description
    payload.extend_from_slice(text.as_bytes());
    payload.push(0);

    let mut frame = Vec::with_capacity(10 + payload.len());
    frame.extend_from_slice(b"COMM");
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(&[0, 0]);
    frame.extend_from_slice(&payload);
    frame
}

/// Build the complete ID3v2.3 tag for `tags`, or `None` when there is
/// nothing to say.
pub fn id3v2_tag(tags: &Tags) -> Option<Vec<u8>> {
    if tags.is_empty() {
        return None;
    }

    let mut frames = Vec::new();
    if let Some(v) = &tags.title {
        frames.extend(text_frame(b"TIT2", v));
    }
    if let Some(v) = &tags.artist {
        frames.extend(text_frame(b"TPE1", v));
    }
    if let Some(v) = &tags.album {
        frames.extend(text_frame(b"TALB", v));
    }
    if let Some(v) = &tags.year {
        frames.extend(text_frame(b"TYER", v));
    }
    if let Some(v) = &tags.comment {
        frames.extend(comment_frame(v));
    }
    // Chapters are CHAP frames in ID3v2.3+, which players that support
    // podcasts read. Each carries its own embedded TIT2.
    for (i, ch) in tags.chapters.iter().enumerate() {
        frames.extend(chap_frame(i, ch, tags.chapters.get(i + 1)));
    }

    let mut out = Vec::with_capacity(10 + frames.len());
    out.extend_from_slice(b"ID3");
    out.extend_from_slice(&[3, 0]); // v2.3.0
    out.push(0); // no flags
    out.extend_from_slice(&synchsafe(frames.len() as u32));
    out.extend(frames);
    Some(out)
}

/// A CHAP frame: an element id, start and end times in milliseconds,
/// start and end byte offsets (unknown, so all-ones), and a nested
/// TIT2 holding the chapter's name.
fn chap_frame(index: usize, chapter: &Chapter, next: Option<&Chapter>) -> Vec<u8> {
    let start_ms = (chapter.start_sec.max(0.0) * 1000.0).round() as u32;
    // The last chapter runs to the end, which is not known here; the
    // spec's "unknown" is all-ones and players treat it as such.
    let end_ms = next
        .map(|n| (n.start_sec.max(0.0) * 1000.0).round() as u32)
        .unwrap_or(u32::MAX);

    let mut payload = Vec::new();
    payload.extend_from_slice(format!("ch{index}").as_bytes());
    payload.push(0);
    payload.extend_from_slice(&start_ms.to_be_bytes());
    payload.extend_from_slice(&end_ms.to_be_bytes());
    payload.extend_from_slice(&u32::MAX.to_be_bytes());
    payload.extend_from_slice(&u32::MAX.to_be_bytes());
    payload.extend(text_frame(b"TIT2", &chapter.title));

    let mut frame = Vec::with_capacity(10 + payload.len());
    frame.extend_from_slice(b"CHAP");
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(&[0, 0]);
    frame.extend_from_slice(&payload);
    frame
}

/// Prepend an ID3v2 tag to an existing MP3.
///
/// The frames are untouched — a tag is a prefix, and every decoder
/// skips it by reading the length out of the header.
pub fn tag_mp3(path: &Path, tags: &Tags) -> Result<()> {
    let Some(tag) = id3v2_tag(tags) else {
        return Ok(());
    };
    let audio = std::fs::read(path)?;
    let mut out = tag;
    out.extend_from_slice(&audio);
    std::fs::write(path, out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full() -> Tags {
        Tags {
            title: Some("Episode 12".into()),
            artist: Some("A Podcast".into()),
            album: Some("Season 2".into()),
            year: Some("2026".into()),
            comment: Some("recorded in one take".into()),
            chapters: vec![
                Chapter {
                    start_sec: 0.0,
                    title: "Intro".into(),
                },
                Chapter {
                    start_sec: 92.5,
                    title: "The interview".into(),
                },
            ],
        }
    }

    #[test]
    fn nothing_to_say_writes_no_tag() {
        assert!(Tags::default().is_empty());
        assert!(id3v2_tag(&Tags::default()).is_none());
    }

    /// The length in an ID3 header is synchsafe so it can never contain
    /// a byte a decoder would mistake for the start of a frame.
    #[test]
    fn the_header_length_is_synchsafe_and_correct() {
        let tag = id3v2_tag(&full()).expect("tags");
        assert_eq!(&tag[0..3], b"ID3");
        assert_eq!(tag[3], 3, "version 2.3");

        let size = &tag[6..10];
        for b in size {
            assert_eq!(b & 0x80, 0, "a synchsafe byte never has the top bit set");
        }
        let declared = ((size[0] as usize) << 21)
            | ((size[1] as usize) << 14)
            | ((size[2] as usize) << 7)
            | (size[3] as usize);
        assert_eq!(
            declared,
            tag.len() - 10,
            "the declared size must be the frames, excluding the 10-byte header"
        );
    }

    #[test]
    fn the_frames_a_player_looks_for_are_present() {
        let tag = id3v2_tag(&full()).expect("tags");
        for id in [b"TIT2", b"TPE1", b"TALB", b"TYER", b"COMM", b"CHAP"] {
            assert!(
                tag.windows(4).any(|w| w == id),
                "missing {} frame",
                String::from_utf8_lossy(id)
            );
        }
        assert!(
            String::from_utf8_lossy(&tag).contains("Episode 12"),
            "the title has to survive into the bytes"
        );
    }

    /// A title with an accent in it must not be mangled, which is why
    /// the encoding byte says UTF-8.
    #[test]
    fn text_is_utf8_and_says_so() {
        let tags = Tags {
            title: Some("Café — pt.2".into()),
            ..Tags::default()
        };
        let tag = id3v2_tag(&tags).expect("tags");
        let pos = tag.windows(4).position(|w| w == b"TIT2").expect("TIT2");
        assert_eq!(tag[pos + 10], 3, "encoding byte 3 is UTF-8");
        assert!(String::from_utf8_lossy(&tag).contains("Café — pt.2"));
    }

    /// A chapter's end is the next chapter's start, and the last one
    /// runs to an unknown end rather than to zero.
    #[test]
    fn chapters_run_from_one_to_the_next() {
        let tag = id3v2_tag(&full()).expect("tags");
        let pos = tag.windows(4).position(|w| w == b"CHAP").expect("CHAP");
        // element id "ch0" + NUL, then start and end.
        let payload = &tag[pos + 10..];
        let after_id = payload.iter().position(|&b| b == 0).unwrap() + 1;
        let start = u32::from_be_bytes(payload[after_id..after_id + 4].try_into().unwrap());
        let end = u32::from_be_bytes(payload[after_id + 4..after_id + 8].try_into().unwrap());
        assert_eq!(start, 0);
        assert_eq!(
            end, 92_500,
            "the first chapter ends where the second starts"
        );
    }

    #[test]
    fn a_timestamp_reads_as_a_time() {
        assert_eq!(timestamp(0.0), "00:00:00.000");
        assert_eq!(timestamp(92.5), "00:01:32.500");
        assert_eq!(timestamp(3661.25), "01:01:01.250");
    }
}
