use axum::{
    Router,
    body::Bytes,
    extract::{Path, State as AxumState},
    http::{HeaderMap, Method, StatusCode},
    response::IntoResponse,
    routing::any,
};
use bon::Builder;
use http::{HeaderName, HeaderValue};
use lmb::{Evaluation, LuaSource, State, StateKey, Store};
use serde_json::{Map, Value};
use std::{
    collections::HashMap, io::Cursor, net::SocketAddr, path::PathBuf, str::FromStr as _, sync::Arc,
    time::Duration,
};
use tower_http::trace::{self, TraceLayer};
use tracing::{Level, error, info, warn};

#[derive(Builder, Clone)]
struct AppState {
    #[builder(start_fn)]
    source: LuaSource,
    json: Option<bool>,
    store: Store,
    timeout: Option<Duration>,
}

#[derive(Builder)]
pub struct ServeOptions {
    #[builder(start_fn, into)]
    bind: SocketAddr,
    #[builder(start_fn)]
    source: LuaSource,
    json: Option<bool>,
    run_migrations: Option<bool>,
    store_path: Option<PathBuf>,
    timeout: Option<Duration>,
}

fn do_handle_request<S>(
    state: AppState,
    method: Method,
    path: S,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse
where
    S: AsRef<str>,
{
    let e = match Evaluation::builder(state.source.clone(), Cursor::new(body))
        .maybe_timeout(state.timeout)
        .store(state.store.clone())
        .build()
    {
        Ok(e) => e,
        Err(err) => {
            error!(?err, "failed to compile Lua code");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                HeaderMap::new(),
                String::new(),
            );
        }
    };

    let mut headers_map: Map<_, Value> = Map::new();
    for (name, value) in headers {
        if let Some(name) = name {
            let value = value.to_str().unwrap_or("");
            headers_map.insert(name.to_string(), value.into());
        }
    }

    let mut request_map: Map<_, Value> = Map::new();
    request_map.insert("method".into(), method.as_str().into());
    request_map.insert("path".into(), path.as_ref().into());
    request_map.insert("headers".into(), headers_map.into());

    let eval_state = Arc::new(State::new());
    eval_state.insert(StateKey::Request, request_map.into());

    let res = e.evaluate().state(eval_state.clone()).call();
    let json = state.json.unwrap_or_default();
    match res {
        Ok(res) => match build_response(json, eval_state, &res.payload) {
            Ok(t) => t,
            Err(err) => {
                error!(?err, "failed to build response");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    HeaderMap::new(),
                    String::new(),
                )
            }
        },
        Err(err) => {
            error!(?err, "failed to run Lua script");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                HeaderMap::new(),
                String::new(),
            )
        }
    }
}

fn build_response(
    json: bool,
    state: Arc<State>,
    value: &Value,
) -> anyhow::Result<(StatusCode, HeaderMap, String)> {
    let (status_code, headers) = state
        .view(&StateKey::Response, |_k, res| {
            let status_code = res
                .pointer("/status_code")
                .and_then(|s| s.as_u64())
                .unwrap_or(200u64);
            let mut m = HashMap::new();
            if let Some(h) = res.get("headers").and_then(|h| h.as_object()) {
                for (name, value) in h.iter() {
                    m.insert(
                        name.to_owned().into_boxed_str(),
                        match value {
                            Value::String(s) => s.to_owned().into_boxed_str(),
                            _ => value.to_string().into_boxed_str(),
                        },
                    );
                }
            }
            (status_code, m)
        })
        .unwrap_or_else(|| (200u64, HashMap::new()));

    let status_code = StatusCode::from_u16(u16::try_from(status_code)?)?;
    let mut header_map = HeaderMap::new();
    for (name, value) in headers.iter() {
        header_map.insert(HeaderName::from_str(name)?, HeaderValue::from_str(value)?);
    }
    let body = if json {
        serde_json::to_string(&value)?
    } else {
        match value {
            Value::String(s) => s.to_owned(),
            _ => value.to_string(),
        }
    };
    Ok((status_code, header_map, body))
}

async fn index_route(
    AxumState(state): AxumState<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    do_handle_request(state, method, "/", headers, body)
}

async fn match_all_route(
    AxumState(state): AxumState<AppState>,
    method: Method,
    Path(path): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let path = format!("/{path}");
    do_handle_request(state, method, path, headers, body)
}

