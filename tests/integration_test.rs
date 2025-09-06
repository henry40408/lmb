use reqwest::Method;
use snapbox::{
    cmd::{Command, cargo_bin},
    str,
};

#[test]
fn eval_add() {
    Command::new(cargo_bin("lmb"))
        .args(["eval", "--file", "src/fixtures/add.lua", "--state", "1"])
        .assert()
        .success()
        .stdout_eq(str![[r#"
2

"#]])
        .stderr_eq(str![]);
}

#[test]
fn eval_env() {
    Command::new(cargo_bin("lmb"))
        .env("FOO", "bar")
        .env("NO_COLOR", "true")
        .args([
            "--allow-env",
            "FOO",
            "eval",
            "--file",
            "src/fixtures/env.lua",
        ])
        .assert()
        .success()
        .stdout_eq(str![[r#"
null
FOO = bar

"#]])
        .stderr_eq(str![]);
}

#[test]
fn eval_error() {
    Command::new(cargo_bin("lmb"))
        .env("NO_COLOR", "true")
        .args(["eval", "--file", "src/fixtures/errors/error.lua"])
        .assert()
        .failure()
        .stdout_eq(str![])
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
fn eval_hello() {
    Command::new(cargo_bin("lmb"))
        .args(["eval", "--file", "src/fixtures/hello.lua"])
        .assert()
        .success()
        .stdout_eq(str![[r#"
true
Hello, world!

"#]])
        .stderr_eq(str![]);
}

#[tokio::test]
async fn eval_http_get() {
    let mut server = mockito::Server::new_async().await;

    let url = server.url();

    let mock = server
        .mock(Method::GET.as_str(), "/")
        .with_status(200)
        .with_body("Hello, world!")
        .create_async()
        .await;

    Command::new(cargo_bin("lmb"))
        .args([
            "--allow-all-net",
            "--http-timeout",
            "1s",
            "eval",
            "--file",
            "src/fixtures/http-get.lua",
            "--state",
            &url,
        ])
        .assert()
        .success()
        .stdout_eq(str![[r#"
null

"#]])
        .stderr_eq(str![]);

    mock.assert_async().await;
}

#[test]
fn eval_infinite() {
    Command::new(cargo_bin("lmb"))
        .env("NO_COLOR", "true")
        .args([
            "--timeout",
            "100ms",
            "eval",
            "--file",
            "src/fixtures/infinite.lua",
        ])
        .assert()
        .failure()
        .stdout_eq(str![])
        .stderr_eq(str![[r#"
Timeout: Lua script execution timed out after [..]ms, timeout was 100ms
Error: Timeout: Lua script execution timed out after [..]ms, timeout was 100ms

"#]]);
}

#[test]
fn eval_no_export() {
    Command::new(cargo_bin("lmb"))
        .env("NO_COLOR", "true")
        .args(["eval", "--file", "src/fixtures/no-export.lua"])
        .assert()
        .success()
        .stdout_eq(str![[r#"
null

"#]])
        .stderr_eq(str![]);
}

#[test]
fn eval_stdin_expression() {
    Command::new(cargo_bin("lmb"))
        .env("NO_COLOR", "true")
        .stdin("return 'Hello, world!'")
        .args(["eval", "--file", "-"])
        .assert()
        .success()
        .stdout_eq(str![[r#"
Hello, world!

"#]])
        .stderr_eq(str![]);
}

#[test]
fn eval_stdin_error() {
    Command::new(cargo_bin("lmb"))
        .env("NO_COLOR", "true")
        .stdin(include_str!("../src/fixtures/errors/error.lua"))
        .args(["eval", "--file", "-"])
        .assert()
        .failure()
        .stdout_eq(str![])
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
