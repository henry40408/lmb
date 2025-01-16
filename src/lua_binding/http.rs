use std::{
    collections::HashMap,
    io::{BufReader, Cursor, Read},
    sync::Arc,
};

use http::{Method, StatusCode};
use mlua::prelude::*;
use parking_lot::Mutex;
use serde_json::{Map, Value};
use tracing::{trace, trace_span, warn};
use ureq::Request;
use url::Url;

use super::{lua_lmb_read, lua_lmb_read_unicode};
use crate::Input;

/// HTTP module
pub struct LuaModHTTP {}

/// HTTP response
pub struct LuaModHTTPResponse {
    charset: Box<str>,
    content_type: Box<str>,
    headers: HashMap<Box<str>, Vec<Box<str>>>,
    reader: Input<Box<dyn Read + Send + Sync + 'static>>,
    status_code: StatusCode,
}

impl LuaUserData for LuaModHTTPResponse {
    fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("charset", |_, this| Ok(this.charset.clone()));
        fields.add_field_method_get("content_type", |_, this| Ok(this.content_type.clone()));
        fields.add_field_method_get("headers", |_, this| Ok(this.headers.clone()));
        fields.add_field_method_get("ok", |_, this| Ok(this.status_code.is_success()));
        fields.add_field_method_get("status_code", |_, this| Ok(this.status_code.as_u16()));
    }

    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("json", |vm, this, ()| {
            if "application/json" != &*this.content_type {
                warn!("content type is not application/json, convert with caution");
            }
            let value: Value = serde_json::from_reader(&mut *this.reader.lock()).into_lua_err()?;
            vm.to_value(&value)
        });
        methods.add_method("read", |vm, this, f: Option<LuaValue>| {
            lua_lmb_read(vm, &this.reader, f)
        });
        methods.add_method("read_unicode", |vm, this, f: LuaValue| {
            lua_lmb_read_unicode(vm, &this.reader, f)
        });
    }
}

fn set_headers(req: Request, headers: Option<&Map<String, Value>>) -> Request {
    let Some(headers) = headers else {
        return req;
    };
    let mut new_req = req;
    for (k, v) in headers.iter() {
        let v = match v {
            Value::String(v) => v.to_owned().into_boxed_str(),
            _ => v.to_string().into_boxed_str(),
        };
        new_req = new_req.set(k, &v);
    }
    new_req
}

fn lua_lmb_fetch(
    _vm: &Lua,
    _: &LuaModHTTP,
    (uri, options): (String, Option<LuaTable>),
) -> LuaResult<LuaModHTTPResponse> {
    let options = serde_json::to_value(options).into_lua_err()?;
    let url: Url = uri.parse().into_lua_err()?;
    let method: Method = options
        .pointer("/method")
        .and_then(|v| v.as_str())
        .unwrap_or("GET")
        .parse()
        .unwrap_or(Method::GET);
    let headers = options.pointer("/headers").and_then(|v| v.as_object());
    let res = if method.is_safe() {
        let req = ureq::request_url(method.as_str(), &url);
        let req = set_headers(req, headers);
        let _s = trace_span!("send_http_request", %method, %url, ?headers).entered();
        req.call()
    } else {
        let body = options
            .pointer("/body")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let req = ureq::request_url(method.as_str(), &url);
        let req = set_headers(req, headers);
        let _s = trace_span!("send_http_request", %method, %url, ?headers).entered();
        req.send(Cursor::new(body))
    };
    let res = match res {
        Ok(res) | Err(ureq::Error::Status(_, res)) => res,
        Err(e) => return Err(e.into_lua_err()),
    };
    let charset = res.charset().to_owned().into_boxed_str();
    let content_type = res.content_type().to_owned().into_boxed_str();
    let headers = {
        let mut headers = HashMap::new();
        for name in res.headers_names() {
            let values = res
                .all(&name)
                .into_iter()
                .map(|s| s.to_owned().into_boxed_str())
                .collect::<Vec<_>>();
            headers.insert(name.into_boxed_str(), values);
        }
        headers
    };
    let status_code = StatusCode::from_u16(res.status()).into_lua_err()?;
    trace!(%status_code, charset, content_type, "response");
    let reader = Arc::new(Mutex::new(BufReader::new(res.into_reader())));
    Ok(LuaModHTTPResponse {
        charset,
        content_type,
        headers,
        reader,
        status_code,
    })
}

