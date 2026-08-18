//! Dispatcher: trait, registry, and per-call context.

use std::collections::HashMap;

use jsonschema::JSONSchema;
use serde_json::Value;

use crate::{DispatchError, Result, ToolResult};

/// Mutable references handed to tools for the duration of a single
/// invocation. Phase 1 carries the session store and the audio engine;
/// later phases may add caches, logging sinks, etc.
///
/// The lifetime parameter ties the borrows to the caller â€” tools must
/// not stash these references beyond the call.
pub struct ToolContext<'a> {
    pub store: &'a mut session::Store,
    pub engine: &'a mut audio_engine::Engine,
    pub user_message: &'a str,
    /// In-memory audio clipboard shared between `copy_region` and
    /// `paste_region`. Held behind a mutable reference so both tools
    /// can read/write without cloning large sample buffers.
    pub clipboard: &'a mut Option<Vec<f32>>,
}

/// A single tool exposed to the model.
///
/// Implementations must return a stable canonical [`Tool::name`] and a
/// full Anthropic-shaped [`Tool::schema`] (name + description +
/// input_schema). The dispatcher validates the `args` JSON against
/// `schema().input_schema` before calling [`Tool::invoke`].
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;

    /// Returns the tool descriptor in the Anthropic tool-use format:
    /// `{ "name": ..., "description": ..., "input_schema": { ... } }`.
    fn schema(&self) -> Value;

    /// Invoked with `args` already validated against `input_schema`.
    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> Result<ToolResult>;
}

/// A tool plus its precompiled `input_schema` validator.
///
/// We compile the JSON schema once at [`ToolDispatcher::register`] time
/// and reuse it on every dispatch â€” schema compilation is comparatively
/// expensive and the schema is stable for the lifetime of the tool.
///
/// `compiled_schema` is `Err(reason)` when the tool's advertised schema
/// is malformed (missing `input_schema`, non-object, or fails to
/// compile). The error is surfaced from [`ToolDispatcher::invoke`] as
/// [`DispatchError::MalformedToolSchema`] so the panic-free API stays
/// panic-free.
struct Registered {
    tool: Box<dyn Tool>,
    compiled_schema: std::result::Result<JSONSchema, String>,
}

/// Registry of tools keyed by canonical name.
///
/// Use [`register`](ToolDispatcher::register) to add tools at startup,
/// then [`invoke`](ToolDispatcher::invoke) per model tool call. Use
/// [`tool_schemas`](ToolDispatcher::tool_schemas) when constructing the
/// `tools` parameter for the Anthropic API.
#[derive(Default)]
pub struct ToolDispatcher {
    tools: HashMap<String, Registered>,
}

