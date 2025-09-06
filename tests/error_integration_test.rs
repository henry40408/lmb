use snapbox::{
    cmd::{Command, cargo_bin},
    str,
};

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
Error: Lua error: runtime error: src/fixtures/errors/error.lua:2: unknown error

"#]]);
}

#[test]
fn eval_error_expr() {
    Command::new(cargo_bin("lmb"))
        .env("NO_COLOR", "true")
        .args(["eval", "--file", "src/fixtures/errors/error-expr.lua"])
        .assert()
        .failure()
        .stdout_eq(str![])
        .stderr_eq(str![[r#"
  x unknown error
   ,-[@src/fixtures/errors/error-expr.lua:1:1]
 1 | error("unknown error")
   : ^^^^^^^^^^^|^^^^^^^^^^^
   :            `-- unknown error
 2 | return true
   `----
Error: Lua error: runtime error: src/fixtures/errors/error-expr.lua:1: unknown error

"#]]);
}

#[test]
fn eval_error_value() {
    Command::new(cargo_bin("lmb"))
        .env("NO_COLOR", "true")
        .args(["eval", "--file", "src/fixtures/errors/error-value.lua"])
        .assert()
        .failure()
        .stdout_eq(str![])
        .stderr_eq(str![[r#"
Lua value as error: {"a":1}
Error: Lua value as error: {"a":1}

"#]]);
}

#[test]
fn eval_error_value_expr() {
    Command::new(cargo_bin("lmb"))
        .env("NO_COLOR", "true")
        .args(["eval", "--file", "src/fixtures/errors/error-value-expr.lua"])
        .assert()
        .failure()
        .stdout_eq(str![])
        .stderr_eq(str![[r#"
Lua value as error: {"a":1}
Error: Lua value as error: {"a":1}

"#]]);
}