impl LuaUserData for LuaModHTTP {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("fetch", lua_lmb_fetch);
    }
}

#[cfg(test)]
mod tests {
    use std::io::empty;

    use mockito::Server;
    use serde_json::json;

    use crate::Evaluation;

    #[test]
    fn http_get() {
        let mut server = Server::new();

        let body = "<html>content</html>";
        let get_mock = server
            .mock("GET", "/html")
            .with_header("content-type", "text/html")
            .with_body(body)
            .create();

        let url = server.url();
        let script = format!(
            r#"
            local m = require('@lmb/http')
            local res = m:fetch('{url}/html')
            return res:read('*a')
            "#
        );
        let e = Evaluation::builder(script, empty()).build().unwrap();
        let res = e.evaluate().call().unwrap();
        assert_eq!(json!(body), res.payload);

        get_mock.assert();
    }

    #[test]
    fn http_get_headers() {
        let mut server = Server::new();

        let body = "a";
        let get_mock = server
            .mock("GET", "/headers")
            .match_header("a", "b")
            .with_header("content-type", "text/plain")
            .with_body(body)
            .create();

        let url = server.url();
        let script = format!(
            r#"
            local m = require('@lmb/http')
            local res = m:fetch('{url}/headers', {{ headers = {{ a = 'b' }} }})
            return res:read('*a')
            "#
        );
        let e = Evaluation::builder(script, empty()).build().unwrap();
        let res = e.evaluate().call().unwrap();
        assert_eq!(json!(body), res.payload);

        get_mock.assert();
    }

    #[test]
    fn http_get_unicode() {
        let mut server = Server::new();

        let body = "<html>中文</html>";
        let get_mock = server
            .mock("GET", "/html")
            .with_header("content-type", "text/html")
            .with_body(body)
            .create();

        let url = server.url();
        let script = format!(
            r#"
            local m = require('@lmb/http')
            local res = m:fetch('{url}/html')
            return res:read_unicode('*a')
            "#
        );
        let e = Evaluation::builder(script, empty()).build().unwrap();
        let res = e.evaluate().call().unwrap();
        assert_eq!(json!(body), res.payload);

        get_mock.assert();
    }

    #[test]
    fn http_get_json() {
        let mut server = Server::new();

        let body = r#"{"a":1}"#;
        let get_mock = server
            .mock("GET", "/json")
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let url = server.url();
        let script = format!(
            r#"
            local m = require('@lmb/http')
            local res = m:fetch('{url}/json')
            return res:json()
            "#
        );
        let e = Evaluation::builder(script, empty()).build().unwrap();
        let res = e.evaluate().call().unwrap();
        assert_eq!(json!({ "a": 1 }), res.payload);

        get_mock.assert();
    }

    #[test]
    fn http_post() {
        let mut server = Server::new();

        let post_mock = server
            .mock("POST", "/add")
            .match_body("1+1")
            .with_header("content-type", "text/plain")
            .with_body("2")
            .create();

        let url = server.url();
        let script = format!(
            r#"
            local m = require('@lmb/http')
            local res = m:fetch('{url}/add', {{
              method = 'POST',
              body = '1+1',
            }})
            return res:read('*a')
            "#
        );
        let e = Evaluation::builder(script, empty()).build().unwrap();
        let res = e.evaluate().call().unwrap();
        assert_eq!(json!("2"), res.payload);

        post_mock.assert();
    }
}
