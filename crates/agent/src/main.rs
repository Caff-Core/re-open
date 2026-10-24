//! `reopen-agent`: the Re-Open background agent and its command-line client.

use std::io::Write as _;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use reopen_agent::ipc::client::{self, ClientError};
use reopen_agent::paths::AgentPaths;
use reopen_agent::render;
use reopen_agent::run::{self, FakeScenario, ProviderChoice, RunOptions};
use serde_json::json;
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(name = "reopen-agent", version, about = "Re-Open background agent")]
struct Cli {
    /// Use the isolated development directories (`reopen-dev`).
    #[arg(long, global = true)]
    dev: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the agent in the foreground until interrupted.
    Run {
        /// Where application capture comes from.
        #[arg(long, value_enum, default_value_t = Provider::Linux)]
        provider: Provider,
        /// What the fake provider reports (with `--provider fake`).
        #[arg(long, value_enum, default_value_t = Scenario::Complete)]
        fake_scenario: Scenario,
        /// Seconds between scheduled captures.
        #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..=3600))]
        interval_secs: u64,
    },
    /// Show agent status and the last checkpoint.
    Status,
    /// Request an immediate capture ("Save now").
    Capture,
    /// List recent checkpoints, newest first.
    History {
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u64).range(1..=200))]
        limit: u64,
    },
    /// Inspect checkpoints.
    Checkpoint {
        #[command(subcommand)]
        command: CheckpointCommand,
    },
}

#[derive(Debug, Subcommand)]
enum CheckpointCommand {
    /// Show one checkpoint's sources and resources.
    Show { id: Uuid },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Provider {
    /// GNOME window list, falling back to running apps (partial).
    Linux,
    /// Scripted fake data.
    Fake,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Scenario {
    Complete,
    Partial,
    Unavailable,
    Empty,
}

impl From<Scenario> for FakeScenario {
    fn from(scenario: Scenario) -> Self {
        match scenario {
            Scenario::Complete => Self::Complete,
            Scenario::Partial => Self::Partial,
            Scenario::Unavailable => Self::Unavailable,
            Scenario::Empty => Self::Empty,
        }
    }
}

/// Exit code when no agent is running for a client command.
const EXIT_NOT_RUNNING: u8 = 5;

fn report(message: &str) {
    let _ = writeln!(std::io::stderr(), "reopen-agent: {message}");
}

async fn shutdown_signal() {
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        () = terminate => {}
    }
}

async fn client_command(paths: &AgentPaths, command: Command) -> Result<String, ClientError> {
    let socket = paths.desktop_socket();
    let (kind, payload) = match &command {
        Command::Status => ("status.get", json!({})),
        Command::Capture => ("capture.request", json!({})),
        Command::History { limit } => ("checkpoints.list", json!({ "limit": limit })),
        Command::Checkpoint {
            command: CheckpointCommand::Show { id },
        } => ("checkpoints.get", json!({ "id": id })),
        Command::Run { .. } => return Err(ClientError::Protocol),
    };
    let result = client::request(&socket, kind, payload).await?;
    let rendered = match command {
        Command::Status => render::status(result),
        Command::Capture => render::capture(result),
        Command::History { .. } => render::history(result),
        Command::Checkpoint { .. } => render::detail(result),
        Command::Run { .. } => return Err(ClientError::Protocol),
    };
    rendered.map_err(|_| ClientError::Protocol)
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let paths = match AgentPaths::resolve(cli.dev, |key| std::env::var_os(key)) {
        Ok(paths) => paths,
        Err(error) => {
            report(&error.to_string());
            return ExitCode::FAILURE;
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            report(&format!("cannot start the async runtime: {}", error.kind()));
            return ExitCode::FAILURE;
        }
    };

    match cli.command {
        Command::Run {
            provider,
            fake_scenario,
            interval_secs,
        } => {
            let provider = match provider {
                Provider::Linux => ProviderChoice::Linux,
                Provider::Fake => ProviderChoice::Fake(fake_scenario.into()),
            };
            let options = RunOptions {
                provider,
                capture_interval: Duration::from_secs(interval_secs),
            };
            match runtime.block_on(run::run(&paths, options, shutdown_signal())) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    report(&error.to_string());
                    ExitCode::from(error.exit_code())
                }
            }
        }
        command => match runtime.block_on(client_command(&paths, command)) {
            Ok(text) => {
                let _ = std::io::stdout().write_all(text.as_bytes());
                ExitCode::SUCCESS
            }
            Err(error) => {
                report(&error.to_string());
                if error == ClientError::AgentNotRunning {
                    ExitCode::from(EXIT_NOT_RUNNING)
                } else {
                    ExitCode::FAILURE
                }
            }
        },
    }
}
