//! HTTP client binding module.
//!
//! This module provides HTTP client functionality for making web requests.
//! Import via `require("@lmb/http")`.
//!
//! # Available Methods
//!
//! - `fetch(url, options)` - Make an HTTP request and return a response object.
//! - `parse_path(path, pattern)` - Extract path parameters from a URL path using a pattern.
//!
//! # Options
//!
//! The `fetch` method accepts an optional table with the following fields:
//! - `method` - HTTP method (GET, POST, PUT, DELETE, etc.). Default: GET.
//! - `headers` - Table of HTTP headers.
//! - `body` - Request body string.
//! - `timeout` - Request timeout (number in seconds or duration string like "5s").
//!
//! # Response Object
//!
//! The response object has the following properties and methods:
//! - `status` - HTTP status code (number).
//! - `ok` - Boolean indicating if status is 2xx.
//! - `headers` - Table of response headers.
//! - `text()` - Get response body as string.
//! - `json()` - Parse response body as JSON.
//!
//! # `parse_path`
//!
//! Extracts named parameters from a URL path by matching it against a pattern.
//! Returns a table of parameter key-value pairs on match, or `nil` if the path
//! does not match the pattern.
//!
//! Pattern syntax uses `{name}` for named parameters and `{*name}` for catch-all
//! parameters (powered by the `matchit` crate).
//!
//! # Example
//!
//! ```lua
//! local http = require("@lmb/http")
//!
//! -- Simple GET request
//! local response = http:fetch("https://api.example.com/data")
//! if response.ok then
//!     local data = response:json()
//!     print(data.message)
//! end
//!
//! -- POST request with headers and body
//! local response = http:fetch("https://api.example.com/submit", {
//!     method = "POST",
//!     headers = {
//!         ["Content-Type"] = "application/json",
//!         ["Authorization"] = "Bearer token123"
//!     },
//!     body = '{"key": "value"}',
//!     timeout = 30
//! })
//!
//! print(response.status)  -- e.g., 200
//! print(response:text())  -- Response body as text
//!
//! -- Parse path parameters
//! local params = http.parse_path("/users/42", "/users/{id}")
//! print(params.id)  -- "42"
//! ```

use std::{str::FromStr, sync::Arc, time::Duration};

use bon::bon;
use bytes::BytesMut;
use matchit::Router;
use mlua::prelude::*;
use reqwest::{
    Method, StatusCode,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use serde_json::{Value, json};
use std::sync::OnceLock;
use tokio::sync::Mutex;
use tracing::{Instrument, debug_span};
use url::Url;

use crate::Permissions;

pub(crate) struct ResponseBinding {
    headers: Value,
    inner: Arc<Mutex<reqwest::Response>>,
    status: StatusCode,
}

impl LuaUserData for ResponseBinding {
    fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("headers", |vm, this| vm.to_value(&this.headers));
        fields.add_field_method_get("ok", |_, this| Ok(this.status.is_success()));
        fields.add_field_method_get("status", |_, this| Ok(this.status.as_u16()));
    }
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method("json", |vm, this, ()| async move {
            let mut buf = BytesMut::new();
            while let Some(chunk) = this.inner.lock().await.chunk().await.into_lua_err()? {
                buf.extend_from_slice(&chunk);
            }
            let value: Value = serde_json::from_slice(&buf).into_lua_err()?;
            vm.to_value(&value)
        });
        methods.add_async_method("text", |_, this, ()| async move {
            let mut buf = BytesMut::new();
            while let Some(chunk) = this.inner.lock().await.chunk().await.into_lua_err()? {
                buf.extend_from_slice(&chunk);
            }
            String::from_utf8(buf.to_vec()).into_lua_err()
        });
    }
}

pub(crate) struct HttpBinding {
    client: OnceLock<reqwest::Client>,
    permissions: Option<Permissions>,
    timeout: Option<Duration>,
}

#[bon]
impl HttpBinding {
    #[builder]
    pub(crate) fn new(permissions: Option<Permissions>, timeout: Option<Duration>) -> Self {
        Self {
            client: OnceLock::new(),
            permissions,
            timeout,
        }
    }

    fn client(&self) -> &reqwest::Client {
        self.client.get_or_init(|| {
            let mut builder = reqwest::Client::builder();
            if let Some(timeout) = self.timeout {
                builder = builder.timeout(timeout);
            }
            builder.build().expect("failed to build HTTP client")
        })
    }
}