impl ToolDispatcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct a dispatcher pre-populated with the default tool set:
    /// `load`, `transcribe`, `separate_stems`, `analyze_track`, `cut_range`,
    /// `trim`, `gain`, `normalize`, `time_stretch`, `pitch_shift`,
    /// `align_to_beat`, `add_track`, `remove_track`, `set_track_gain`,
    /// `render_preview`, `render_final`.
    ///
    /// Callers that need a different mix (e.g. tests omitting the
    /// Whisper-dependent `transcribe`) should use [`Self::new`] and
    /// register individually.
    pub fn default_dispatcher() -> Self {
        use crate::tool::{
            AddEffectTool, AddTrackTool, AlignToBeatTool, AnalyzeTrackTool, ApplyDiffTool,
            ApplyRecipeTool, AuditionEffectTool, ChangeSpeedTool, ClickRemovalTool,
            CompareNodesTool, CompressorTool, CopyRegionTool, CreateBusTool, CutRangeTool,
            DeEsserTool, DistortionTool, DuplicateTrackTool, EchoTool, EqTool, ExportLabelsTool,
            ExportMultipleTool, ExportRecipeTool, FadeTool, ForkNodeTool, GainTool,
            GenerateNoiseTool, GenerateToneTool, HighPassFilterTool, ImportLabelsTool,
            InsertSilenceTool, InvertTool, LabelTool, LevelerTool, LimiterTool, LoadTool,
            LowPassFilterTool, MixToNewTrackTool, MonoToStereoTool, MoveClipTool, MuteTrackTool,
            NameNodeTool, NoiseGateTool, NoiseReductionTool, NormalizeLoudnessTool, NormalizeTool,
            NotchFilterTool, PasteRegionTool, PhaserTool, PitchShiftTool, PlotSpectrumTool,
            RemoveClipTool, RemoveEffectTool, RemoveSendTool, RemoveTrackTool, RenameTrackTool,
            RenderFinalTool, RenderPreviewTool, ReorderEffectsTool, RepeatSelectionTool,
            ResampleTrackTool, ReverbTool, ReverseTool, RevertToTool, SeparateStemsTool,
            SetClipEnvelopeTool, SetEffectBypassedTool, SetEffectParamsTool, SetPanTool,
            SetSendTool, SetTrackGainTool, SilenceFinderTool, SilenceRegionTool, SoloTrackTool,
            SplitClipTool, StereoToMonoTool, StereoWidenerTool, StorageReportTool, TimeShiftTool,
            TimeStretchTool, TranscribeTool, TremoloTool, TrimTool, TruncateSilenceTool,
            VocalReductionTool,
        };
        let mut d = Self::new();
        d.register(Box::new(LoadTool));
        d.register(Box::new(TranscribeTool));
        d.register(Box::new(SeparateStemsTool));
        d.register(Box::new(AnalyzeTrackTool));
        d.register(Box::new(CutRangeTool));
        d.register(Box::new(TrimTool));
        d.register(Box::new(GainTool));
        d.register(Box::new(NormalizeTool));
        d.register(Box::new(NormalizeLoudnessTool));
        d.register(Box::new(TimeStretchTool));
        d.register(Box::new(PitchShiftTool));
        d.register(Box::new(AlignToBeatTool));
        d.register(Box::new(AddTrackTool));
        d.register(Box::new(CreateBusTool));
        d.register(Box::new(SetSendTool));
        d.register(Box::new(RemoveSendTool));
        d.register(Box::new(RemoveTrackTool));
        d.register(Box::new(SetTrackGainTool));
        d.register(Box::new(RenderPreviewTool));
        d.register(Box::new(RenderFinalTool));
        // M24: branching DAG ops.
        d.register(Box::new(ForkNodeTool));
        d.register(Box::new(ApplyDiffTool));
        d.register(Box::new(CompareNodesTool));
        d.register(Box::new(RevertToTool));
        d.register(Box::new(NameNodeTool));
        // D1-D3: destructive sample edits.
        d.register(Box::new(EqTool));
        d.register(Box::new(CompressorTool));
        d.register(Box::new(FadeTool));
        d.register(Box::new(ReverseTool));
        d.register(Box::new(ReverbTool));
        d.register(Box::new(InsertSilenceTool));
        d.register(Box::new(ClickRemovalTool));
        d.register(Box::new(EchoTool));
        // D4-D6: copy/paste + labels.
        d.register(Box::new(CopyRegionTool));
        d.register(Box::new(PasteRegionTool));
        d.register(Box::new(LabelTool));
        // D7: spectral noise reduction.
        d.register(Box::new(NoiseReductionTool));
        // D8: noise gate.
        d.register(Box::new(NoiseGateTool));
        // A2: de_esser (sibilance reduction).
        d.register(Box::new(DeEsserTool));
        // D9-D10: leveler and limiter.
        d.register(Box::new(LevelerTool));
        d.register(Box::new(LimiterTool));
        // Task 6: per-clip volume envelope.
        d.register(Box::new(SetClipEnvelopeTool));
        // A1 task 5: change_speed linear resampling.
        d.register(Box::new(ChangeSpeedTool));
        // A1: silence_finder (analysis), silence region, invert, repeat_selection.
        d.register(Box::new(SilenceFinderTool));
        d.register(Box::new(SilenceRegionTool));
        d.register(Box::new(InvertTool));
        d.register(Box::new(RepeatSelectionTool));
        // A1 task 2: metadata mutation.
        d.register(Box::new(SetPanTool));
        d.register(Box::new(RenameTrackTool));
        d.register(Box::new(ResampleTrackTool));
        // A1 task 6: time_shift, duplicate_track.
        d.register(Box::new(TimeShiftTool));
        d.register(Box::new(DuplicateTrackTool));
        // A1 task 7: mute_track, solo_track.
        d.register(Box::new(MuteTrackTool));
        d.register(Box::new(SoloTrackTool));
        // A1 task 8: split_clip.
        d.register(Box::new(SplitClipTool));
        // #103: single-clip placement and deletion.
        d.register(Box::new(MoveClipTool));
        d.register(Box::new(RemoveClipTool));
        // #102: non-destructive per-track effect chains.
        d.register(Box::new(StorageReportTool));
        // #162: the edit chain as a portable file.
        d.register(Box::new(ExportRecipeTool));
        // #166: hear an effect before committing to it.
        d.register(Box::new(AuditionEffectTool));
        d.register(Box::new(ApplyRecipeTool));
        d.register(Box::new(AddEffectTool));
        d.register(Box::new(RemoveEffectTool));
        d.register(Box::new(ReorderEffectsTool));
        d.register(Box::new(SetEffectParamsTool));
        d.register(Box::new(SetEffectBypassedTool));
        // A1 task 9: biquad filters.
        d.register(Box::new(HighPassFilterTool));
        d.register(Box::new(LowPassFilterTool));
        d.register(Box::new(NotchFilterTool));
        // truncate_silence.
        d.register(Box::new(TruncateSilenceTool));
        // Channel conversion: stereo_to_mono, mono_to_stereo.
        d.register(Box::new(MonoToStereoTool));
        d.register(Box::new(StereoToMonoTool));
        // Audio generators: synthesize tones and noise.
        d.register(Box::new(GenerateToneTool));
        d.register(Box::new(GenerateNoiseTool));
        // vocal_reduction: L-R center cancellation for stereo.
        d.register(Box::new(VocalReductionTool));
        // mix_to_new_track: offline-render selected tracks into a new track.
        d.register(Box::new(MixToNewTrackTool));
        // A3 task 2: FFT magnitude spectrum analysis.
        d.register(Box::new(PlotSpectrumTool));
        // A3 task 3: tremolo, phaser, distortion, stereo_widener.
        d.register(Box::new(DistortionTool));
        d.register(Box::new(PhaserTool));
        d.register(Box::new(StereoWidenerTool));
        d.register(Box::new(TremoloTool));
        // A3 task 4: export_labels, import_labels (Audacity format).
        d.register(Box::new(ExportLabelsTool));
        d.register(Box::new(ImportLabelsTool));
        // export_multiple: non-destructive per-track WAV export.
        d.register(Box::new(ExportMultipleTool));
        d
    }

    /// Register a tool. The tool's `input_schema` is extracted and
    /// compiled once here so per-dispatch validation is cheap.
    ///
    /// In debug builds this triggers a `debug_assert!` if a tool with
    /// the same name is already registered; in release builds the
    /// previous entry is silently replaced (last write wins). Either
    /// way, call sites should avoid duplicate registration.
    ///
    /// If the tool's schema is malformed (missing `input_schema`, not
    /// an object, or fails to compile as a JSON Schema), the failure is
    /// stored and surfaced later as
    /// [`DispatchError::MalformedToolSchema`] on `invoke`. We don't
    /// fail registration â€” that would force the API to grow a `Result`
    /// for what is fundamentally a tool-author bug.
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name();
        debug_assert!(
            !self.tools.contains_key(name),
            "tool already registered: {name}",
        );

        let compiled_schema = compile_tool_schema(tool.as_ref());

        self.tools.insert(
            name.to_string(),
            Registered {
                tool,
                compiled_schema,
            },
        );
    }

    /// Look up a tool by name without invoking it.
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|r| r.tool.as_ref())
    }

    /// Remove a previously-registered tool. Returns `true` when a
    /// tool with that name was present.
    pub fn unregister(&mut self, name: &str) -> bool {
        self.tools.remove(name).is_some()
    }

    /// Remove every tool whose name starts with `prefix`. Returns the
    /// number removed. Used by the MCP integration to clear a single
    /// server's remote tools â€” wire names are namespaced
    /// `<server>__<tool>` so the prefix is `"<server>__"`.
    pub fn unregister_prefix(&mut self, prefix: &str) -> usize {
        let names: Vec<String> = self
            .tools
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        let n = names.len();
        for name in names {
            self.tools.remove(&name);
        }
        n
    }

    /// Schemas for every registered tool, shaped for the Anthropic API's
    /// `tools` parameter. Order is unspecified.
    pub fn tool_schemas(&self) -> Value {
        Value::Array(self.tools.values().map(|r| r.tool.schema()).collect())
    }

    /// Names of every registered tool. Order is unspecified.
    pub fn tool_names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    /// Validate `args` against the tool's precompiled `input_schema`
    /// and dispatch.
    ///
    /// Errors:
    /// * [`DispatchError::Unknown`] if `name` is not registered.
    /// * [`DispatchError::MalformedToolSchema`] if the tool's own
    ///   `input_schema` was missing, non-object, or invalid at register
    ///   time. This is a tool-author bug and is distinct from
    ///   `SchemaValidation`, which signals a bad caller payload.
    /// * [`DispatchError::SchemaValidation`] if `args` does not match
    ///   the tool's `input_schema`.
    pub fn invoke(&self, name: &str, args: Value, ctx: &mut ToolContext) -> Result<ToolResult> {
        let entry = self
            .tools
            .get(name)
            .ok_or_else(|| DispatchError::Unknown(name.to_string()))?;

        let compiled = entry.compiled_schema.as_ref().map_err(|reason| {
            DispatchError::MalformedToolSchema {
                tool: name.to_string(),
                reason: reason.clone(),
            }
        })?;

        if let Err(errors) = compiled.validate(&args) {
            let joined = errors
                .map(|e| format!("{} at {}", e, e.instance_path))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(DispatchError::SchemaValidation(joined));
        }

        // Provenance is recorded here rather than in each tool, and that
        // is the whole reason it is feasible: this is the one place every
        // edit passes through, so all 81 tools are covered by one code
        // path and a tool added tomorrow is covered without being
        // touched. Threading it through the tools instead would mean
        // editing sixty-odd files and would drift the first time someone
        // forgot — the failure mode this repo already keeps guard tests
        // for.
        let head_before = ctx.store.head();
        let result = entry.tool.invoke(args.clone(), ctx)?;

        // A moved head means a node was appended, and that node is the
        // one this call produced.
        if let Some(new_head) = ctx.store.head() {
            if Some(new_head) != head_before {
                // A tool that reads outside the session may have closed
                // over what it read and recorded a richer op itself
                // (#163) — `load` pins its source by content hash,
                // `paste_region` names the CAS blob its clipboard went
                // to. Only those few do; everything else is covered by
                // the default below, which is the point of recording
                // here rather than in sixty-odd tools.
                let already_recorded = ctx
                    .store
                    .get(new_head)
                    .map(|n| n.op.is_some())
                    .unwrap_or(false);
                if already_recorded {
                    return Ok(result);
                }

                let op = session::NodeOp {
                    tool: name.to_string(),
                    params: args,
                    engine_version: env!("CARGO_PKG_VERSION").to_string(),
                    reproducible: !READS_OUTSIDE_THE_SESSION.contains(&name),
                    inputs: serde_json::Value::Null,
                };
                // Provenance is metadata about an edit that has already
                // happened and is already durable. Failing the call now
                // would report an error for work that succeeded, so a
                // write failure is logged and swallowed; the node simply
                // reads as "not known to be rebuildable", which is the
                // safe direction.
                if let Err(e) = ctx.store.set_op(new_head, op) {
                    tracing::warn!(tool = name, error = %e, "failed to record node provenance");
                }
            }
        }
        Ok(result)
    }
}

