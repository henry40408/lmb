use std::{
    io::{self, Read, Write},
    path::PathBuf,
    process::ExitCode,
    sync::Arc,
    time::Duration,
};

use anyhow::bail;
use axum::{Router, routing::any};
use bon::Builder;
use byte_unit::Byte;
use clap::{Parser, Subcommand};
use clio::Input;
use lmb::{
    LmbError, Runner,
    error::{ErrorReport, build_report, render_report},
    permission::{EnvPermissions, NetPermissions, Permissions},
};
use no_color::is_no_color;
use rusqlite::Connection;
use serde_json::{Value, json};
use tracing::{Instrument, Level, debug, debug_span, info, warn};
use tracing_subscriber::{EnvFilter, fmt::format::FmtSpan};

mod serve;

const VERSION: &str = env!("APP_VERSION");

const LONG_ABOUT: &str = "\
A standalone Luau runtime for running Lua scripts from the command line.

lmb provides a sandboxed environment with built-in modules for cryptography,
HTTP requests, JSON/YAML/TOML parsing, and coroutine utilities. It is optimized
for quick execution and suitable for low-end hardware like Raspberry Pi.";

const AFTER_HELP: &str = "\
EXAMPLES:
    Evaluate a Lua script:
        lmb eval --file script.lua

    Evaluate with input from stdin:
        echo 'Hello' | lmb eval --file script.lua

    Start an HTTP server:
        lmb serve --file handler.lua --bind 0.0.0.0:3000

    Allow environment variable access:
        lmb --allow-env API_KEY eval --file script.lua

    Allow network access to specific hosts:
        lmb --allow-net api.example.com eval --file script.lua";

#[derive(Debug, Parser)]
#[clap(author, version=VERSION, about, long_about = LONG_ABOUT, after_help = AFTER_HELP)]
struct Opts {
    /// Allow all resources
    #[clap(long, env = "ALLOW_ALL")]
    allow_all: bool,
    /// Allow all environment variables
    #[clap(long, env = "ALLOW_ALL_ENVS")]
    allow_all_envs: bool,
    /// Allowed environment variables
    #[clap(long, value_delimiter = ',', env = "ALLOW_ENV")]
    allow_env: Vec<String>,
    /// Denied environment variables. These take precedence over allowed variables
    #[clap(long, value_delimiter = ',', env = "DENY_ENV")]
    deny_env: Vec<String>,
    /// Allow all network addresses
    #[clap(long, env = "ALLOW_ALL_NET")]
    allow_all_net: bool,
    /// Allowed network addresses
    #[clap(long, value_delimiter = ',', env = "ALLOW_NET")]
    allow_net: Vec<String>,
    /// Denied network addresses. These take precedence over allowed addresses
    #[clap(long, value_delimiter = ',', env = "DENY_NET")]
    deny_net: Vec<String>,
    /// Enable debug mode
    #[clap(long, short = 'd', env = "DEBUG")]
    debug: bool,
    /// Optional HTTP timeout in seconds
    #[clap(long, env = "HTTP_TIMEOUT")]
    http_timeout: Option<jiff::Span>,
    /// Disable colored output
    #[clap(long, env = "NO_COLOR")]
    no_color: bool,
    /// Disable store usage
    #[clap(long, action, group = "store_group", env = "NO_STORE")]
    no_store: bool,
    /// Path to SQLite file for persistent key-value storage
    #[clap(long, value_parser, group = "store_group", env = "STORE_PATH")]
    store_path: Option<PathBuf>,
    /// Script execution timeout (e.g., 30s, 1m). Default: 30s, use 0 for unlimited
    #[clap(long, env = "TIMEOUT")]
    timeout: Option<jiff::Span>,
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Evaluate a Lua script file and execute its returned function
    #[clap(after_help = "\
EXAMPLES:
    lmb eval --file hello.lua
    lmb eval --file script.lua --state '{\"key\": \"value\"}'
    echo 'input' | lmb eval --file processor.lua")]
    Eval {
        /// Path to the Lua script file, use '-' for stdin
        #[clap(long, value_parser, env = "FILE_PATH")]
        file: Input,
        /// JSON state passed to the Lua script as ctx.state
        #[clap(long, env = "STATE")]
        state: Option<String>,
    },
    /// Start an HTTP server that handles requests with a Lua script
    #[clap(after_help = "\
EXAMPLES:
    lmb serve --file api.lua
    lmb serve --file api.lua --bind 0.0.0.0:8080
    lmb serve --file api.lua --pool-size 4 --max-body-size 10mb")]
    Serve {
        /// Address and port to listen on
        #[clap(long, default_value = "127.0.0.1:3000", env = "BIND")]
        bind: String,
        /// Path to the Lua script file, use '-' for stdin
        #[clap(long, value_parser, env = "FILE_PATH")]
        file: Input,
        /// Maximum request body size (e.g., 10mb, 1gb)
        #[clap(long, default_value = "100M", env = "MAX_BODY_SIZE")]
        max_body_size: String,
        /// Number of Lua VM instances to pool for concurrent requests.
        /// WARNING: Requires proper state isolation in Lua scripts.
        #[clap(long, env = "POOL_SIZE")]
        pool_size: Option<usize>,
        /// JSON state passed to the Lua script as ctx.state
        #[clap(long, env = "STATE")]
        state: Option<String>,
    },
}

