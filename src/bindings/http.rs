use std::{str::FromStr, sync::Arc, time::Duration};

use bon::bon;
use bytes::BytesMut;
use mlua::prelude::*;
use reqwest::{
    Method, StatusCode,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tracing::debug_span;
use url::Url;

use crate::LmbResult;

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
    client: reqwest::Client,
}

#[bon]
impl HttpBinding {
    #[builder]
    pub(crate) fn new(timeout: Option<Duration>) -> LmbResult<Self> {
        let mut client = reqwest::Client::builder();
        if let Some(timeout) = timeout {
            client = client.timeout(timeout);
        }
        let client = client.build()?;
        Ok(Self { client })
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
                let mut built = this.client.request(method.clone(), url.clone());
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
                    let _ =
                        debug_span!("send_http_request", method = %method, url = %url).entered();
                    this.client.execute(request).await.into_lua_err()?
                };
                let headers = {
                    let mut m = json!({});
                    let headers = response.headers().clone();
                    for (k, v) in headers {
                        if let Some(k) = k {
                            if let Ok(v) = v.to_str() {
                                m[k.as_str()] = json!(v.to_string());
                            }
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

        let source = include_str!("fixtures/http-get.lua");
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

        let source = include_str!("fixtures/http-post.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        let state = State::builder().state(json!(url)).build();
        let result = runner.invoke().state(state).call().await.unwrap();

        assert_eq!(json!({"a":1}), result.result.unwrap());

        mock.assert_async().await;
    }
}