impl LuaUserData for HttpBinding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method(
            "fetch",
            |_, this, (url, options): (String, Option<LuaTable>)| async move {
                let options = serde_json::to_value(&options).into_lua_err()?;
                let method = options
                    .pointer("/method")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Method::from_str(&s.to_ascii_uppercase()).ok())
                    .unwrap_or_default();
                let headers = options
                    .pointer("/headers")
                    .and_then(|v| v.as_object())
                    .map(|obj| {
                        let mut headers = HeaderMap::new();
                        for (k, v) in obj {
                            let k = k.parse::<HeaderName>().ok();
                            let v = v.as_str().and_then(|s| s.parse::<HeaderValue>().ok());
                            if let (Some(k), Some(v)) = (k, v) {
                                headers.insert(k, v);
                            }
                        }
                        headers
                    });
                let body = options.pointer("/body").and_then(|v| v.as_str());

                let url = Url::parse(&url).into_lua_err()?;
                if let Some(perm) = &this.permissions
                    && !perm.is_url_allowed(&url)
                {
                    return Err(LuaError::runtime("URL is not allowed"));
                }

                let mut built = this.client().request(method.clone(), url.clone());
                if let Some(headers) = headers {
                    built = built.headers(headers);
                }
                if let Some(body) = body {
                    built = built.body(body.to_string());
                }
                if let Some(timeout) = options.pointer("/timeout") {
                    match timeout {
                        Value::Number(n) if n.is_u64() => {
                            let secs = n.as_u64().unwrap_or(0);
                            built = built.timeout(Duration::from_secs(secs));
                        }
                        Value::String(s) => {
                            let timeout = jiff::Span::from_str(s).into_lua_err()?;
                            built = built.timeout(Duration::try_from(timeout).into_lua_err()?);
                        }
                        _ => return Err(LuaError::runtime("Invalid timeout value")),
                    }
                }

                let request = built.build().into_lua_err()?;
                let response = {
                    let span = debug_span!("send_http_request", method = %method, url = %url);
                    this.client()
                        .execute(request)
                        .instrument(span)
                        .await
                        .into_lua_err()?
                };
                let headers = {
                    let mut m = json!({});
                    let headers = response.headers().clone();
                    for (k, v) in headers {
                        if let Some(k) = k
                            && let Ok(v) = v.to_str()
                        {
                            m[k.as_str()] = json!(v.to_string());
                        }
                    }
                    m
                };
                let status_code = response.status();
                let inner = Arc::new(Mutex::new(response));
                Ok(ResponseBinding {
                    headers,
                    inner,
                    status: status_code,
                })
            },
        );
        methods.add_function("parse_path", |vm, (path, pattern): (String, String)| {
            let mut router = Router::new();
            router.insert(&pattern, ()).into_lua_err()?;
            match router.at(&path) {
                Ok(matched) => {
                    let table = vm.create_table()?;
                    for (key, value) in matched.params.iter() {
                        table.set(key, value)?;
                    }
                    Ok(LuaValue::Table(table))
                }
                Err(_) => Ok(LuaNil),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use reqwest::Method;
    use serde_json::json;
    use tokio::io::empty;

    use crate::{Runner, State};

    #[tokio::test]
    async fn test_http_get() {
        let mut server = mockito::Server::new_async().await;

        let url = server.url();
        let mock = server
            .mock(Method::GET.as_str(), "/")
            .with_status(200)
            .with_body("Hello, world!")
            .create_async()
            .await;

        let source = include_str!("../fixtures/bindings/http-get.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        let state = State::builder().state(json!(url)).build();
        let result = runner.invoke().state(state).call().await.unwrap();

        assert_eq!(json!("Hello, world!"), result.result.unwrap());

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_http_post() {
        let mut server = mockito::Server::new_async().await;

        let url = server.url();
        let mock = server
            .mock(Method::POST.as_str(), "/")
            .match_header("x-api-key", "api-key")
            .match_body("a")
            .with_status(201)
            .with_body(r#"{"a":1}"#)
            .with_header("mocked", "1")
            .create_async()
            .await;

        let source = include_str!("../fixtures/bindings/http-post.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        let state = State::builder().state(json!(url)).build();
        let result = runner.invoke().state(state).call().await.unwrap();

        assert_eq!(json!({"a":1}), result.result.unwrap());

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_parse_path() {
        let source = include_str!("../fixtures/bindings/http-parse-path.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }
}
