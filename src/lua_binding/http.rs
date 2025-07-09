use std::{collections::HashMap, str::FromStr, sync::Arc};

use bytes::BytesMut;
use http::{
    HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri,
    header::{ACCEPT, CONTENT_TYPE, USER_AGENT},
};
use mlua::prelude::*;
use reqwest::{Client, Response};
use serde_json::{Map, Value};
use tokio::sync::Mutex;
use tracing::{Instrument as _, trace, trace_span};

/// HTTP module
pub struct LuaModHTTP {}

/// HTTP response
pub struct LuaModHTTPResponse {
    content_type: Box<str>,
    headers: HashMap<Box<str>, Vec<Box<str>>>,
    response: Arc<Mutex<Response>>,
    status_code: StatusCode,
}

impl LuaUserData for LuaModHTTPResponse {
    fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("content_type", |_, this| Ok(this.content_type.clone()));
        fields.add_field_method_get("headers", |_, this| Ok(this.headers.clone()));
        fields.add_field_method_get("ok", |_, this| Ok(this.status_code.is_success()));
        fields.add_field_method_get("status_code", |_, this| Ok(this.status_code.as_u16()));
    }

    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method("chunk", |vm, this, ()| async move {
            let Some(chunk) = this.response.lock().await.chunk().await.into_lua_err()? else {
                return Ok(None);
            };
            Ok(Some(LuaValue::String(vm.create_string(chunk)?)))
        });
        methods.add_async_method("json", |vm, this, ()| async move {
            let mut buf = BytesMut::new();
            while let Some(chunk) = this.response.lock().await.chunk().await.into_lua_err()? {
                buf.extend_from_slice(&chunk);
            }
            let value: Value = serde_json::from_slice(&buf).into_lua_err()?;
            Ok(vm.to_value(&value))
        });
        methods.add_async_method("text", |_, this, ()| async move {
            let mut buf = BytesMut::new();
            while let Some(chunk) = this.response.lock().await.chunk().await.into_lua_err()? {
                buf.extend_from_slice(&chunk);
            }
            String::from_utf8(buf.to_vec()).into_lua_err()
        });
    }
}

fn build_headers(headers: Option<&Map<String, Value>>) -> crate::Result<HeaderMap> {
    let headers = match headers {
        Some(h) => h,
        None => &Map::new(),
    };

    let mut m = HeaderMap::new();
    for (k, v) in headers.iter() {
        let v = match v {
            Value::String(s) => s.to_owned().into_boxed_str(),
            _ => v.to_string().into_boxed_str(),
        };
        m.insert(
            HeaderName::from_str(k).into_lua_err()?,
            HeaderValue::from_str(&v).into_lua_err()?,
        );
    }

    if !m.contains_key(USER_AGENT) {
        let value = format!("lmb/{}", env!("APP_VERSION"));
        #[allow(clippy::unwrap_used)]
        m.insert(USER_AGENT, value.parse().unwrap());
    }

    if !m.contains_key(ACCEPT) {
        #[allow(clippy::unwrap_used)]
        m.insert(ACCEPT, "*/*".parse().unwrap());
    }

    Ok(m)
}

async fn lua_lmb_fetch(uri: String, options: Option<LuaTable>) -> LuaResult<LuaModHTTPResponse> {
    let options = serde_json::to_value(options).into_lua_err()?;
    let uri: Uri = uri.parse().into_lua_err()?;
    let method: Method = options
        .pointer("/method")
        .and_then(|v| v.as_str())
        .unwrap_or("GET")
        .parse()
        .unwrap_or(Method::GET);
    let headers = options.pointer("/headers").and_then(|v| v.as_object());

    let client = Client::new();
    let headers = build_headers(headers).into_lua_err()?;
    let body = options
        .pointer("/body")
        .map(|v| match v {
            Value::String(s) => s.to_owned(),
            _ => v.to_string(),
        })
        .unwrap_or_default();
    let request = client
        .request(method, uri.to_string())
        .headers(headers)
        .body(body)
        .build()
        .into_lua_err()?;
    let span = {
        let method = request.method();
        let headers = request.headers();
        let uri = request.url();
        trace_span!("send HTTP request", %method, %uri, ?headers)
    };
    let response = match client.execute(request).instrument(span).await {
        Ok(res) => res,
        Err(e) => return Err(e.into_lua_err()),
    };
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .into();
    let headers = {
        let mut headers = HashMap::new();
        for name in response.headers().keys() {
            let values = response
                .headers()
                .get_all(name)
                .iter()
                .filter_map(|s| s.to_str().ok())
                .map(Into::into)
                .collect();
            headers.insert(name.to_string().into_boxed_str(), values);
        }
        headers
    };
    let status_code = response.status();
    trace!(%status_code, content_type, "response");
    let response = Arc::new(Mutex::new(response));
    Ok(LuaModHTTPResponse {
        content_type,
        headers,
        response,
        status_code,
    })
}

