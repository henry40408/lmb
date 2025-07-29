use std::{str::FromStr, sync::Arc, time::Duration};

use bon::bon;
use bytes::BytesMut;
use mlua::prelude::*;
use parking_lot::Mutex;
use reqwest::{
    Method,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use serde_json::Value;
use url::Url;

use crate::LmbResult;

pub(crate) struct ResponseBinding(Arc<Mutex<reqwest::Response>>);

impl LuaUserData for ResponseBinding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method("json", |vm, this, ()| async move {
            let mut buf = BytesMut::new();
            while let Some(chunk) = this.0.lock().chunk().await.into_lua_err()? {
                buf.extend_from_slice(&chunk);
            }
            let value: Value = serde_json::from_slice(&buf).into_lua_err()?;
            vm.to_value(&value).into_lua_err()
        });
        methods.add_async_method("text", |_, this, ()| async move {
            let mut buf = BytesMut::new();
            while let Some(chunk) = this.0.lock().chunk().await.into_lua_err()? {
                buf.extend_from_slice(&chunk);
            }
            String::from_utf8(buf.to_vec()).into_lua_err()
        });
    }
}

pub(crate) struct HttpBinding {
    pub(crate) client: reqwest::Client,
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
            async move |_, this, (url, options): (String, Option<LuaTable>)| {
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
                let mut built = this.client.request(method, url);
                if let Some(headers) = headers {
                    built = built.headers(headers);
                }
                if let Some(body) = body {
                    built = built.body(body.to_string());
                }

                let request = built.build().into_lua_err()?;
                let response = this.client.execute(request).await.into_lua_err()?;
                Ok(ResponseBinding(Arc::new(Mutex::new(response))))
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use reqwest::Method;
    use serde_json::{Value, json};
    use tokio::io::empty;

    use crate::Runner;

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
        let runner = Runner::builder(&source, empty()).build().unwrap();
        let result = runner
            .invoke()
            .state(Value::String(url))
            .call()
            .await
            .unwrap();

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
            .with_status(200)
            .with_body(r#"{"a":1}"#)
            .create_async()
            .await;

        let source = include_str!("fixtures/http-post.lua");
        let runner = Runner::builder(&source, empty()).build().unwrap();
        let result = runner
            .invoke()
            .state(Value::String(url))
            .call()
            .await
            .unwrap();

        assert_eq!(json!({"a":1}), result.result.unwrap());

        mock.assert_async().await;
    }
}
