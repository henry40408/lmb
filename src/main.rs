use std::{io::Read, path::PathBuf, process::ExitCode, time::Duration};

use anyhow::bail;
use clap::{Parser, Subcommand};
use clio::Input;
use lmb::{
    Runner,
    error::{ErrorReport, build_report, render_report},
};
use no_color::is_no_color;
use rusqlite::Connection;
use serde_json::Value;
use tokio::io::{self, AsyncWriteExt as _};
use tracing::{Instrument, debug_span, info};
use tracing_subscriber::fmt::format::FmtSpan;

const VERSION: &str = env!("APP_VERSION");

#[derive(Debug, Parser)]
#[clap(author, version=VERSION, about, long_about = None)]
struct Opts {
    /// Allowed environment variables
    #[clap(long, value_delimiter = ',')]
    allow_env: Vec<String>,
    /// Optional HTTP timeout in seconds
    #[clap(long)]
    http_timeout: Option<jiff::Span>,
    /// Optional path to the store file
    #[clap(long, value_parser, group = "store")]
    store_path: Option<PathBuf>,
    /// Disable store usage
    #[clap(long, action, group = "store")]
    no_store: bool,
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Evaluate a file
    Eval {
        /// Path to the file to evaluate
        #[clap(long, value_parser)]
        file: Input,
        /// Optional state to pass to the Lua script
        #[clap(long, value_parser)]
        state: Option<Value>,
        /// Timeout. Default is 30 seconds, set to 0 for no timeout
        #[clap(long)]
        timeout: Option<jiff::Span>,
    },
}

async fn try_main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_ansi(!is_no_color())
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_span_events(FmtSpan::CLOSE)
        .compact()
        .init();

    let opts = Opts::parse();
    info!("Parsed options: {opts:?}");

    match opts.command {
        Command::Eval {
            mut file,
            state,
            timeout,
        } => {
            let span = debug_span!("eval").entered();
            info!("Evaluate Lua script: {file:?}");
            info!("State: {state:?}");

            let reader = io::stdin();
            let source = if file.is_local() {
                None
            } else if file.is_std() {
                let mut buf = String::new();
                file.read_to_string(&mut buf)?;
                Some(buf)
            } else {
                bail!("Expected a local file or a stdin input, but got: {file}");
            };

            let http_timeout = match opts.http_timeout {
                None => Some(Duration::from_secs(30)),
                Some(t) if t.is_zero() => None,
                Some(t) => Some(Duration::try_from(t)?),
            };
            info!("Using HTTP timeout: {:?}", http_timeout);

            let timeout = match timeout {
                None => Some(Duration::from_secs(30)),
                Some(t) if t.is_zero() => None,
                Some(t) => Some(Duration::try_from(t)?),
            };
            info!("Using timeout: {:?}", timeout);

            let conn = match (opts.store_path, opts.no_store) {
                (None, false) => Some(Connection::open_in_memory()?),
                (Some(path), false) => Some(Connection::open(path)?),
                _ => None,
            };

            let runner = if let Some(source) = &source {
                info!("Evaluating Lua code from stdin or a string input");
                Runner::builder(source, reader)
                    .allow_env(opts.allow_env)
                    .default_name("(stdin)")
                    .maybe_http_timeout(http_timeout)
                    .maybe_store(conn)
                    .maybe_timeout(timeout)
                    .build()?
            } else {
                info!("Evaluating Lua code from file: {:?}", file.path().path());
                Runner::builder(file.path().path(), reader)
                    .allow_env(opts.allow_env)
                    .maybe_http_timeout(http_timeout)
                    .maybe_store(conn)
                    .maybe_timeout(timeout)
                    .build()?
            };

            let result = {
                let span2 = debug_span!(parent: &span, "invoke");
                runner
                    .invoke()
                    .maybe_state(state)
                    .call()
                    .instrument(span2)
                    .await?
            };
            info!("Lua evaluated");

            match result.result {
                Ok(value) => {
                    info!("Lua evaluation result: {value}");
                    if let Value::String(s) = &value {
                        println!("{s}");
                    } else {
                        println!("{value}");
                    }
                }
                Err(e) => {
                    let report = if let Some(source) = &source {
                        build_report(source, &e).default_name("(stdin)").call()?
                    } else {
                        build_report(file.path().path(), &e).call()?
                    };
                    match report {
                        ErrorReport::Report(report) => {
                            let mut s = String::new();
                            render_report(&mut s, &report);
                            io::stderr().write_all(s.as_bytes()).await?;
                        }
                        ErrorReport::String(msg) => eprintln!("{msg}"),
                    }
                    return Err(e.into());
                }
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    if let Err(e) = try_main().await {
        eprintln!("Error: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
