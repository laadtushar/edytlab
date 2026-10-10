//! Dispatcher: trait, registry, and per-call context.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use jsonschema::JSONSchema;
use serde_json::Value;

use crate::{DispatchError, Result, ToolResult};

/// Mutable references handed to tools for the duration of a single
/// invocation. Phase 1 carries the session store and the audio engine;
/// later phases may add caches, logging sinks, etc.
///
/// The lifetime parameter ties the borrows to the caller â€” tools must
/// not stash these references beyond the call.
///
/// What `store` is depends on how the call was dispatched. Under
/// [`crate::Shared::dispatch`] a tool that says [`Tool::runs_off_the_lock`]
/// is given a *staged* handle (`session::Store::stage`) and a fresh
/// engine and clipboard of its own, and no lock is held while it runs;
/// every other tool is given the app's store, engine and clipboard with
/// their locks held for the whole call, as before (#421).
pub struct ToolContext<'a> {
    pub store: &'a mut session::Store,
    pub engine: &'a mut audio_engine::Engine,
    pub user_message: &'a str,
    /// In-memory audio clipboard shared between `copy_region` and
    /// `paste_region`. Held behind a mutable reference so both tools
    /// can read/write without cloning large sample buffers.
    pub clipboard: &'a mut Option<crate::Clipboard>,
    /// Tools this turn is allowed to run, or `None` for unrestricted.
    ///
    /// This lives on the *context* rather than the dispatcher because
    /// the context is what reaches a tool, and meta-tools are where the
    /// restriction used to be lost: `batch_apply`, `apply_recipe` and
    /// `rederive` each build a fresh `default_dispatcher()`, which never
    /// saw the whitelist. The capabilities checkbox was consequently a
    /// suggestion — unticking `render_final` still let the model reach
    /// it by calling the enabled-by-default `batch_apply` (#238).
    ///
    /// It is a required field, deliberately. A future meta-tool that
    /// builds its own sub-context cannot silently forget to carry the
    /// policy forward: leaving it out is a compile error, not a hole.
    pub allowed_tools: Option<&'a std::collections::HashSet<String>>,
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

    /// Whether a call to this tool can change anything the user owns.
    ///
    /// A call "mutates" if it can append a session node, move the head,
    /// label or rename a version, overwrite the clipboard, or write or
    /// delete any file other than the app's own content-addressed,
    /// rebuildable caches (the `derived/track-<hash>.wav` that the app's
    /// `list_tracks` command also writes).
    ///
    /// Plan first relies on this flag (#415): with it on and no plan from
    /// the model, the first step that includes a mutating call is shown
    /// to the user for approval before anything in it runs.
    ///
    /// The default is `true`, deliberately. A new tool, and every tool
    /// an MCP server adds, is held for approval unless someone marks it
    /// read-only on purpose; `tests/read_only_tools.rs` pins that list,
    /// so widening it is a reviewed decision rather than an oversight.
    ///
    /// Per tool, not per call: the one place arguments change the answer
    /// is `apply_recipe`'s dry run, and that is held conservatively.
    fn mutates(&self) -> bool {
        true
    }

    /// Whether this tool may run without the app's store lock held (#421).
    ///
    /// A long tool (a time-stretch of a 32 s stereo track takes ten
    /// seconds in a debug build) used to hold the store, engine and
    /// clipboard locks for all of it, and every UI read waited behind it.
    /// A tool that returns `true` here is run by [`crate::Shared::dispatch`]
    /// against a staged store handle with no lock held. Its appends are
    /// kept in memory and published under the lock only if the session's
    /// head has not moved meanwhile; if it has — the user edited while the
    /// tool ran — the tool is run again on the new head, so the user's
    /// edit is kept and the tool's result lands on top of it.
    ///
    /// A tool may say `true` only if all of these hold:
    ///
    /// * its session access is `head`, `get`, `project_dir` and `append`,
    ///   plus `set_op` on a node it appended. A staged handle refuses
    ///   anything that rewrites history (`set_head`, `set_label`,
    ///   `remove_node`, `detach_parent`, `append_branches`, `fork`);
    /// * it never touches `ctx.clipboard` or `ctx.engine`: in an off-lock
    ///   run they are a local empty clipboard and a fresh engine, not the
    ///   app's;
    /// * it builds no nested dispatcher, and reports no `progress`;
    /// * running it again on a newer head is correct, because it may be.
    ///   It runs on the arguments it was given, so it must read the head
    ///   it is run on rather than remember anything from the first run;
    /// * every file it writes outside the session is named by its own
    ///   content or by a unique temporary name, and is put in place
    ///   whole. A run that loses the race leaves its files behind, unnamed
    ///   by any node, for the sweep to remove.
    ///
    /// The default is `false`, deliberately: a new tool, and every tool an
    /// MCP server adds, runs under the lock as every tool did before.
    /// `tests/off_lock_tools.rs` pins the list, and scans each opted-in
    /// tool's source for what a staged run cannot do, so widening it is a
    /// reviewed decision.
    fn runs_off_the_lock(&self) -> bool {
        false
    }

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
    // Shared so a call can be admitted under the dispatcher's lock and run
    // after it is released ([`ToolDispatcher::prepare`]).
    tool: Arc<dyn Tool>,
    compiled_schema: std::result::Result<JSONSchema, String>,
}

