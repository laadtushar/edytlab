# Licence of the recording in this folder

## The recording

`ami_es2004a_640_680s.wav` is 40 seconds (640 s to 680 s) of the headset mix
(`ES2004a.Mix-Headset.wav`) of meeting ES2004a of the **AMI Meeting Corpus**.

- Creator: the AMI Consortium. Copyright (c) the AMI Consortium (the corpus
  pages name no year or further notice).
- Source: the corpus' download page, <https://groups.inf.ed.ac.uk/ami/download/>;
  the file is
  <https://groups.inf.ed.ac.uk/ami/AMICorpusMirror/amicorpus/ES2004a/audio/ES2004a.Mix-Headset.wav>.
- Licence: Creative Commons Attribution 4.0 International (CC BY 4.0),
  <https://groups.inf.ed.ac.uk/ami/corpus/license.shtml>,
  <https://creativecommons.org/licenses/by/4.0/legalcode>.
- Changes: **the file was cut** to those 40 seconds and nothing else was done
  to the samples. It is not the AMI Consortium's file and they do not endorse
  this use.
- No warranty: the material is provided as is, as section 5 of the CC BY 4.0
  legal code says (disclaimer of warranties and limitation of liability);
  nothing here adds a warranty to it.

Please cite: J. Carletta et al., "The AMI meeting corpus: A pre-announcement",
International workshop on machine learning for multimodal interaction, Springer,
2005.

## The annotation

`ami_es2004a_640_680s.rttm` is the corpus' manual annotation of the same
seconds, as converted to RTTM by the Brno University of Technology's AMI
diarization setup, <https://github.com/BUTSpeechFIT/AMI-diarization-setup>
(Apache-2.0), from its file
`only_words/rttms/test/ES2004a.rttm`
(<https://github.com/BUTSpeechFIT/AMI-diarization-setup/blob/main/only_words/rttms/test/ES2004a.rttm>).
It is **cut** to the same 40 seconds, shifted so that the excerpt starts at 0
(a turn that began before 640 s starts at 0), and the recording id is changed to
`excerpt`. The annotation is the AMI Consortium's (CC BY 4.0, as above); the
RTTM file is the setup's (Apache-2.0).

`APACHE-2.0-AMI-diarization-setup.txt` is a copy of that repository's `LICENSE`
(the Apache License, Version 2.0, unmodified; the repository has no `NOTICE`
file). It covers `ami_es2004a_640_680s.rttm` and nothing else in this folder.

Please cite: F. Landini, J. Profant, M. Diez, L. Burget, "Bayesian HMM
clustering of x-vector sequences (VBx) in speaker diarization", Computer Speech &
Language, 2022.

## The derived data

The other files here (`*.f32`, `*.u8`, `*.f64`, `fixtures.json`) are derived data:
what `make_fixtures.py` made from the recording with segmentation and clustering
as in pyannote.audio 3.1 and the speaker embedding of pyannote.audio 3.0's ONNX
export (MIT, (c) 2020 CNRS) and the models `crates/ml-pipeline/models.json` pins.
`reference.py` carries pyannote.audio's MIT notice for the code it copies.
