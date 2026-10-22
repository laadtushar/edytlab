//! What the pinned ONNX export must look like, checked by name before any run.
//!
//! The decoder is fed by name (`past_key_values.3.decoder.key`) and read back
//! by name (`present.3.decoder.key`, `cross_attentions.4`). A different
//! export, such as another size, a quantised encoder with different outputs, or
//! one without the `_timestamped` attention outputs, does not fail at
//! those names with anything a person can act on: ONNX Runtime says
//! "invalid input name" from deep inside a step, or, worse, a wrong size reads
//! as a plausible transcript. So [`WhisperModel::load`](crate::WhisperModel::load)
//! lists the inputs and outputs of both sessions and calls [`check_encoder`] and
//! [`check_decoder`], which answer with one sentence about what differs
//! ([`ExportError`]).
//!
//! These are pure functions over names, element types and shapes, so CI tests
//! them on made-up lists; `tests/transcribe_known_words.rs` runs it against the
//! real files.
//!
//! ## The export, as the plan on #384 and `onnx-community/whisper-base_timestamped`
//! (`608c49e61301901684bc36cac8f74b95ff6b5a8e`) have it
//!
//! | session | inputs | outputs |
//! |---|---|---|
//! | encoder | `input_features` f32 `[batch, 80, 3000]` | `last_hidden_state` f32 `[batch, 1500, 512]` |
//! | merged decoder | `input_ids` i64 `[batch, seq]`, `encoder_hidden_states` f32 `[batch, 1500, 512]`, `past_key_values.{i}.{decoder,encoder}.{key,value}` f32 `[batch, 8, past, 64]` for 6 layers, `use_cache_branch` bool `[1]` | `logits` f32 `[batch, seq, 51865]`, `present.{i}.{decoder,encoder}.{key,value}`, `cross_attentions.{i}` f32 `[batch, 8, seq, 1500]` |

use std::collections::BTreeSet;

/// Why a session is not the pinned export's. The message names the file and
/// says what differs.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExportError {
    /// The encoder's inputs, outputs, element types or shapes differ.
    #[error("{0}")]
    Encoder(String),
    /// The decoder's differ, or it does not take what the encoder gives.
    #[error("{0}")]
    Decoder(String),
}

/// The element types the check cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elem {
    F32,
    I64,
    Bool,
    Other,
}

/// One input or output of a session: its name, element type and shape (`-1`
/// for a dimension that is not fixed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TensorSpec {
    pub name: String,
    pub elem: Elem,
    pub dims: Vec<i64>,
}

impl TensorSpec {
    /// A spec from a name, an element type and a shape (`-1` for a dimension
    /// that is not fixed).
    pub fn new(name: &str, elem: Elem, dims: &[i64]) -> Self {
        Self {
            name: name.to_string(),
            elem,
            dims: dims.to_vec(),
        }
    }
}

/// The geometry of the decoder, read from its inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportLayout {
    /// Decoder layers (6 for `base`).
    pub layers: usize,
    /// Attention heads per layer (8).
    pub heads: usize,
    /// Width of one head (64).
    pub head_dim: usize,
    /// Width of the encoder's output and the decoder's model (512).
    pub width: usize,
    /// Frames the encoder emits for a 30 s window (1500).
    pub frames: usize,
    /// Width of the logits (51 865).
    pub vocab: usize,
}

/// Mel bands the encoder takes.
const MEL_BANDS: i64 = 80;
/// Mel frames of a 30 s window.
const MEL_FRAMES: i64 = 3_000;
/// Frames the encoder emits for them: half as many.
const ENCODER_FRAMES: i64 = 1_500;

/// The `(layer, head)` pairs whose cross-attention lines tokens up with time,
/// from `generation_config.json` of `whisper-base` (`alignment_heads`). The
/// file is not one of the three the plan pins, so the list is here, as the
/// suppress lists in `decode.rs` are.
pub const BASE_ALIGNMENT_HEADS: [(usize, usize); 8] = [
    (3, 1),
    (4, 2),
    (4, 3),
    (4, 7),
    (5, 1),
    (5, 2),
    (5, 4),
    (5, 6),
];

