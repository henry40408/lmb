use std::{
    collections::HashMap,
    io::{Cursor, Read},
    path::PathBuf,
    process::ExitCode,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use anyhow::bail;
use axum::{
    Router,
    body::to_bytes,
    extract::{Query, Request, State},
    http::{HeaderName, HeaderValue, Response, status::StatusCode},
    response::IntoResponse,
    routing::any,
};
use byte_unit::Byte;
use clap::{Parser, Subcommand};
use clio::Input;
use lmb::{
    Runner,
    error::{ErrorReport, build_report, render_report},
};
use no_color::is_no_color;
use rusqlite::Connection;
use serde_json::{Value, json};
use tokio::io::{self, AsyncWriteExt as _};
use tracing::{Instrument, Level, debug, debug_span, error};
use tracing_subscriber::{EnvFilter, fmt::format::FmtSpan};

const VERSION: &str = env!("APP_VERSION");

#[derive(Debug, Parser)]
#[clap(author, version=VERSION, about, long_about = None)]
struct Opts {
    /// Allowed environment variables
    #[clap(long, value_delimiter = ',', env = "ALLOW_ENV")]
    allow_env: Vec<String>,
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
    #[clap(long, action, group = "store", env = "NO_STORE")]
    no_store: bool,
    /// Optional path to the store file
    #[clap(long, value_parser, group = "store", env = "STORE_PATH")]
    store_path: Option<PathBuf>,
    /// Timeout. Default is 30 seconds, set to 0 for no timeout
    #[clap(long, env = "TIMEOUT")]
    timeout: Option<jiff::Span>,
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Evaluate a file
    Eval {
        /// Path to the file to evaluate
        #[clap(long, value_parser, env = "FILE_PATH")]
        file: Input,
        /// Optional state to pass to the Lua script
        #[clap(long, env = "STATE")]
        state: Option<String>,
    },
    /// Serve a file
    Serve {
        /// Bound address and port
        #[clap(long, default_value = "127.0.0.1:3000", env = "BIND")]
        bind: String,
        /// Path to the file to serve
        #[clap(long, value_parser, env = "FILE_PATH")]
        file: Input,
        /// Optional maximum body size
        #[clap(long, default_value = "100M", env = "MAX_BODY_SIZE")]
        max_body_size: String,
        /// Optional state to pass to the Lua script
        #[clap(long, env = "STATE")]
        state: Option<String>,
    },
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

    match opts.command {
        Command::Eval { mut file, state } => {
            let span = debug_span!("eval").entered();
            debug!("Evaluate Lua script: {file:?}");

            let state = state.as_ref().map(|s| match serde_json::from_str(s) {
                Ok(value) => value,
                Err(_) => json!(s.clone()), // treat invalid value as string
            });
            debug!("State: {state:?}");

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
            debug!("Using HTTP timeout: {:?}", http_timeout);

            let timeout = match opts.timeout {
                None => Some(Duration::from_secs(30)),
                Some(t) if t.is_zero() => None,
                Some(t) => Some(Duration::try_from(t)?),
            };
            debug!("Using timeout: {:?}", timeout);

            let conn = match (opts.store_path, opts.no_store) {
                (None, false) => Some(Connection::open_in_memory()?),
                (Some(path), false) => Some(Connection::open(path)?),
                _ => None,
            };

            let runner = if let Some(source) = &source {
                debug!("Evaluating Lua code from stdin or a string input");
                Runner::builder(source, reader)
                    .allow_env(opts.allow_env)
                    .default_name("(stdin)")
                    .maybe_http_timeout(http_timeout)
                    .maybe_store(conn)
                    .maybe_timeout(timeout)
                    .build()?
            } else {
                debug!("Evaluating Lua code from file: {:?}", file.path().path());
                Runner::builder(file.path().path(), reader)
                    .allow_env(opts.allow_env)
                    .maybe_http_timeout(http_timeout)
                    .maybe_store(conn)
                    .maybe_timeout(timeout)
                    .build()?
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
        Command::Serve {
            bind,
            mut file,
            max_body_size,
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

            let http_timeout = match opts.http_timeout {
                None => Some(Duration::from_secs(30)),
                Some(t) if t.is_zero() => None,
                Some(t) => Some(Duration::try_from(t)?),
            };
            debug!("Using HTTP timeout: {:?}", http_timeout);

            let timeout = match opts.timeout {
                None => Some(Duration::from_secs(30)),
                Some(t) if t.is_zero() => None,
                Some(t) => Some(Duration::try_from(t)?),
            };
            debug!("Using timeout: {:?}", timeout);

            let mut source = String::new();
            file.read_to_string(&mut source)?;

            let name = if file.is_local() {
                file.path().to_string_lossy().to_string()
            } else if file.is_std() {
                "(stdin)".to_string()
            } else {
                bail!("Expected a local file or a stdin input, but got: {file}");
            };

            let app_state = Arc::new(AppState {
                allow_env: opts.allow_env.clone(),
                http_timeout,
                max_body_size,
                name,
                no_store: opts.no_store,
                state,
                store_path: opts.store_path.clone(),
                source,
                timeout,
            });
            let app = Router::new()
                .route("/{*wildcard}", any(request_handler))
                .route("/", any(request_handler))
                .with_state(app_state);
            let listener = tokio::net::TcpListener::bind(&bind).await?;
            debug!("Listening on {}", listener.local_addr()?);
            axum::serve(listener, app).await?;
        }
    }

    Ok(())
}

#[derive(Clone)]
struct AppState {
    allow_env: Vec<String>,
    http_timeout: Option<Duration>,
    max_body_size: usize,
    name: String,
    no_store: bool,
    state: Option<Value>,
    store_path: Option<PathBuf>,
    source: String,
    timeout: Option<Duration>,
}

struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Something went wrong {}", self.0),
        )
            .into_response()
    }
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

async fn try_request_handler(
    app_state: Arc<AppState>,
    query: HashMap<String, String>,
    req: Request,
) -> anyhow::Result<Response<String>> {
    let method = json!(req.method().as_str());
    let path = json!(req.uri().path());
    let headers = {
        let mut m = json!({});
        for (k, v) in req.headers() {
            m[k.as_str()] = json!(v.to_str()?);
        }
        m
    };
    let query = json!(query);

    let bytes = to_bytes(req.into_body(), app_state.max_body_size).await?;
    let reader = Cursor::new(bytes);

    let conn = match (app_state.store_path.clone(), app_state.no_store) {
        (None, false) => Some(Connection::open_in_memory()?),
        (Some(path), false) => Some(Connection::open(path)?),
        _ => None,
    };

    debug!("Evaluating Lua code");
    let runner = Runner::builder(app_state.source.clone(), reader)
        .allow_env(app_state.allow_env.clone())
        .default_name(app_state.name.clone())
        .maybe_http_timeout(app_state.http_timeout)
        .maybe_store(conn)
        .maybe_timeout(app_state.timeout)
        .build()?;

    let request = json!({ "headers": headers, "method": method, "path": path, "query": query });
    let state = lmb::State::builder()
        .maybe_state(app_state.state.clone())
        .request(request)
        .build();
    let res = runner.invoke().state(state).call().await?;

    match res.result {
        Ok(output) => {
            debug!("Request succeeded: {output}");
            match output {
                Value::String(s) => Ok(Response::new(s)),
                Value::Object(_) => {
                    let body = output
                        .pointer("/body")
                        .map(|v| match v {
                            Value::String(s) => s.clone(),
                            _ => v.to_string(),
                        })
                        .unwrap_or_default();
                    let mut res = Response::new(body);

                    let status_code = output.pointer("/status_code").and_then(|v| v.as_u64());
                    if let Some(status_code) = status_code {
                        if let Ok(status_code) = u16::try_from(status_code) {
                            *res.status_mut() = StatusCode::from_u16(status_code)?;
                        }
                    }

                    let headers = output.pointer("/headers").and_then(|v| v.as_object());
                    if let Some(m) = headers {
                        for (k, v) in m {
                            if let Some(s) = v.as_str() {
                                let k = HeaderName::from_str(k.as_str())?;
                                let v = HeaderValue::from_str(s)?;
                                res.headers_mut().insert(k, v);
                            }
                        }
                    }

                    Ok(res)
                }
                v => Ok(Response::new(v.to_string())),
            }
        }
        Err(err) => {
            error!("Request failed: {err:?}");
            Err(err.into())
        }
    }
}

async fn request_handler(
    State(app_state): State<Arc<AppState>>,
    Query(query): Query<HashMap<String, String>>,
    request: Request,
) -> Result<Response<String>, AppError> {
    let span = debug_span!("handle_request");
    let res = try_request_handler(app_state, query, request)
        .instrument(span)
        .await?;
    Ok(res)
}

#[tokio::main]
async fn main() -> ExitCode {
    if let Err(e) = try_main().await {
        eprintln!("Error: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
