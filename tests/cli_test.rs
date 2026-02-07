use reqwest::Method;
use snapbox::{
    cmd::{self, Command},
    str,
};

mod eval {
    use super::*;

    #[test]
    fn add() {
        Command::new(cmd::cargo_bin!("lmb"))
            .env("NO_COLOR", "true")
            .args([
                "eval",
                "--file",
                "src/fixtures/core/add.lua",
                "--state",
                "1",
            ])
            .assert()
            .success()
            .stdout_eq(str![[r#"
[..]  WARN lmb: No store path specified, using in-memory store
2

"#]])
            .stderr_eq(str![]);
    }

    #[test]
    fn env() {
        Command::new(cmd::cargo_bin!("lmb"))
            .env("FOO", "bar")
            .env("NO_COLOR", "true")
            .args([
                "--allow-env",
                "FOO",
                "eval",
                "--file",
                "src/fixtures/core/env.lua",
            ])
            .assert()
            .success()
            .stdout_eq(str![[r#"
[..]  WARN lmb: No store path specified, using in-memory store
null
FOO = bar

"#]])
            .stderr_eq(str![]);
    }

    #[test]
    fn hello() {
        Command::new(cmd::cargo_bin!("lmb"))
            .env("NO_COLOR", "true")
            .args(["eval", "--file", "src/fixtures/core/hello.lua"])
            .assert()
            .success()
            .stdout_eq(str![[r#"
[..]  WARN lmb: No store path specified, using in-memory store
true
Hello, world!

"#]])
            .stderr_eq(str![]);
    }

    #[tokio::test]
    async fn http_get() {
        let mut server = mockito::Server::new_async().await;

        let url = server.url();

        let mock = server
            .mock(Method::GET.as_str(), "/")
            .with_status(200)
            .with_body("Hello, world!")
            .create_async()
            .await;

        Command::new(cmd::cargo_bin!("lmb"))
            .env("NO_COLOR", "true")
            .args([
                "--allow-all-net",
                "--http-timeout",
                "1s",
                "eval",
                "--file",
                "src/fixtures/bindings/http-get.lua",
                "--state",
                &url,
            ])
            .assert()
            .success()
            .stdout_eq(str![[r#"
[..]  WARN lmb: No store path specified, using in-memory store
Hello, world!

"#]])
            .stderr_eq(str![]);

        mock.assert_async().await;
    }

    #[test]
    fn infinite_timeout() {
        Command::new(cmd::cargo_bin!("lmb"))
            .env("NO_COLOR", "true")
            .args([
                "--timeout",
                "100ms",
                "eval",
                "--file",
                "src/fixtures/core/infinite.lua",
            ])
            .assert()
            .failure()
            .stdout_eq(str![[r#"
[..]  WARN lmb: No store path specified, using in-memory store

"#]])
            .stderr_eq(str![[r#"
Timeout: Lua script execution timed out after [..]ms, timeout was 100ms
Error: Timeout: Lua script execution timed out after [..]ms, timeout was 100ms

"#]]);
    }

    #[test]
    fn no_export() {
        Command::new(cmd::cargo_bin!("lmb"))
            .env("NO_COLOR", "true")
            .args(["eval", "--file", "src/fixtures/core/no-export.lua"])
            .assert()
            .success()
            .stdout_eq(str![[r#"
[..]  WARN lmb: No store path specified, using in-memory store
null

"#]])
            .stderr_eq(str![]);
    }

    #[test]
    fn stdin_expression() {
        Command::new(cmd::cargo_bin!("lmb"))
            .env("NO_COLOR", "true")
            .stdin("return 'Hello, world!'")
            .args(["eval", "--file", "-"])
            .assert()
            .success()
            .stdout_eq(str![[r#"
[..]  WARN lmb: No store path specified, using in-memory store
Hello, world!

"#]])
            .stderr_eq(str![]);
    }

    #[test]
    fn stdin_error() {
        Command::new(cmd::cargo_bin!("lmb"))
            .env("NO_COLOR", "true")
            .stdin(include_str!("../src/fixtures/errors/error.lua"))
            .args(["eval", "--file", "-"])
            .assert()
            .failure()
            .stdout_eq(str![[r#"
[..]  WARN lmb: No store path specified, using in-memory store

"#]])
            .stderr_eq(str![[r#"
  x unknown error
   ,-[(stdin):2:1]
 1 | function f()
 2 |   error("unknown error")
   : ^^^^^^^^^^^^|^^^^^^^^^^^^
   :             `-- unknown error
 3 |   return true
 4 | end
   `----

"#]]);
    }
}

mod errors {
    use super::*;

    #[test]
    fn callback_error() {
        Command::new(cmd::cargo_bin!("lmb"))
            .env("NO_COLOR", "true")
            .args(["eval", "--file", "src/fixtures/errors/callback-error.lua"])
            .assert()
            .failure()
            .stdout_eq(str![[r#"
[..]  WARN lmb: No store path specified, using in-memory store

"#]])
            .stderr_eq(str![[r#"
  x EOF while parsing an object at line 1 column 1
   ,-[@src/fixtures/errors/callback-error.lua:3:1]
 2 |   local json = require("@lmb/json")
 3 |   return json.decode("{")
   : ^^^^^^^^^^^^^|^^^^^^^^^^^^
   :              `-- EOF while parsing an object at line 1 column 1
 4 | end
 5 | 
   `----

"#]]);
    }

    #[test]
    fn callback_expr_error() {
        Command::new(cmd::cargo_bin!("lmb"))
            .env("NO_COLOR", "true")
            .args([
                "eval",
                "--file",
                "src/fixtures/errors/callback-expr-error.lua",
            ])
            .assert()
            .failure()
            .stdout_eq(str![[r#"
[..]  WARN lmb: No store path specified, using in-memory store

"#]])
            .stderr_eq(str![[r#"
  x EOF while parsing an object at line 1 column 1
   ,-[@src/fixtures/errors/callback-expr-error.lua:2:1]
 1 | local json = require("@lmb/json")
 2 | return json.decode("{")
   : ^^^^^^^^^^^^|^^^^^^^^^^^
   :             `-- EOF while parsing an object at line 1 column 1
   `----

"#]]);
    }

    #[test]
    fn error() {
        Command::new(cmd::cargo_bin!("lmb"))
            .env("NO_COLOR", "true")
            .args(["eval", "--file", "src/fixtures/errors/error.lua"])
            .assert()
            .failure()
            .stdout_eq(str![[r#"
[..]  WARN lmb: No store path specified, using in-memory store

"#]])
            .stderr_eq(str![[r#"
  x unknown error
   ,-[@src/fixtures/errors/error.lua:2:1]
 1 | function f()
 2 |   error("unknown error")
   : ^^^^^^^^^^^^|^^^^^^^^^^^^
   :             `-- unknown error
 3 |   return true
 4 | end
   `----

"#]]);
    }

    #[test]
    fn error_expr() {
        Command::new(cmd::cargo_bin!("lmb"))
            .env("NO_COLOR", "true")
            .args(["eval", "--file", "src/fixtures/errors/error-expr.lua"])
            .assert()
            .failure()
            .stdout_eq(str![[r#"
[..]  WARN lmb: No store path specified, using in-memory store

"#]])
            .stderr_eq(str![[r#"
  x unknown error
   ,-[@src/fixtures/errors/error-expr.lua:1:1]
 1 | error("unknown error")
   : ^^^^^^^^^^^|^^^^^^^^^^^
   :            `-- unknown error
 2 | return true
   `----

"#]]);
    }

    #[test]
    fn error_value() {
        Command::new(cmd::cargo_bin!("lmb"))
            .env("NO_COLOR", "true")
            .args(["eval", "--file", "src/fixtures/errors/error-value.lua"])
            .assert()
            .failure()
            .stdout_eq(str![[r#"
[..]  WARN lmb: No store path specified, using in-memory store

"#]])
            .stderr_eq(str![[r#"
Lua value as error: {"a":1}
Error: Lua value as error: {"a":1}

"#]]);
    }

    #[test]
    fn error_value_expr() {
        Command::new(cmd::cargo_bin!("lmb"))
            .env("NO_COLOR", "true")
            .args(["eval", "--file", "src/fixtures/errors/error-value-expr.lua"])
            .assert()
            .failure()
            .stdout_eq(str![[r#"
[..]  WARN lmb: No store path specified, using in-memory store

"#]])
            .stderr_eq(str![[r#"
Lua value as error: {"a":1}
Error: Lua value as error: {"a":1}

"#]]);
    }

    #[test]
    fn syntax_error() {
        Command::new(cmd::cargo_bin!("lmb"))
            .env("NO_COLOR", "true")
            .args(["eval", "--file", "src/fixtures/errors/syntax-error.lua"])
            .assert()
            .failure()
            .stdout_eq(str![[r#"
[..]  WARN lmb: No store path specified, using in-memory store

"#]])
            .stderr_eq(str![[r#"
  x Incomplete statement: expected assignment or a function call
   ,-[@src/fixtures/errors/syntax-error.lua:2:1]
 1 | function syntax_error()
 2 |     ret true
   : ^^^^^^|^^^^^^
   :       `-- Incomplete statement: expected assignment or a function call
 3 | end
 4 | 
   `----

"#]]);
    }
}