/// The alignment heads of the export with this geometry, or `None` if this
/// build does not know them (only `base`: 6 layers of 8 heads).
pub fn alignment_heads(layers: usize, heads: usize) -> Option<&'static [(usize, usize)]> {
    (layers == 6 && heads == 8).then_some(&BASE_ALIGNMENT_HEADS[..])
}

/// Which of the two cache tensors a name is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cache {
    /// `past_key_values.*`, an input: what the decoder has computed so far.
    Past,
    /// `present.*`, an output: the cache after this step.
    Present,
}

/// Which attention a cache belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Self-attention over the tokens so far; one position longer each step.
    Decoder,
    /// Cross-attention over the encoder's output; the first step's, unchanged.
    Encoder,
}

/// The key or the value of an attention cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Key,
    Value,
}

/// `past_key_values.{layer}.{decoder|encoder}.{key|value}` or
/// `present.{layer}.{decoder|encoder}.{key|value}`.
pub fn cache_name(cache: Cache, layer: usize, side: Side, part: Part) -> String {
    format!(
        "{}.{layer}.{}.{}",
        match cache {
            Cache::Past => "past_key_values",
            Cache::Present => "present",
        },
        match side {
            Side::Decoder => "decoder",
            Side::Encoder => "encoder",
        },
        match part {
            Part::Key => "key",
            Part::Value => "value",
        }
    )
}

/// `cross_attentions.{layer}`.
pub fn cross_name(layer: usize) -> String {
    format!("cross_attentions.{layer}")
}

fn names(specs: &[TensorSpec]) -> BTreeSet<&str> {
    specs.iter().map(|s| s.name.as_str()).collect()
}

fn list(items: impl IntoIterator<Item = impl AsRef<str>>) -> String {
    let mut shown: Vec<String> = items
        .into_iter()
        .map(|s| format!("`{}`", s.as_ref()))
        .collect();
    let more = shown.len().saturating_sub(4);
    shown.truncate(4);
    let mut text = shown.join(", ");
    if more > 0 {
        text.push_str(&format!(" and {more} more"));
    }
    text
}

/// Check the encoder's lists against the export, naming what differs.
pub fn check_encoder(inputs: &[TensorSpec], outputs: &[TensorSpec]) -> Result<(), ExportError> {
    let wrong = |what: String| Err(ExportError::Encoder(format!("encoder_model.onnx {what}")));
    let [input] = inputs else {
        return wrong(format!(
            "takes {} inputs ({}), expected only `input_features`",
            inputs.len(),
            list(names(inputs))
        ));
    };
    if input.name != "input_features" || input.elem != Elem::F32 || input.dims.len() != 3 {
        return wrong(format!(
            "input is `{}` ({:?}, rank {}), expected `input_features` as a rank 3 f32 tensor",
            input.name,
            input.elem,
            input.dims.len()
        ));
    }
    let Some(out) = outputs.iter().find(|o| o.name == "last_hidden_state") else {
        return wrong(format!(
            "has no output `last_hidden_state`; its outputs are {}",
            list(names(outputs))
        ));
    };
    if out.elem != Elem::F32 || out.dims.len() != 3 {
        return wrong(format!(
            "`last_hidden_state` is {:?} of rank {}, expected a rank 3 f32 tensor",
            out.elem,
            out.dims.len()
        ));
    }
    // The mel transform makes `[1, 80, 3000]` and the decoder and the timing
    // work in the 1500 frames that come out. A dimension the export leaves
    // dynamic (`-1`; the pinned encoder does for its input) cannot be checked
    // here, but one it fixes has to be the pinned number.
    let fixed = |dims: &[i64], at: usize, name: &str, want: i64| -> Result<(), ExportError> {
        match dims.get(at) {
            Some(&d) if d != want && d >= 0 => Err(ExportError::Encoder(format!(
                "encoder_model.onnx has {name} of {d}, expected {want} (a 30 s window of the \
                 pinned export)"
            ))),
            _ => Ok(()),
        }
    };
    fixed(&input.dims, 1, "`input_features` mel bands", MEL_BANDS)?;
    fixed(&input.dims, 2, "`input_features` frames", MEL_FRAMES)?;
    fixed(&out.dims, 1, "`last_hidden_state` frames", ENCODER_FRAMES)?;
    Ok(())
}

