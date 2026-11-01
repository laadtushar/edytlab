# The stem separation fixture

`vocals.flac`, `drums.flac`, `bass.flac` and `other.flac`: 8.5 s each, 16-bit
stereo, 44.1 kHz. `tests/real_model.rs` adds them up, separates the sum with
the real model, and scores each estimate against the stem it should be.
`make_stems.py` makes them from the four sources below (it checks each
source's SHA-256 first) and says how they were cut and levelled.

They are four unrelated recordings. They were not recorded together; what the
test needs is four sources that really are separate, so that the right answer
is known and is not an estimate.

## Credit and licences

The repository's own licence (MIT) covers the code. These files are other
people's recordings, kept under their own licences, which allow what is done
with them here (use, cutting, mixing and redistribution in this repository):

| file | what | source | licence |
|---|---|---|---|
| `vocals.flac` | a solo singer, "Row, Row, Row Your Boat", straight tone | VocalSet, `FULL/female9/excerpts/straight/f9_row_straight.wav`: Julia Wilkins, Prem Seetharaman, Alison Wahl and Bryan Pardo, "VocalSet: A Singing Voice Dataset", ISMIR 2018, <https://doi.org/10.5281/zenodo.1193957> | [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) (the Zenodo record says `cc-by-4.0`) |
| `drums.flac` | a hip-hop drum loop at 110 BPM | Freesound 573948, "Hip-Hop Drum Loop - 110bpm" by blakengouda, <https://freesound.org/people/blakengouda/sounds/573948/> | [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) |
| `bass.flac` | an electric bass loop at 110 BPM | Freesound 483146, "Electric bass guitar loop 1 bpm 110.wav" by josefpres, <https://freesound.org/people/josefpres/sounds/483146/> | CC0 1.0 |
| `other.flac` | a funk guitar loop at 110 BPM | Freesound 483627, "Funk guitar loop 110 bpm.wav" by josefpres, <https://freesound.org/people/josefpres/sounds/483627/> | CC0 1.0 |

The licences were read on the sources' own pages on 2026-10-27: each Freesound
page shows "Creative Commons 0" and links
`creativecommons.org/publicdomain/zero/1.0/`, and the Zenodo record's metadata
gives `cc-by-4.0`. The Freesound files are the site's "high quality" previews
(MP3, about 145 kbit/s), because those can be fetched without an account;
they are CC0 like the originals.

Changes made to the VocalSet recording (CC BY 4.0 asks that they be said):
cut to 8.5 s starting 0.3 s in, scaled to a stated level, faded in and out
over 30 ms, copied to both channels, and encoded as FLAC. The loops got the
same, cut from 0.05 s in.

Nothing here comes from MUSDB18 or any non-commercial set.
