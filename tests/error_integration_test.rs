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
