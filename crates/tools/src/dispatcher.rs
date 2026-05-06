//! Dispatcher: trait, registry, and per-call context.

use std::collections::HashMap;

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

/// Registry of tools keyed by canonical name.
///
/// Use [`register`](ToolDispatcher::register) to add tools at startup,
/// then [`invoke`](ToolDispatcher::invoke) per model tool call. Use
/// [`tool_schemas`](ToolDispatcher::tool_schemas) when constructing the
/// `tools` parameter for the Anthropic API.
#[derive(Default)]
pub struct ToolDispatcher {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolDispatcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tool. If a tool with the same name is already
    /// registered it is replaced (last write wins); call sites should
    /// avoid this in practice.
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// Look up a tool by name without invoking it.
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    /// Schemas for every registered tool, shaped for the Anthropic API's
    /// `tools` parameter. Order is unspecified.
    pub fn tool_schemas(&self) -> Value {
        Value::Array(self.tools.values().map(|t| t.schema()).collect())
    }

    /// Validate `args` against the tool's `input_schema` and dispatch.
    ///
    /// Errors:
    /// * [`DispatchError::Unknown`] if `name` is not registered.
    /// * [`DispatchError::SchemaValidation`] if `args` does not match.
    /// * [`DispatchError::Tool`] if the tool itself fails (propagated).
    pub fn invoke(&self, name: &str, args: Value, ctx: &mut ToolContext) -> Result<ToolResult> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| DispatchError::Unknown(name.to_string()))?;

        let schema = tool.schema();
        let input_schema = schema.get("input_schema").ok_or_else(|| {
            DispatchError::SchemaValidation(format!("tool {name} did not expose an input_schema",))
        })?;

        crate::schema::validate(input_schema, &args).map_err(DispatchError::SchemaValidation)?;

        tool.invoke(args, ctx)
    }
}