/// A call that passed [`ToolDispatcher::prepare`]: the tool, ready to run
/// without the dispatcher.
///
/// Holds its own reference to the tool, so the dispatcher's lock can be
/// dropped before [`ToolDispatcher::run`] — a tool unregistered meanwhile
/// (an MCP server removed) finishes the call it was admitted for.
pub struct Prepared {
    tool: Arc<dyn Tool>,
    /// The name the caller used. Never `Tool::name()`, which leaks a
    /// `String` per call for an MCP tool.
    name: String,
}

impl Prepared {
    /// See [`Tool::runs_off_the_lock`].
    pub fn runs_off_the_lock(&self) -> bool {
        self.tool.runs_off_the_lock()
    }

    /// The name the call was made under.
    pub fn name(&self) -> &str {
        &self.name
    }
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
            ApplyRecipeTool, AuditionEffectTool, BatchApplyTool, ChangeSpeedTool, ClickRemovalTool,
            CompactSessionTool, CompareNodesTool, CompressorTool, CopyRegionTool, CreateBusTool,
            CutRangeTool, CutWordsTool, DeEsserTool, DistortionTool, DuckUnderSpeechTool,
            DuplicateTrackTool, EchoTool, EqTool, ExportLabelsTool, ExportMultipleTool,
            ExportRecipeTool, FadeTool, ForkNodeTool, GainTool, GenerateNoiseTool,
            GenerateToneTool, HighPassFilterTool, ImportLabelsTool, InsertSilenceTool, InvertTool,
            LabelTool, LevelerTool, LimiterTool, LoadTool, LowPassFilterTool, MixToNewTrackTool,
            MonoToStereoTool, MoveClipTool, MuteTrackTool, NameNodeTool, NoiseGateTool,
            NoiseReductionTool, NormalizeLoudnessTool, NormalizeTool, NotchFilterTool,
            PasteRegionTool, PhaserTool, PitchShiftTool, PlotSpectrumTool, PunchInTool,
            RemoveClipTool, RemoveEffectTool, RemoveFillersTool, RemoveSendTool, RemoveTrackTool,
            RenameTrackTool, RenderFinalTool, RenderPreviewTool, ReorderEffectsTool,
            RepeatSelectionTool, ResampleTrackTool, ReverbTool, ReverseTool, RevertToTool,
            SelectRegionTool, SeparateStemsTool, SetClipEnvelopeTool, SetEffectBypassedTool,
            SetEffectParamsTool, SetPanTool, SetSendTool, SetSyncLockTool, SetTrackGainTool,
            SilenceFinderTool, SilenceRegionTool, SoloTrackTool, SplitBySpeakerTool, SplitClipTool,
            StereoToMonoTool, StereoWidenerTool, StorageReportTool, TimeShiftTool, TimeStretchTool,
            TranscribeTool, TremoloTool, TrimTool, TruncateSilenceTool, VocalReductionTool,
        };
        let mut d = Self::new();
        d.register(Box::new(LoadTool));
        d.register(Box::new(TranscribeTool));
        d.register(Box::new(SeparateStemsTool));
        d.register(Box::new(SplitBySpeakerTool));
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
        d.register(Box::new(SetSyncLockTool));
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
        // #168: make every range-taking tool describable.
        d.register(Box::new(SelectRegionTool));
        // A1 task 5: change_speed linear resampling.
        d.register(Box::new(ChangeSpeedTool));
        // A1: silence_finder (analysis), silence region, invert, repeat_selection.
        d.register(Box::new(SilenceFinderTool));
        d.register(Box::new(SilenceRegionTool));
        d.register(Box::new(InvertTool));
        d.register(Box::new(RepeatSelectionTool));
        // #203: fix a misread line without redoing the take.
        d.register(Box::new(PunchInTool));
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
        // #98: the deliberate half of reclamation.
        d.register(Box::new(CompactSessionTool));
        // #162: the edit chain as a portable file.
        d.register(Box::new(ExportRecipeTool));
        // #166: hear an effect before committing to it.
        d.register(Box::new(AuditionEffectTool));
        d.register(Box::new(ApplyRecipeTool));
        // #169: one chain across a folder.
        d.register(Box::new(BatchApplyTool));
        // #157: editing the audio by editing the transcript.
        d.register(Box::new(CutWordsTool));
        // #165: the most-performed edit in podcast production.
        d.register(Box::new(RemoveFillersTool));
        // #168: music under voice, keyed on words rather than level.
        d.register(Box::new(DuckUnderSpeechTool));
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
                tool: Arc::from(tool),
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

    /// Everything that must hold before a call may run: the tool is
    /// permitted this turn, it exists, its own schema compiled, and `args`
    /// satisfy that schema. Checked in that order, with those errors.
    ///
    /// This is the single pre-dispatch path. [`Self::invoke`] runs it, and
    /// [`Self::would_mutate`] asks it, so the approval gate and the
    /// dispatch cannot disagree about which calls would be refused.
    fn admit(
        &self,
        name: &str,
        args: &Value,
        allowed: Option<&HashSet<String>>,
    ) -> Result<&Registered> {
        // Before the registry lookup, so a refused tool is refused
        // whether or not it exists.
        //
        // The whitelist used to be applied only when trimming the
        // schema list sent to the model, and never here — so a model
        // that named a filtered-out tool anyway got it executed, and any
        // nested dispatcher bypassed the restriction entirely (#238).
        if let Some(allowed) = allowed {
            if !allowed.contains(name) {
                return Err(DispatchError::NotPermitted(name.to_string()));
            }
        }

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

        if let Err(errors) = compiled.validate(args) {
            let joined = errors
                .map(|e| format!("{} at {}", e, e.instance_path))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(DispatchError::SchemaValidation(joined));
        }

        Ok(entry)
    }

    /// Whether dispatching `name` with `args` would change the session
    /// (see [`Tool::mutates`]).
    ///
    /// A call that would be refused before it ran — not permitted, an
    /// unknown tool, arguments that fail the schema — runs nothing, so it
    /// needs no approval and answers `false`. Sharing [`Self::admit`] with
    /// `invoke` is what keeps this from drifting from what dispatch does.
    pub fn would_mutate(
        &self,
        name: &str,
        args: &Value,
        allowed: Option<&HashSet<String>>,
    ) -> bool {
        // `name` here is the model's, never `Tool::name()`: that leaks a
        // `String` per call for an MCP tool.
        self.admit(name, args, allowed)
            .is_ok_and(|entry| entry.tool.mutates())
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
        let prepared = self.prepare(name, &args, ctx.allowed_tools)?;
        Self::run(&prepared, args, ctx)
    }

    /// Admit a call — the checks of [`Self::invoke`], with the same errors —
    /// and return it ready to run without this dispatcher (#421).
    ///
    /// The point of splitting `invoke` in two is the lock: a caller that
    /// shares the dispatcher behind a mutex takes it for this and drops it
    /// before [`Self::run`], so a tool that takes seconds does not hold
    /// up everything else that wants the registry.
    pub fn prepare(
        &self,
        name: &str,
        args: &Value,
        allowed: Option<&HashSet<String>>,
    ) -> Result<Prepared> {
        let entry = self.admit(name, args, allowed)?;
        Ok(Prepared {
            tool: Arc::clone(&entry.tool),
            name: name.to_string(),
        })
    }

    /// Run an admitted call: the tool, then the provenance record for the
    /// node it produced. This is the second half of [`Self::invoke`], with
    /// no registry needed.
    pub fn run(prepared: &Prepared, args: Value, ctx: &mut ToolContext) -> Result<ToolResult> {
        let name = prepared.name.as_str();

        // Provenance is recorded here rather than in each tool, and that
        // is the whole reason it is feasible: this is the one place every
        // edit passes through, so all 81 tools are covered by one code
        // path and a tool added tomorrow is covered without being
        // touched. Threading it through the tools instead would mean
        // editing sixty-odd files and would drift the first time someone
        // forgot — the failure mode this repo already keeps guard tests
        // for.
        let head_before = ctx.store.head();
        let result = prepared.tool.invoke(args.clone(), ctx)?;

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

                // A range the call took from the chat message is part of
                // what it did: a replay has no message, so record it as
                // the range the call used (#377). Typed ranges win in
                // `range_resolver::resolve`, so the replay reads exactly
                // these seconds back.
                let mut params = args;
                if READS_RANGE_FROM_MESSAGE.contains(&name)
                    && params.get("range").is_none_or(Value::is_null)
                {
                    if let Some(range) = crate::util::range_resolver::from_message(ctx.user_message)
                    {
                        params["range"] = serde_json::json!(range);
                    }
                }
                let op = session::NodeOp {
                    tool: name.to_string(),
                    params,
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
/// Tools that, when a call names no `range`, take it from the
/// `[apply to …]` prefix the app puts on a chat message for a selection.
///
/// The dispatcher records that range in the op, so the call replays.
/// Only these: a tool that ignores the message would replay a range it
/// never used. `tool_provenance.rs` checks that every tool reading the
/// message is listed.
pub const READS_RANGE_FROM_MESSAGE: &[&str] = &["copy_region", "fade", "reverse"];

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
