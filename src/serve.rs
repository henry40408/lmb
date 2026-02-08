use std::{collections::HashMap, io::Cursor, str::FromStr as _, sync::Arc};

use axum::{
    body::{Body, to_bytes},
    extract::{Query, Request, State},
    http::{HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use base64::prelude::*;
use lmb::{LmbResult, Runner, pool::Pool, reader::SharedReader};
use mlua::ExternalResult;
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::io::empty;
use tracing::{Instrument as _, debug, debug_span, error};

use crate::{AppState, open_store_connection};

/// Type alias for the Runner pool used in serve mode.
pub(crate) type RunnerPool = Pool<String>;

/// Creates a new Runner pool with the given configuration.
pub(crate) fn create_pool(app_state: &AppState) -> anyhow::Result<RunnerPool> {
    let reader = Arc::new(SharedReader::new(empty()));

    let store = open_store_connection(
        app_state.store_path.clone(),
        app_state.no_store.unwrap_or(false),
    )?
    .map(|conn| Arc::new(Mutex::new(conn)));

    let manager = lmb::pool::RunnerManager::builder(app_state.source.clone(), reader)
        .maybe_store(store)
        .build();

    let pool_size = app_state.pool_size.unwrap_or(8);
    let pool = Pool::builder(manager).max_size(pool_size).build()?;
    Ok(pool)
}

pub(crate) struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        // Log error internally, don't expose details to clients
        error!("Internal error: {:?}", self.0);
        (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
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

fn decode_base64_string(is_base64_encoded: bool, s: &String) -> LmbResult<Vec<u8>> {
    Ok(if is_base64_encoded {
        BASE64_STANDARD.decode(s.as_bytes()).into_lua_err()?
    } else {
        s.as_bytes().to_vec()
    })
}

async fn try_request_handler(
    app_state: Arc<AppState>,
    pool: Option<Arc<RunnerPool>>,
    query: HashMap<String, String>,
    req: Request,
) -> anyhow::Result<Response<Body>> {
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

    let bytes = to_bytes(
        req.into_body(),
        app_state.max_body_size.unwrap_or(10 * 1_024 * 1_024),
    )
    .await?;

    let request = json!({ "headers": headers, "method": method, "path": path, "query": query });
    let state = lmb::State::builder()
        .maybe_state(app_state.state.clone())
        .request(request)
        .build();

    debug!("Evaluating Lua code");
    let res = if let Some(pool) = pool {
        // Pool mode: get runner from pool and swap reader with request body
        let runner = pool.get().await?;
        runner.swap_reader(Cursor::new(bytes)).await;
        runner.invoke().state(state).call().await?
    } else {
        // Non-pool mode: create new runner per request
        let reader = Cursor::new(bytes);
        let conn = open_store_connection(
            app_state.store_path.clone(),
            app_state.no_store.unwrap_or(false),
        )?;

        let runner = Runner::builder(app_state.source.clone(), reader)
            .maybe_default_name(app_state.name.clone())
            .maybe_http_timeout(app_state.http_timeout)
            .maybe_permissions(app_state.permissions.clone())
            .maybe_store(conn)
            .maybe_timeout(app_state.timeout)
            .build()?;
        runner.invoke().state(state).call().await?
    };

    match res.result {
        Ok(output) => {
            debug!("Request succeeded: {output}");
            match output {
                Value::String(s) => Ok(Response::new(s.into())),
                Value::Object(_) => {
                    let is_base64_encoded = output
                        .pointer("/is_base64_encoded")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let body = output
                        .pointer("/body")
                        .and_then(|v| match v {
                            Value::String(s) => decode_base64_string(is_base64_encoded, s).ok(),
                            _ => decode_base64_string(is_base64_encoded, &v.to_string()).ok(),
                        })
                        .unwrap_or_default();
                    let mut res = Response::new(body.into());

                    let status_code = output.pointer("/status_code").and_then(|v| v.as_u64());
                    if let Some(status_code) = status_code
                        && let Ok(status_code) = u16::try_from(status_code)
                    {
                        *res.status_mut() = StatusCode::from_u16(status_code)?;
                    }

                    let headers = output.pointer("/headers").and_then(|v| v.as_object());
                    if let Some(m) = headers {
                        for (k, v) in m {
                            let v = match v {
                                Value::String(s) => s,
                                _ => &v.to_string(),
                            };
                            let k = HeaderName::from_str(k.as_str())?;
                            let v = HeaderValue::from_str(v)?;
                            res.headers_mut().insert(k, v);
                        }
                    }

                    Ok(res)
                }
                v => Ok(Response::new(v.to_string().into())),
            }
        }
        Err(err) => {
            error!("Request failed: {err:?}");
            Err(err.into())
        }
    }
}

pub(crate) async fn request_handler(
    State((app_state, pool)): State<(Arc<AppState>, Option<Arc<RunnerPool>>)>,
    Query(query): Query<HashMap<String, String>>,
    request: Request,
) -> Result<Response<Body>, AppError> {
    let span = debug_span!("handle_request");
    let res = try_request_handler(app_state, pool, query, request)
        .instrument(span)
        .await?;
    Ok(res)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum_test::TestServer;
    use serde_json::{Value, json};

    use crate::{AppState, build_router, serve::create_pool};

    #[tokio::test]
    async fn test_serve() {
        let source = include_str!("./fixtures/serve/serve.lua");
        let app_state = Arc::new(AppState::builder().source(source).build());
        let router = build_router(app_state, None);
        let server = TestServer::new(router).unwrap();

        let res = server.get("/").await;
        assert_eq!(201, res.status_code());
        assert_eq!("text/html", res.headers().get("content-type").unwrap());
        assert_eq!("<h1>Hello, World!</h1>", res.text());
    }

    #[tokio::test]
    async fn test_serve_echo() {
        let source = include_str!("./fixtures/serve/serve-echo.lua");
        let app_state = Arc::new(AppState::builder().source(source).build());
        let router = build_router(app_state, None);
        let server = TestServer::new(router).unwrap();

        let res = server
            .post("/a/b/c?a=1&b=2")
            .add_header("i-am", "teapot")
            .json(&json!({ "foo": 1, "bar": 2 }))
            .await;
        assert_eq!(200, res.status_code());
        assert_eq!(
            json!({
                "body": { "foo": 1, "bar": 2 },
                "headers": { "content-type": "application/json", "i-am": "teapot" },
                "method": "POST",
                "path": "/a/b/c",
                "query": { "a": "1", "b": "2" }
            }),
            res.json::<Value>()
        );
    }

    #[tokio::test]
    async fn test_serve_base64() {
        let source = include_str!("./fixtures/serve/serve-base64.lua");
        let app_state = Arc::new(AppState::builder().source(source).build());
        let router = build_router(app_state, None);
        let server = TestServer::new(router).unwrap();

        let res = server.get("/").await;
        assert_eq!(200, res.status_code());
        assert_eq!("hello world", res.text());
    }

    #[tokio::test]
    async fn test_serve_with_pool() {
        let source = include_str!("./fixtures/serve/serve.lua");
        let app_state = Arc::new(AppState::builder().source(source).pool_size(2).build());
        let pool = create_pool(&app_state).unwrap();
        let router = build_router(app_state, Some(Arc::new(pool)));
        let server = TestServer::new(router).unwrap();

        // Make multiple requests to exercise pool
        for _ in 0..4 {
            let res = server.get("/").await;
            assert_eq!(201, res.status_code());
            assert_eq!("<h1>Hello, World!</h1>", res.text());
        }
    }

    #[tokio::test]
    async fn test_serve_string_response() {
        let source = r#"return function() return "plain text" end"#;
        let app_state = Arc::new(AppState::builder().source(source).build());
        let router = build_router(app_state, None);
        let server = TestServer::new(router).unwrap();

        let res = server.get("/").await;
        assert_eq!(200, res.status_code());
        assert_eq!("plain text", res.text());
    }

    #[tokio::test]
    async fn test_serve_number_response() {
        let source = r#"return function() return 42 end"#;
        let app_state = Arc::new(AppState::builder().source(source).build());
        let router = build_router(app_state, None);
        let server = TestServer::new(router).unwrap();

        let res = server.get("/").await;
        assert_eq!(200, res.status_code());
        assert_eq!("42", res.text());
    }

    #[tokio::test]
    async fn test_serve_error_response() {
        let source = r#"return function() error("test error") end"#;
        let app_state = Arc::new(AppState::builder().source(source).build());
        let router = build_router(app_state, None);
        let server = TestServer::new(router).unwrap();

        let res = server.get("/").await;
        assert_eq!(500, res.status_code());
    }

    #[tokio::test]
    async fn test_create_pool_with_store() {
        let source = r#"return function() return "ok" end"#;
        let app_state = Arc::new(
            AppState::builder()
                .source(source)
                .pool_size(2)
                .no_store(false)
                .build(),
        );
        let pool = create_pool(&app_state).unwrap();
        let runner = pool.get().await.unwrap();
        let res = runner.invoke().call().await.unwrap();
        assert_eq!(json!("ok"), res.result.unwrap());
    }

    #[tokio::test]
    async fn test_create_pool_no_store() {
        let source = r#"return function() return "ok" end"#;
        let app_state = Arc::new(
            AppState::builder()
                .source(source)
                .pool_size(2)
                .no_store(true)
                .build(),
        );
        let pool = create_pool(&app_state).unwrap();
        let runner = pool.get().await.unwrap();
        let res = runner.invoke().call().await.unwrap();
        assert_eq!(json!("ok"), res.result.unwrap());
    }
}