fn parse_timeout(span: Option<jiff::Span>) -> anyhow::Result<Option<Duration>> {
    match span {
        None => Ok(Some(Duration::from_secs(30))),
        Some(t) if t.is_zero() => Ok(None),
        Some(t) => Ok(Some(Duration::try_from(t)?)),
    }
}

pub(crate) fn open_store_connection(
    store_path: Option<PathBuf>,
    no_store: bool,
) -> anyhow::Result<Option<Connection>> {
    match (store_path, no_store) {
        (None, false) => Ok(Some(Connection::open_in_memory()?)),
        (Some(path), false) => Ok(Some(Connection::open(path)?)),
        _ => Ok(None),
    }
}

fn permissions_from_opts(opts: &Opts) -> Permissions {
    if opts.allow_all {
        Permissions::All {
            denied_env: opts.deny_env.iter().cloned().collect(),
            denied_net: opts.deny_net.iter().cloned().collect(),
        }
    } else {
        Permissions::Some {
            env: if opts.allow_all_envs {
                EnvPermissions::All {
                    denied: opts.deny_env.iter().cloned().collect(),
                }
            } else {
                EnvPermissions::Some {
                    allowed: opts.allow_env.iter().cloned().collect(),
                    denied: opts.deny_env.iter().cloned().collect(),
                }
            },
            net: if opts.allow_all_net {
                NetPermissions::All {
                    denied: opts.deny_net.iter().cloned().collect(),
                }
            } else {
                NetPermissions::Some {
                    allowed: opts.allow_net.iter().cloned().collect(),
                    denied: opts.deny_net.iter().cloned().collect(),
                }
            },
        }
    }
}

async fn report_error(file: &Input, source: &Option<String>, e: &LmbError) -> anyhow::Result<()> {
    let report = if let Some(source) = &source {
        build_report(source, e).default_name("(stdin)").call()?
    } else {
        build_report(file.path().path(), e).call()?
    };
    match report {
        ErrorReport::Report(report) => {
            let mut s = String::new();
            render_report(&mut s, &report);
            io::stderr().write_all(s.as_bytes())?;
            io::stderr().flush()?;
        }
        ErrorReport::String(msg) => eprintln!("{msg}"),
    }
    Ok(())
}