pub fn init_route(opts: &ServeOptions) -> anyhow::Result<Router> {
    let store = if let Some(path) = &opts.store_path {
        let store = Store::builder()
            .maybe_run_migrations(opts.run_migrations)
            .path(path.as_path())
            .build()?;
        info!(?path, "open store");
        store
    } else {
        let store = Store::default();
        warn!(
            "no store path is specified, an in-memory store will be used and values will be lost when process ends"
        );
        store
    };
    let app_state = AppState::builder(opts.source.clone())
        .maybe_json(opts.json)
        .store(store)
        .maybe_timeout(opts.timeout)
        .build();
    let app = Router::new()
        .route("/", any(index_route))
        .route("/{*path}", any(match_all_route))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
                .on_response(trace::DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(app_state);
    Ok(app)
}

pub async fn serve_file(opts: &ServeOptions) -> anyhow::Result<()> {
    let bind = &opts.bind;
    let app = init_route(opts)?;
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    let local_addr: SocketAddr = listener.local_addr()?;
    info!(bind = %local_addr, "serving lua script");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::init_route;
    use crate::serve::ServeOptions;
    use axum_test::TestServer;
    use http::HeaderValue;
    use serde_json::{Value, json};
    use std::net::SocketAddr;

    #[tokio::test]
    async fn echo_request() {
        let addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let script = include_str!("fixtures/echo-request.lua").into();
        let opts = ServeOptions::builder(addr, script).json(true).build();

        let router = init_route(&opts).unwrap();
        let server = TestServer::new(router.into_make_service()).unwrap();
        let res = server.post("/foo/bar/baz").json(&json!({"a":1})).await;
        assert_eq!(200, res.status_code());

        let value: Value = serde_json::from_str(&res.text()).unwrap();
        let expected = json!({
            "body": r#"{"a":1}"#,
            "request": {
                "headers": {
                    "content-type": "application/json",
                },
                "method": "POST",
                "path": "/foo/bar/baz",
            },
        });
        assert_eq!(expected, value);
    }

    #[tokio::test]
    async fn headers_status_code() {
        let addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let script = include_str!("fixtures/headers-status-code.lua").into();
        let opts = ServeOptions::builder(addr, script).build();

        let router = init_route(&opts).unwrap();
        let server = TestServer::new(router.into_make_service()).unwrap();
        let res = server.post("/").await;
        assert_eq!(418, res.status_code());
        assert_eq!(
            HeaderValue::from_static("a teapot"),
            res.headers().get("whoami").unwrap()
        );
        assert_eq!("I'm a teapot.", res.text());
    }

    #[tokio::test]
    async fn headers_status_code_bad_script() {
        let addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let script = "ret 'hello'".into();
        let opts = ServeOptions::builder(addr, script).build();

        let router = init_route(&opts).unwrap();
        let server = TestServer::new(router.into_make_service()).unwrap();
        let res = server.post("/").await;
        assert_eq!(500, res.status_code());
        assert_eq!("", res.text());
    }

    #[tokio::test]
    async fn headers_status_code_invalid_status_code() {
        let addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let script = include_str!("fixtures/invalid-status-code.lua").into();
        let opts = ServeOptions::builder(addr, script).build();

        let router = init_route(&opts).unwrap();
        let server = TestServer::new(router.into_make_service()).unwrap();
        let res = server.post("/").await;
        assert_eq!(500, res.status_code());
        assert_eq!("", res.text());
    }

    #[tokio::test]
    async fn json_string() {
        let addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let script = "return 'hello'".into();
        let opts = ServeOptions::builder(addr, script).json(true).build();

        let router = init_route(&opts).unwrap();
        let server = TestServer::new(router.into_make_service()).unwrap();
        let res = server.post("/").await;
        assert_eq!(200, res.status_code());
        assert_eq!(r#""hello""#, res.text());
    }

    #[tokio::test]
    async fn number() {
        let addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let script = "return 1".into();
        let opts = ServeOptions::builder(addr, script).build();

        let router = init_route(&opts).unwrap();
        let server = TestServer::new(router.into_make_service()).unwrap();
        let res = server.post("/").await;
        assert_eq!(200, res.status_code());
        assert_eq!("1", res.text());
    }

    #[tokio::test]
    async fn raw_string() {
        let addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let script = "return 'hello'".into();
        let opts = ServeOptions::builder(addr, script).build();

        let router = init_route(&opts).unwrap();
        let server = TestServer::new(router.into_make_service()).unwrap();
        let res = server.post("/").await;
        assert_eq!(200, res.status_code());
        assert_eq!("hello", res.text());
    }

    #[tokio::test]
    async fn serve() {
        let addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let script = "return 1".into();
        let opts = ServeOptions::builder(addr, script).json(true).build();

        let router = init_route(&opts).unwrap();
        let server = TestServer::new(router.into_make_service()).unwrap();
        let res = server.post("/").await;
        assert_eq!(200, res.status_code());
        assert_eq!("1", res.text());
    }
}
