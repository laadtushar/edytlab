//! `edytlab-cli` — headless one-shot driver for the agent loop.
//!
//! Pure test/infrastructure entry point. Wires the same core stack as
//! the Tauri shell (`ai::Agent` + `tools::ToolDispatcher` +
//! `session::Store` + `audio_engine::Engine`) and runs ONE
//! [`Agent::turn`](ai::Agent::turn) against a user-supplied message,
//! streaming events as JSON-lines on stdout. Used by the M16 E2E
//! podcast-cleanup smoke test and for ad-hoc headless reproductions on
//! CI; it is not packaged in user-facing builds.

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use clap::Parser;
use serde::Serialize;

#[derive(Parser, Debug)]
#[command(about = "edytlab headless agent driver (E2E test infrastructure)")]
struct Args {
    /// Natural-language message to send as a single agent turn.
    #[arg(long)]
    message: String,

    /// LLM API key. Required; we never read the OS keychain from here
    /// so test runs don't accidentally pick up the developer's
    /// daily-driver key. Falls back to `ANTHROPIC_API_KEY` if
    /// `--api-key` is omitted.
    #[arg(long, env = "ANTHROPIC_API_KEY")]
    api_key: String,

    /// LLM provider id. `anthropic` (default) hits Anthropic's API
    /// directly; `openrouter` routes through OpenRouter's
    /// Anthropic-compatible Messages endpoint with `Authorization:
    /// Bearer <key>` auth and the `anthropic/...` model namespace.
    #[arg(long, default_value = ai::ANTHROPIC_ID)]
    provider: String,

    /// Project directory hosting the `.audiograph/` session store.
    /// Defaults to a fresh temp dir that lives for the process.
    #[arg(long)]
    project_dir: Option<PathBuf>,

    /// Optional base URL override (for `wiremock`-style fixtures or
    /// reverse-proxy debugging). Defaults to the provider's public API.
    #[arg(long)]
    base_url: Option<String>,

    /// Optional model id override; defaults to the provider's
    /// `default_model`. Use the canonical Anthropic id (e.g.
    /// `claude-sonnet-4-6`); the OpenRouter provider prepends
    /// `anthropic/` automatically.
    #[arg(long)]
    model: Option<String>,
}

/// JSON-lines event shape. Each [`ai::AgentEvent`] is mapped to a small
/// stable object so test consumers can match on `kind` without parsing
/// Rust `Debug` output.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CliEvent {
    Text { delta: String },
    ToolCallStart { id: String, name: String },
    ToolCallEnd { id: String, ok: bool },
    NodeCreated { node_id: String },
    Done,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Hold the TempDir for the process lifetime so the store directory
    // doesn't get nuked mid-turn.
    let (project_dir, _tmp_guard) = match args.project_dir {
        Some(p) => (p, None),
        None => {
            let tmp = tempfile::TempDir::new()?;
            (tmp.path().to_path_buf(), Some(tmp))
        }
    };

    let store = Arc::new(Mutex::new(session::Store::open(&project_dir)?));
    let dispatcher = Arc::new(Mutex::new(tools::ToolDispatcher::default_dispatcher()));
    let engine = Arc::new(Mutex::new(audio_engine::Engine::new()));

    let provider = ai::validate::provider_for(&args.provider);
    let mut cfg = ai::LlmConfig::new(provider, args.api_key);
    if let Some(model) = args.model {
        cfg = cfg.with_model(model);
    }
    if let Some(base_url) = args.base_url {
        cfg = cfg.with_base_url(base_url);
    }

    let clipboard = Arc::new(Mutex::new(None::<tools::Clipboard>));
    let plan_notify = std::sync::Arc::new(tokio::sync::Notify::new());
    // The CLI is non-interactive: there is no UI to edit a proposed plan
    // before approval, so the override slot stays empty for its lifetime.
    let plan_steps_override = Arc::new(Mutex::new(None::<String>));
    let mut agent = ai::Agent::new(
        cfg,
        dispatcher,
        store,
        engine,
        plan_notify,
        plan_steps_override,
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
        clipboard,
    );

    let result = agent
        .turn(args.message, |ev| {
            let mapped = match ev {
                ai::AgentEvent::TextDelta(t) => CliEvent::Text { delta: t },
                ai::AgentEvent::ToolCallStart { name, id } => CliEvent::ToolCallStart { id, name },
                // `view` is a chart for a GUI to draw; a line-oriented
                // CLI has nowhere to put it, and the tool's own JSON
                // result is already on the model's side of the wire.
                ai::AgentEvent::ToolCallEnd { id, ok, view: _ } => CliEvent::ToolCallEnd { id, ok },
                ai::AgentEvent::NodeCreated(id) => CliEvent::NodeCreated {
                    node_id: id.to_hex(),
                },
                ai::AgentEvent::Done => CliEvent::Done,
                // Plan events are frontend-only (approval card); CLI just
                // prints the steps as a text delta so the user can see them.
                ai::AgentEvent::Plan { steps } => CliEvent::Text {
                    delta: format!("[plan: {} steps]", steps.len()),
                },
                // The CLI has no approval UI and never sets plan_first,
                // so this is unreachable there today — but printing it
                // beats a silent turn if that ever changes.
                ai::AgentEvent::PlanRejected => CliEvent::Text {
                    delta: "[plan rejected]".to_string(),
                },
                // Reachable wherever a plan was asked for: the turn goes
                // ahead without the gate, and saying so beats letting it
                // look like the model chose not to plan (#267).
                ai::AgentEvent::PlanUnavailable { reason } => CliEvent::Text {
                    delta: format!("[plan unavailable: {reason}]"),
                },
            };
            // Best-effort: a stdout broken-pipe (consumer process exited)
            // shouldn't panic the agent loop. We swallow write errors and
            // let the turn finish; the final summary line below propagates
            // the error if stdout is genuinely gone.
            if let Ok(line) = serde_json::to_string(&mapped) {
                let _ = writeln!(io::stdout(), "{line}");
            }
        })
        .await?;

    // Final summary line for non-streaming consumers.
    let summary = serde_json::json!({
        "kind": "summary",
        "text": result.text,
        "stop_reason": result.stop_reason,
        "node_ids": result.node_ids.iter().map(|i| i.to_hex()).collect::<Vec<_>>(),
    });
    // Propagate any write error here so callers piping to a closed
    // consumer get a non-zero exit instead of a panic from `println!`.
    writeln!(io::stdout(), "{summary}")?;

    Ok(())
}
