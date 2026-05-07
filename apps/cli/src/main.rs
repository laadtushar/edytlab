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

    /// Anthropic API key. Required; we never read the OS keychain from
    /// here so test runs don't accidentally pick up the developer's
    /// daily-driver key.
    #[arg(long, env = "ANTHROPIC_API_KEY")]
    api_key: String,

    /// Project directory hosting the `.audiograph/` session store.
    /// Defaults to a fresh temp dir that lives for the process.
    #[arg(long)]
    project_dir: Option<PathBuf>,

    /// Optional Anthropic base URL override (for `wiremock`-style
    /// fixtures or reverse-proxy debugging). Defaults to the public API.
    #[arg(long)]
    base_url: Option<String>,

    /// Optional Anthropic model override; defaults to
    /// [`ai::DEFAULT_MODEL`].
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

    let mut cfg = ai::AnthropicConfig::new(args.api_key);
    if let Some(model) = args.model {
        cfg = cfg.with_model(model);
    }
    if let Some(base_url) = args.base_url {
        cfg = cfg.with_base_url(base_url);
    }

    let mut agent = ai::Agent::new(cfg, dispatcher, store, engine);

    let result = agent
        .turn(args.message, |ev| {
            let mapped = match ev {
                ai::AgentEvent::TextDelta(t) => CliEvent::Text { delta: t },
                ai::AgentEvent::ToolCallStart { name, id } => CliEvent::ToolCallStart { id, name },
                ai::AgentEvent::ToolCallEnd { id, ok } => CliEvent::ToolCallEnd { id, ok },
                ai::AgentEvent::NodeCreated(id) => CliEvent::NodeCreated {
                    node_id: id.to_hex(),
                },
                ai::AgentEvent::Done => CliEvent::Done,
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
