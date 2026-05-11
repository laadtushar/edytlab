//! Dispatcher: trait, registry, and per-call context.

use std::collections::HashMap;

use jsonschema::JSONSchema;
use serde_json::Value;

use crate::{DispatchError, Result, ToolResult};

/// Mutable references handed to tools for the duration of a single
/// invocation. Phase 1 carries the session store and the audio engine;
/// later phases may add caches, logging sinks, etc.
///
/// The lifetime parameter ties the borrows to the caller — tools must
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
/// and reuse it on every dispatch — schema compilation is comparatively
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
            AddTrackTool, AlignToBeatTool, AnalyzeTrackTool, ApplyDiffTool, CompareNodesTool,
            CopyRegionTool, CutRangeTool, FadeTool, ForkNodeTool, GainTool, InsertSilenceTool,
            LabelTool, LoadTool, NameNodeTool, NormalizeTool, PasteRegionTool, PitchShiftTool,
            RemoveTrackTool, RenderFinalTool, RenderPreviewTool, ReverseTool, RevertToTool,
            SeparateStemsTool, SetTrackGainTool, TimeStretchTool, TranscribeTool, TrimTool,
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
        d.register(Box::new(TimeStretchTool));
        d.register(Box::new(PitchShiftTool));
        d.register(Box::new(AlignToBeatTool));
        d.register(Box::new(AddTrackTool));
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
        d.register(Box::new(FadeTool));
        d.register(Box::new(ReverseTool));
        d.register(Box::new(InsertSilenceTool));
        // D4-D6: copy/paste + labels.
        d.register(Box::new(CopyRegionTool));
        d.register(Box::new(PasteRegionTool));
        d.register(Box::new(LabelTool));
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
    /// fail registration — that would force the API to grow a `Result`
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

    /// Schemas for every registered tool, shaped for the Anthropic API's
    /// `tools` parameter. Order is unspecified.
    pub fn tool_schemas(&self) -> Value {
        Value::Array(self.tools.values().map(|r| r.tool.schema()).collect())
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

        entry.tool.invoke(args, ctx)
    }
}

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