impl LuaUserData for LuaModHTTP {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method(
            "fetch",
            |_, _, (uri, options): (String, Option<LuaTable>)| async move {
                lua_lmb_fetch(uri, options).await
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use http::header::{ACCEPT, CONTENT_TYPE, USER_AGENT};
    use mockito::Server;
    use serde_json::json;
    use tokio::io::empty;

    use crate::Evaluation;

    #[tokio::test]
    async fn http_get() {
        let mut server = Server::new_async().await;

        let body = "<html>content</html>";
        let get_mock = server
            .mock("GET", "/html")
            .match_request(|r| r.has_header(ACCEPT) && r.has_header(USER_AGENT))
            .with_header(CONTENT_TYPE, "text/html")
            .with_body(body)
            .create_async()
            .await;

        let url = server.url();
        let script = format!(
            r#"
            local m = require('@lmb').http
            local res = m:fetch('{url}/html')
            return res:text()
            "#
        );
        let e = Evaluation::builder(script, empty()).build().unwrap();
        let res = e.evaluate_async().call().await.unwrap();
        assert_eq!(json!(body), res.payload);

        get_mock.assert_async().await;
    }

    #[tokio::test]
    async fn http_get_headers() {
        let mut server = Server::new_async().await;

        let body = "a";
        let get_mock = server
            .mock("GET", "/headers")
            .match_header("a", "b")
            .match_header(USER_AGENT, "agent/1.0")
            .with_header(CONTENT_TYPE, "text/plain")
            .with_body(body)
            .create_async()
            .await;

        let url = server.url();
        let script = format!(
            r#"
            local m = require('@lmb').http
            local res = m:fetch('{url}/headers', {{ headers = {{ a = 'b', ['user-agent'] = 'agent/1.0' }} }})
            return res:text()
            "#
        );
        let e = Evaluation::builder(script, empty()).build().unwrap();
        let res = e.evaluate_async().call().await.unwrap();
        assert_eq!(json!(body), res.payload);

        get_mock.assert_async().await;
    }

    #[tokio::test]
    async fn http_get_unicode() {
        let mut server = Server::new_async().await;

        let body = "<html>中文</html>";
        let get_mock = server
            .mock("GET", "/html")
            .with_header(CONTENT_TYPE, "text/html")
            .with_body(body)
            .create_async()
            .await;

        let url = server.url();
        let script = format!(
            r#"
            local m = require('@lmb').http
            local res = m:fetch('{url}/html')
            return res:text()
            "#
        );
        let e = Evaluation::builder(script, empty()).build().unwrap();
        let res = e.evaluate_async().call().await.unwrap();
        assert_eq!(json!(body), res.payload);

        get_mock.assert_async().await;
    }

    #[tokio::test]
    async fn http_get_json() {
        let mut server = Server::new_async().await;

        let body = r#"{"a":1}"#;
        let get_mock = server
            .mock("GET", "/json")
            .with_header(CONTENT_TYPE, "application/json")
            .with_body(body)
            .create_async()
            .await;

        let url = server.url();
        let script = format!(
            r#"
            local m = require('@lmb').http
            local res = m:fetch('{url}/json')
            return res:json()
            "#
        );
        let e = Evaluation::builder(script, empty()).build().unwrap();
        let res = e.evaluate_async().call().await.unwrap();
        assert_eq!(json!({ "a": 1 }), res.payload);

        get_mock.assert_async().await;
    }

    #[tokio::test]
    async fn http_post() {
        let mut server = Server::new_async().await;

        let post_mock = server
            .mock("POST", "/add")
            .match_body("1+1")
            .with_header(CONTENT_TYPE, "text/plain")
            .with_body("2")
            .create_async()
            .await;

        let url = server.url();
        let script = format!(
            r#"
            local m = require('@lmb').http
            local res = m:fetch('{url}/add', {{
              method = 'POST',
              body = '1+1',
            }})
            return res:text()
            "#
        );
        let e = Evaluation::builder(script, empty()).build().unwrap();
        let res = e.evaluate_async().call().await.unwrap();
        assert_eq!(json!("2"), res.payload);

        post_mock.assert_async().await;
    }
}
