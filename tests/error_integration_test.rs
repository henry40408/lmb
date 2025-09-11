use snapbox::{
    cmd::{Command, cargo_bin},
    str,
};

#[test]
fn eval_callback_error() {
    Command::new(cargo_bin("lmb"))
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
fn eval_callback_expr_error() {
    Command::new(cargo_bin("lmb"))
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
fn eval_error() {
    Command::new(cargo_bin("lmb"))
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
fn eval_error_expr() {
    Command::new(cargo_bin("lmb"))
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
fn eval_error_value() {
    Command::new(cargo_bin("lmb"))
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
fn eval_error_value_expr() {
    Command::new(cargo_bin("lmb"))
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
fn eval_syntax_error() {
    Command::new(cargo_bin("lmb"))
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