async fn try_main() -> anyhow::Result<()> {
    let opts = Opts::parse();
    debug!("Parsed options: {opts:?}");

    let default_directive = if opts.debug {
        Level::DEBUG
    } else {
        Level::WARN
    };
    let env_filter = EnvFilter::builder()
        .with_default_directive(default_directive.into())
        .from_env_lossy();
    let no_color = opts.no_color || is_no_color();
    tracing_subscriber::fmt()
        .with_ansi(!no_color)
        .with_env_filter(env_filter)
        .with_span_events(FmtSpan::CLOSE)
        .compact()
        .init();

    let permissions = permissions_from_opts(&opts);
    debug!("Permissions: {permissions:?}");

    match opts.command {
        Command::Eval { mut file, state } => {
            let span = debug_span!("eval").entered();
            debug!("Evaluate Lua script: {file:?}");

            let state = state.as_ref().map(|s| match serde_json::from_str(s) {
                Ok(value) => value,
                Err(_) => json!(s.clone()), // treat invalid value as string
            });
            debug!("State: {state:?}");

            let reader = tokio::io::stdin();
            let source = if file.is_local() {
                None
            } else if file.is_std() {
                let mut buf = String::new();
                file.read_to_string(&mut buf)?;
                Some(buf)
            } else {
                bail!("Expected a local file or a stdin input, but got: {file}");
            };

            let http_timeout = parse_timeout(opts.http_timeout)?;
            debug!("Using HTTP timeout: {http_timeout:?}");

            let timeout = parse_timeout(opts.timeout)?;
            debug!("Using timeout: {timeout:?}");

            if opts.store_path.is_none() && !opts.no_store {
                warn!("No store path specified, using in-memory store");
            }
            let conn = open_store_connection(opts.store_path, opts.no_store)?;

            let runner = if let Some(source) = &source {
                debug!("Evaluating Lua code from stdin or a string input");
                Runner::builder(source, reader)
                    .default_name("(stdin)")
                    .maybe_http_timeout(http_timeout)
                    .permissions(permissions)
                    .maybe_store(conn)
                    .maybe_timeout(timeout)
                    .build()
            } else {
                debug!("Evaluating Lua code from file: {:?}", file.path().path());
                Runner::builder(file.path().path(), reader)
                    .maybe_http_timeout(http_timeout)
                    .permissions(permissions)
                    .maybe_store(conn)
                    .maybe_timeout(timeout)
                    .build()
            };

            let runner = match runner {
                Ok(runner) => runner,
                Err(e) => {
                    report_error(&file, &source, &e).await?;
                    return Err(e.into());
                }
            };

            let result = {
                let span2 = debug_span!(parent: &span, "invoke");
                let state = lmb::State::builder().maybe_state(state).build();
                runner
                    .invoke()
                    .state(state)
                    .call()
                    .instrument(span2)
                    .await?
            };
            debug!("Lua evaluated");

            match result.result {
                Ok(value) => {
                    debug!("Lua evaluation result: {value}");
                    if let Value::String(s) = &value {
                        println!("{s}");
                    } else {
                        println!("{value}");
                    }
                }
                Err(e) => {
                    report_error(&file, &source, &e).await?;
                    return Err(e.into());
                }
            }
        }
        Command::Serve {
            bind,
            mut file,
            max_body_size,
            pool_size,
            state,
        } => {
            let state = state
                .as_ref()
                .map(|s| match serde_json::from_str::<Value>(s) {
                    Ok(value) => value,
                    Err(_) => json!(s.clone()), // treat invalid value as string
                });
            debug!("State: {state:?}");

            let max_body_size = Byte::parse_str(max_body_size, true)?;
            let max_body_size = usize::try_from(max_body_size.as_u64())?;

            let http_timeout = parse_timeout(opts.http_timeout)?;
            debug!("Using HTTP timeout: {http_timeout:?}");

            let timeout = parse_timeout(opts.timeout)?;
            debug!("Using timeout: {timeout:?}");

            let mut source = String::new();
            file.read_to_string(&mut source)?;

            let name = if file.is_local() {
                file.path().to_string_lossy().to_string()
            } else if file.is_std() {
                "(stdin)".to_string()
            } else {
                bail!("Expected a local file or a stdin input, but got: {file}");
            };

            if let Some(size) = pool_size {
                warn!(
                    "Runner pool enabled (size: {size}). \
                    Lua scripts MUST handle state isolation properly. \
                    Global variables and module-level state will be shared across requests."
                );
            }

            let app_state = Arc::new(
                AppState::builder()
                    .source(source)
                    .maybe_http_timeout(http_timeout)
                    .max_body_size(max_body_size)
                    .name(name)
                    .no_store(opts.no_store)
                    .permissions(permissions)
                    .maybe_pool_size(pool_size)
                    .maybe_state(state)
                    .maybe_store_path(opts.store_path)
                    .maybe_timeout(timeout)
                    .build(),
            );

            let pool = if pool_size.is_some() {
                Some(Arc::new(serve::create_pool(&app_state)?))
            } else {
                None
            };

            let app = build_router(app_state, pool);
            let listener = tokio::net::TcpListener::bind(&bind).await?;
            info!("Listening on {}", listener.local_addr()?);
            axum::serve(listener, app).await?;
        }
    }

    Ok(())
}

fn build_router(app_state: Arc<AppState>, pool: Option<Arc<serve::RunnerPool>>) -> Router {
    Router::new()
        .route("/{*wildcard}", any(serve::request_handler))
        .route("/", any(serve::request_handler))
        .with_state((app_state, pool))
}

#[derive(Builder, Clone)]
struct AppState {
    #[builder(into)]
    source: String,
    http_timeout: Option<Duration>,
    max_body_size: Option<usize>,
    name: Option<String>,
    no_store: Option<bool>,
    permissions: Option<Permissions>,
    pool_size: Option<usize>,
    state: Option<Value>,
    store_path: Option<PathBuf>,
    timeout: Option<Duration>,
}

#[tokio::main]
async fn main() -> ExitCode {
    if let Err(e) = try_main().await {
        match e.downcast_ref::<LmbError>() {
            Some(LmbError::Lua(..)) => { /* error has been reported */ }
            _ => eprintln!("Error: {e}"),
        }

        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
