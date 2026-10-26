# Licence of the recording in this folder

`ami_es2004a_640_680s.wav` is 40 seconds (640 s to 680 s) of the headset mix
(`ES2004a.Mix-Headset.wav`) of meeting ES2004a of the **AMI Meeting Corpus**,
copied without other change than the cut. `ami_es2004a_640_680s.rttm` is the
corpus' manual annotation of the same seconds, as converted to RTTM by the
Brno University of Technology's AMI diarization setup
(<https://github.com/BUTSpeechFIT/AMI-diarization-setup>, Apache-2.0), cut to the
same seconds and shifted so that the excerpt starts at 0.

The corpus is released under the Creative Commons Attribution 4.0
International Public License (CC BY 4.0): <https://groups.inf.ed.ac.uk/ami/corpus/license.shtml>,
<https://creativecommons.org/licenses/by/4.0/>.

Please cite: J. Carletta et al., "The AMI meeting corpus: A pre-announcement",
International workshop on machine learning for multimodal interaction, Springer,
2005. Annotation setup: F. Landini, J. Profant, M. Diez, L. Burget, "Bayesian HMM
clustering of x-vector sequences (VBx) in speaker diarization", Computer Speech &
Language, 2022.

The other files here (`*.f32`, `*.u8`, `*.f64`, `fixtures.json`) are derived data:
what `make_fixtures.py` made from the recording with the pyannote.audio 3.1
algorithm (MIT, (c) CNRS) and the models `crates/ml-pipeline/models.json` pins.