/// Check the decoder's lists against the export and read its geometry.
///
/// `encoder_output_dims` are the dims of the encoder's `last_hidden_state`;
/// the decoder has to take what the encoder gives.
pub fn check_decoder(
    inputs: &[TensorSpec],
    outputs: &[TensorSpec],
    encoder_output_dims: &[i64],
) -> Result<ExportLayout, ExportError> {
    let wrong = |what: String| {
        Err(ExportError::Decoder(format!(
            "decoder_model_merged_int8.onnx {what}"
        )))
    };
    let input_names = names(inputs);
    let output_names = names(outputs);

    // The layers, from the decoder keys it takes.
    let mut layers = 0usize;
    while input_names.contains(cache_name(Cache::Past, layers, Side::Decoder, Part::Key).as_str()) {
        layers += 1;
    }
    if layers == 0 {
        return wrong(format!(
            "has no input `past_key_values.0.decoder.key`, so it is not a merged decoder with a \
             key-value cache; its inputs are {}",
            list(&input_names)
        ));
    }

    let mut expected_in: BTreeSet<String> =
        ["input_ids", "encoder_hidden_states", "use_cache_branch"]
            .iter()
            .map(|s| s.to_string())
            .collect();
    let mut expected_out: BTreeSet<String> = BTreeSet::from(["logits".to_string()]);
    for layer in 0..layers {
        for side in [Side::Decoder, Side::Encoder] {
            for part in [Part::Key, Part::Value] {
                expected_in.insert(cache_name(Cache::Past, layer, side, part));
                expected_out.insert(cache_name(Cache::Present, layer, side, part));
            }
        }
        expected_out.insert(cross_name(layer));
    }
    let missing_in: Vec<_> = expected_in
        .iter()
        .filter(|n| !input_names.contains(n.as_str()))
        .collect();
    let extra_in: Vec<_> = input_names
        .iter()
        .filter(|n| !expected_in.contains(**n))
        .collect();
    if !missing_in.is_empty() || !extra_in.is_empty() {
        return wrong(format!(
            "has inputs that differ from the export: missing {}, unexpected {}",
            if missing_in.is_empty() {
                "none".to_string()
            } else {
                list(missing_in)
            },
            if extra_in.is_empty() {
                "none".to_string()
            } else {
                list(extra_in)
            }
        ));
    }
    let missing_out: Vec<_> = expected_out
        .iter()
        .filter(|n| !output_names.contains(n.as_str()))
        .collect();
    if !missing_out.is_empty() {
        let hint = if missing_out
            .iter()
            .any(|n| n.starts_with("cross_attentions"))
        {
            " (the `_timestamped` export has the cross-attention outputs word timing needs)"
        } else {
            ""
        };
        return wrong(format!("has no output {}{hint}", list(missing_out)));
    }

    // Every name used below is in `expected_in` or `expected_out`, and the two
    // checks above returned if any of those was missing, so a lookup failing is
    // not something this function can reach; it is an error all the same.
    let find = |specs: &'_ [TensorSpec], name: &str| -> Result<TensorSpec, ExportError> {
        specs
            .iter()
            .find(|s| s.name == name)
            .cloned()
            .ok_or_else(|| {
                ExportError::Decoder(format!("decoder_model_merged_int8.onnx has no `{name}`"))
            })
    };
    let input = |name: &str| find(inputs, name);
    let output = |name: &str| find(outputs, name);

    // Element types.
    for (name, want) in [
        ("input_ids", Elem::I64),
        ("encoder_hidden_states", Elem::F32),
        ("use_cache_branch", Elem::Bool),
    ] {
        let found = input(name)?.elem;
        if found != want {
            return wrong(format!("takes `{name}` as {found:?}, expected {want:?}"));
        }
    }
    let logits = output("logits")?;
    if logits.elem != Elem::F32 {
        return wrong(format!(
            "returns `logits` as {:?}, expected F32",
            logits.elem
        ));
    }

    // Geometry, from the first layer's decoder key `[batch, heads, past, head_dim]`.
    let key = input("past_key_values.0.decoder.key")?;
    let (heads, head_dim) = match key.dims.as_slice() {
        [_, h, _, d] if *h > 0 && *d > 0 => (*h as usize, *d as usize),
        other => {
            return wrong(format!(
                "has `past_key_values.0.decoder.key` of shape {other:?}, expected \
                 [batch, heads, past, head_dim] with fixed heads and head_dim"
            ))
        }
    };
    let hidden = input("encoder_hidden_states")?;
    let width = match hidden.dims.as_slice() {
        [_, _, w] if *w > 0 => *w as usize,
        other => {
            return wrong(format!(
                "has `encoder_hidden_states` of shape {other:?}, expected [batch, frames, width]"
            ))
        }
    };
    if width != heads * head_dim {
        return wrong(format!(
            "has {heads} heads of {head_dim} but a model width of {width}"
        ));
    }
    let vocab = match logits.dims.as_slice() {
        [_, _, v] if *v > 0 => *v as usize,
        other => {
            return wrong(format!(
                "returns `logits` of shape {other:?}, expected [batch, seq, vocab]"
            ))
        }
    };
    let frames = match encoder_output_dims {
        [_, f, w] if *f == ENCODER_FRAMES && *w as usize == width => *f as usize,
        other => {
            return Err(ExportError::Encoder(format!(
                "encoder_model.onnx gives `last_hidden_state` of shape {other:?}, but the \
                 decoder takes a width of {width} (expected [batch, {ENCODER_FRAMES}, {width}])"
            )))
        }
    };
    if alignment_heads(layers, heads).is_none() {
        return wrong(format!(
            "has {layers} layers of {heads} heads, and this build knows the alignment heads of \
             whisper-base (6 layers of 8 heads) only"
        ));
    }
    Ok(ExportLayout {
        layers,
        heads,
        head_dim,
        width,
        frames,
        vocab,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const KV: [i64; 4] = [-1, 8, -1, 64];

    /// The lists of the real `whisper-base_timestamped` export, as `ort`
    /// printed them (see the PR for #384).
    fn base_decoder() -> (Vec<TensorSpec>, Vec<TensorSpec>) {
        let mut inputs = vec![
            TensorSpec::new("input_ids", Elem::I64, &[-1, -1]),
            TensorSpec::new("encoder_hidden_states", Elem::F32, &[-1, -1, 512]),
        ];
        let mut outputs = vec![TensorSpec::new("logits", Elem::F32, &[-1, -1, 51_865])];
        for layer in 0..6 {
            for side in [Side::Decoder, Side::Encoder] {
                for part in [Part::Key, Part::Value] {
                    inputs.push(TensorSpec::new(
                        &cache_name(Cache::Past, layer, side, part),
                        Elem::F32,
                        &KV,
                    ));
                    outputs.push(TensorSpec::new(
                        &cache_name(Cache::Present, layer, side, part),
                        Elem::F32,
                        &KV,
                    ));
                }
            }
            outputs.push(TensorSpec::new(
                &cross_name(layer),
                Elem::F32,
                &[-1, 8, -1, -1],
            ));
        }
        inputs.push(TensorSpec::new("use_cache_branch", Elem::Bool, &[1]));
        (inputs, outputs)
    }

    fn base_encoder() -> (Vec<TensorSpec>, Vec<TensorSpec>) {
        (
            vec![TensorSpec::new("input_features", Elem::F32, &[-1, -1, -1])],
            vec![TensorSpec::new(
                "last_hidden_state",
                Elem::F32,
                &[-1, 1500, 512],
            )],
        )
    }

    #[test]
    fn the_pinned_export_passes_and_gives_its_geometry() {
        let (i, o) = base_encoder();
        check_encoder(&i, &o).expect("encoder");
        let (di, dos) = base_decoder();
        let layout = check_decoder(&di, &dos, &[-1, 1500, 512]).expect("decoder");
        assert_eq!(
            layout,
            ExportLayout {
                layers: 6,
                heads: 8,
                head_dim: 64,
                width: 512,
                frames: 1500,
                vocab: 51_865,
            }
        );
    }

    #[test]
    fn a_decoder_without_use_cache_branch_names_it() {
        let (mut di, dos) = base_decoder();
        di.retain(|s| s.name != "use_cache_branch");
        let err = check_decoder(&di, &dos, &[-1, 1500, 512])
            .unwrap_err()
            .to_string();
        assert!(err.contains("`use_cache_branch`"), "{err}");
        assert!(err.contains("missing"), "{err}");
        assert!(err.starts_with("decoder_model_merged_int8.onnx"), "{err}");
    }

    #[test]
    fn a_decoder_with_an_input_it_does_not_feed_is_refused() {
        let (mut di, dos) = base_decoder();
        di.push(TensorSpec::new("position_ids", Elem::I64, &[-1, -1]));
        let err = check_decoder(&di, &dos, &[-1, 1500, 512])
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("unexpected") && err.contains("`position_ids`"),
            "{err}"
        );
    }

    #[test]
    fn an_export_without_the_attention_outputs_says_what_to_get() {
        let (di, mut dos) = base_decoder();
        dos.retain(|s| !s.name.starts_with("cross_attentions"));
        let err = check_decoder(&di, &dos, &[-1, 1500, 512])
            .unwrap_err()
            .to_string();
        assert!(err.contains("cross_attentions.0"), "{err}");
        assert!(err.contains("_timestamped"), "{err}");
    }

    #[test]
    fn an_unmerged_decoder_is_not_mistaken_for_the_export() {
        let inputs = vec![
            TensorSpec::new("input_ids", Elem::I64, &[-1, -1]),
            TensorSpec::new("encoder_hidden_states", Elem::F32, &[-1, -1, 512]),
        ];
        let outputs = vec![TensorSpec::new("logits", Elem::F32, &[-1, -1, 51_865])];
        let err = check_decoder(&inputs, &outputs, &[-1, 1500, 512])
            .unwrap_err()
            .to_string();
        assert!(err.contains("merged decoder"), "{err}");
    }

    #[test]
    fn another_size_is_refused_because_its_alignment_heads_are_unknown() {
        // whisper-tiny: 4 layers of 6 heads of 64, width 384.
        let mut inputs = vec![
            TensorSpec::new("input_ids", Elem::I64, &[-1, -1]),
            TensorSpec::new("encoder_hidden_states", Elem::F32, &[-1, -1, 384]),
            TensorSpec::new("use_cache_branch", Elem::Bool, &[1]),
        ];
        let mut outputs = vec![TensorSpec::new("logits", Elem::F32, &[-1, -1, 51_865])];
        for layer in 0..4 {
            for side in [Side::Decoder, Side::Encoder] {
                for part in [Part::Key, Part::Value] {
                    inputs.push(TensorSpec::new(
                        &cache_name(Cache::Past, layer, side, part),
                        Elem::F32,
                        &[-1, 6, -1, 64],
                    ));
                    outputs.push(TensorSpec::new(
                        &cache_name(Cache::Present, layer, side, part),
                        Elem::F32,
                        &[-1, 6, -1, 64],
                    ));
                }
            }
            outputs.push(TensorSpec::new(
                &cross_name(layer),
                Elem::F32,
                &[-1, 6, -1, -1],
            ));
        }
        let err = check_decoder(&inputs, &outputs, &[-1, 1500, 384])
            .unwrap_err()
            .to_string();
        assert!(err.contains("4 layers of 6 heads"), "{err}");
        assert!(err.contains("whisper-base"), "{err}");
    }

    #[test]
    fn a_decoder_that_does_not_take_what_the_encoder_gives_is_refused() {
        let (di, dos) = base_decoder();
        let err = check_decoder(&di, &dos, &[-1, 1500, 768])
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("encoder_model.onnx") && err.contains("768"),
            "{err}"
        );
    }

    #[test]
    fn fixed_dimensions_that_are_not_the_pinned_ones_are_refused() {
        // Dynamic ones (-1) pass, as the pinned encoder's input does.
        let (_, out) = base_encoder();
        let with_input = |dims: &[i64]| vec![TensorSpec::new("input_features", Elem::F32, dims)];
        assert!(check_encoder(&with_input(&[1, 80, 3000]), &out).is_ok());
        assert!(check_encoder(&with_input(&[-1, -1, -1]), &out).is_ok());
        let err = check_encoder(&with_input(&[-1, 128, -1]), &out)
            .unwrap_err()
            .to_string();
        assert!(err.contains("128") && err.contains("mel bands"), "{err}");
        let err = check_encoder(&with_input(&[-1, 80, 1500]), &out)
            .unwrap_err()
            .to_string();
        assert!(err.contains("1500") && err.contains("frames"), "{err}");

        let (inp, _) = base_encoder();
        let other = vec![TensorSpec::new(
            "last_hidden_state",
            Elem::F32,
            &[-1, 750, 512],
        )];
        let err = check_encoder(&inp, &other).unwrap_err().to_string();
        assert!(err.contains("750") && err.contains("1500"), "{err}");

        // The decoder refuses an encoder whose frames are not 1500 too, or
        // whose frames are not fixed.
        let (di, dos) = base_decoder();
        assert!(check_decoder(&di, &dos, &[-1, 750, 512]).is_err());
        assert!(check_decoder(&di, &dos, &[-1, -1, 512]).is_err());
    }

    #[test]
    fn an_encoder_with_other_names_is_refused() {
        let inputs = vec![TensorSpec::new("mel", Elem::F32, &[-1, 80, 3000])];
        let (_, outputs) = base_encoder();
        let err = check_encoder(&inputs, &outputs).unwrap_err().to_string();
        assert!(err.contains("input_features"), "{err}");

        let (inputs, _) = base_encoder();
        let outputs = vec![TensorSpec::new("hidden", Elem::F32, &[-1, 1500, 512])];
        let err = check_encoder(&inputs, &outputs).unwrap_err().to_string();
        assert!(
            err.contains("last_hidden_state") && err.contains("`hidden`"),
            "{err}"
        );
    }

    #[test]
    fn names_follow_the_export() {
        let name = |cache, layer, side, part| cache_name(cache, layer, side, part);
        assert_eq!(
            name(Cache::Past, 3, Side::Decoder, Part::Key),
            "past_key_values.3.decoder.key"
        );
        assert_eq!(
            name(Cache::Past, 0, Side::Encoder, Part::Value),
            "past_key_values.0.encoder.value"
        );
        assert_eq!(
            name(Cache::Present, 5, Side::Encoder, Part::Key),
            "present.5.encoder.key"
        );
        assert_eq!(cross_name(4), "cross_attentions.4");
    }

    #[test]
    fn only_base_has_known_alignment_heads() {
        assert_eq!(alignment_heads(6, 8).map(<[_]>::len), Some(8));
        assert!(alignment_heads(4, 6).is_none());
    }
}
