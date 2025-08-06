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
fn eval_error() {
    Command::new(cargo_bin("lmb"))
        .env("NO_COLOR", "1")
        .args(["eval", "--file", "src/fixtures/error.lua"])
        .assert()
        .failure()
        .stdout_eq(str![])
        .stderr_eq(str![[r#"
  x An error occurred
   ,-[@src/fixtures/error.lua:3:1]
 2 |   local a = 1
 3 |   error("An error occurred")
   : ^^^^^^^^^^^^^^|^^^^^^^^^^^^^^
   :               `-- An error occurred
 4 |   a = a + 1
 5 |   return a
   `----
Error: Lua error: runtime error: src/fixtures/error.lua:3: An error occurred
stack traceback:
	[C]: in function 'error'
	src/fixtures/error.lua:3: in function 'f'

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
        .env("NO_COLOR", "1")
        .args([
            "eval",
            "--file",
            "src/fixtures/infinite.lua",
            "--timeout",
            "100ms",
        ])
        .assert()
        .failure()
        .stdout_eq(str![])
        .stderr_eq(str![[r#"
Timeout: Lua script execution timed out after 100[..]ms, timeout was 100ms
Error: Timeout: Lua script execution timed out after 100[..]ms, timeout was 100ms

"#]]);
}

#[test]
fn eval_no_export() {
    Command::new(cargo_bin("lmb"))
        .env("NO_COLOR", "1")
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
        .env("NO_COLOR", "1")
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
        .env("NO_COLOR", "1")
        .stdin(include_str!("../src/fixtures/error.lua"))
        .args(["eval", "--file", "-"])
        .assert()
        .failure()
        .stdout_eq(str![])
        .stderr_eq(str![[r#"
  x An error occurred
   ,-[-:3:1]
 2 |   local a = 1
 3 |   error("An error occurred")
   : ^^^^^^^^^^^^^^|^^^^^^^^^^^^^^
   :               `-- An error occurred
 4 |   a = a + 1
 5 |   return a
   `----
Error: Lua error: runtime error: [..]: An error occurred
stack traceback:
	[C]: in function 'error'
	[..]: in function 'f'

"#]]);
}