/// Tools whose output depends on something the session does not contain.
///
/// This is the **fallback** classification, applied when the tool did not
/// record an op of its own. Since #163 three of the four close over what
/// they read and record themselves, so the entry here only applies when
/// that recording failed — a clipboard blob that could not be written,
/// say — and the safe reading is the one this list gives.
///
/// * `paste_region` splices `ToolContext::clipboard`. It used to live
///   only in memory, so after a paste that audio existed *only* in the
///   derived file; `copy_region` now persists it as a CAS blob and the
///   paste references it.
/// * `load` reads a file from somewhere else on disk that may have moved
///   or changed since; it now pins the *audio* by content hash, so a
///   move is still recognised and a change is refused by name.
/// * `transcribe` records its model and stays here permanently: model
///   weights are not part of the session and should not be.
/// * `separate_stems` appends no node at all — it writes stems and
///   returns their paths — so it has no op to classify. Listed anyway,
///   because the day it does append one, the safe default is this one.
///
/// This list is an optimisation and a diagnostic, not the safety
/// mechanism. The safety mechanism is that a derived file is named
/// `blake3(its own samples)`, so any rebuild is checked against the name
/// it has to produce — a misclassification here cannot corrupt audio, it
/// can only waste a rebuild attempt. `tool_provenance.rs` pins these
/// names against the registry so a rename cannot quietly drop one.
pub const READS_OUTSIDE_THE_SESSION: &[&str] =
    &["load", "paste_region", "transcribe", "separate_stems"];

/// Extract `input_schema` from `tool.schema()` and compile it. Returns
/// `Err(reason)` for any tool-author bug so the dispatcher can surface
/// it as [`DispatchError::MalformedToolSchema`] on dispatch.
fn compile_tool_schema(tool: &dyn Tool) -> std::result::Result<JSONSchema, String> {
    let schema = tool.schema();
    let input_schema = schema
        .get("input_schema")
        .ok_or_else(|| "missing input_schema".to_string())?;
    if !input_schema.is_object() {
        return Err("input_schema is not a JSON object".to_string());
    }
    JSONSchema::options()
        .with_draft(jsonschema::Draft::Draft7)
        .compile(input_schema)
        .map_err(|e| format!("invalid input_schema: {e}"))
}
