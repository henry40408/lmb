use assert_fs::NamedTempFile;
use snapbox::{
    cmd::{Command, cargo_bin},
    str,
};
use std::time::Duration;

#[test]
fn check_stdin_syntax_error() {
    Command::new(cargo_bin("lmb"))
        .stdin("ret true")
        .args(["--no-color", "check", "--file", "-"])
        .assert()
        .failure()
        .stdout_eq(str![])
        .stderr_eq(str![[r#"
× unexpected expression when looking for a statement
   ╭────
 1 │ ret true
   ·     ──┬─
   ·       ╰── unexpected expression when looking for a statement
   ╰────
  × unexpected token, this needs to be a statement
   ╭────
 1 │ ret true
   ·     ──┬─
   ·       ╰── unexpected token, this needs to be a statement
   ╰────

"#]]);
}

#[test]
fn check_stdin_tokenizer_error() {
    Command::new(cargo_bin("lmb"))
        .stdin("return !true")
        .args(["--no-color", "check", "--file", "-"])
        .assert()
        .failure()
        .stdout_eq(str![])
        .stderr_eq(str![[r#"
× unexpected character !
   ╭────
 1 │ return !true
   ·        ┬
   ·        ╰── unexpected character !
   ╰────
  × unexpected token, this needs to be a statement
   ╭────
 1 │ return !true
   ·         ──┬─
   ·           ╰── unexpected token, this needs to be a statement
   ╰────

"#]]);
}

#[test]
fn eval_file() {
    Command::new(cargo_bin("lmb"))
        .args(["--no-color", "eval", "--file", "lua-examples/hello.lua"])
        .assert()
        .success()
        .stdout_eq(str![[r#"
[..]  INFO rusqlite_migration: Database migrated to version 1    
nullhello, world!

"#]])
        .stderr_eq(str![]);
}

#[test]
fn eval_json_output() {
    Command::new(cargo_bin("lmb"))
        .args([
            "--no-color",
            "--json",
            "example",
            "eval",
            "--name",
            "return-table",
        ])
        .assert()
        .success()
        .stdout_eq(str![[r#"
[..]  INFO rusqlite_migration: Database migrated to version 1    
{"bool":true,"num":1.23,"str":"hello"}
"#]])
        .stderr_eq(str![]);
}

#[test]
fn eval_stdin() {
    Command::new(cargo_bin("lmb"))
        .stdin("return 1+1")
        .args(["--no-color", "eval", "--file", "-"])
        .assert()
        .success()
        .stdout_eq(str![[r#"
[..]  INFO rusqlite_migration: Database migrated to version 1    
2
"#]])
        .stderr_eq(str![]);
}

#[test]
fn eval_stdin_runtime_error() {
    Command::new(cargo_bin("lmb"))
        .stdin("print(1)\nprint(nil+1)\nprint(2)")
        .args(["--no-color", "eval", "--file", "-"])
        .assert()
        .failure()
        .stderr_eq(str![[r#"
  ×  attempt to perform arithmetic (add) on nil and number
   ╭─[2:1]
 1 │ print(1)
 2 │ print(nil+1)
   · ──────┬──────
   ·       ╰──  attempt to perform arithmetic (add) on nil and number
 3 │ print(2)
   ╰────

"#]])
        .stderr_eq(str![[r#"
  ×  attempt to perform arithmetic (add) on nil and number
   ╭─[2:1]
 1 │ print(1)
 2 │ print(nil+1)
   · ──────┬──────
   ·       ╰──  attempt to perform arithmetic (add) on nil and number
 3 │ print(2)
   ╰────

"#]]);
}

#[test]
fn eval_stdin_syntax_error() {
    Command::new(cargo_bin("lmb"))
        .stdin("return !true")
        .args(["--no-color", "eval", "--file", "-"])
        .assert()
        .failure()
        .stdout_eq(str![[r#"
[..]  INFO rusqlite_migration: Database migrated to version 1    

"#]])
        .stderr_eq(str![
            "lua error: syntax error: 1: Unexpected '!'; did you mean 'not'?"
        ]);
}

#[test]
fn eval_store_migrate() {
    let store = NamedTempFile::new("db.sqlite3").unwrap();
    let store_path = store.path().to_string_lossy();
    Command::new(cargo_bin("lmb"))
        .stdin("return true")
        .args([
            "--no-color",
            "--store-path",
            &store_path,
            "--run-migrations",
            "eval",
            "--file",
            "-",
        ])
        .assert()
        .success()
        .stdout_eq(str![[r#"
[..]  INFO rusqlite_migration: Database migrated to version 1    
true
"#]])
        .stderr_eq(str![]);
}

#[test]
fn example_cat() {
    Command::new(cargo_bin("lmb"))
        .args(["--no-color", "example", "cat", "--name", "hello"])
        .assert()
        .success()
        .stdout_eq(str![[r#"
--[[
--description = "Hello, world!"
--]]
print("hello, world!")


"#]])
        .stderr_eq(str![]);
}

#[test]
fn example_cat_absent() {
    Command::new(cargo_bin("lmb"))
        .args(["--no-color", "example", "cat", "--name", "__absent__"])
        .assert()
        .failure()
        .stderr_eq(str![[r#"
example with __absent__ not found

"#]])
        .stderr_eq(str![[r#"
example with __absent__ not found

"#]]);
}

#[test]
fn example_eval() {
    Command::new(cargo_bin("lmb"))
        .stdin("1949\n")
        .args(["--no-color", "example", "eval", "--name", "algebra"])
        .assert()
        .success()
        .stdout_eq(str![[r#"
[..]  INFO rusqlite_migration: Database migrated to version 1    
3798601
"#]])
        .stderr_eq(str![]);
}

#[test]
fn example_list() {
    Command::new(cargo_bin("lmb"))
        .args(["example", "list"])
        .assert()
        .success()
        .stdout_eq(str![[r#"
 name          description                                                            
 algebra       Return the square of number.                                           
 count-bytes   Count bytes from standard input.                                       
 crypto        Hash data with HMAC-SHA256.                                            
 echo          Echo the input.                                                        
 env           Print environment variable.                                            
 error         Demonstrate how the runner reacts when an error is thrown.             
 hello         Hello, world!                                                          
 http-client   Send HTTP GET request.                                                 
 http-echo     Echo headers and body from HTTP request.                               
 input         Echo the standard input.                                               
 join-all      Join multiple coroutines and wait for all to finish.                   
 last          The LAST Lua script to handle HTTP request.                            
 mw1           The FIRST Lua script to read request body.                             
 mw2           The SECOND Lua script to log request.                                  
 read-unicode  Read 2 unicode characters from the standard input.                     
 return-table  The function can also return a table.                                  
               Please note that JSON mode is needed to show the whole table,          
               otherwise "table: 0x..." will be printed, which aligns how Lua works.  
 store         Update an absent key 'a' in store and return the new value.            
               Please note that since store is epheremal the output will always be 1. 
                                                                                      

"#]])
        .stderr_eq(str![]);
}

#[test]
fn example_serve() {
    Command::new(cargo_bin("lmb"))
        .timeout(Duration::from_secs(2))
        .args([
            "--no-color",
            "example",
            "serve",
            "--bind",
            "127.0.0.1:0",
            "--name",
            "hello",
        ])
        .assert()
        .stdout_eq(str![[r#"
[..]  INFO rusqlite_migration: Database migrated to version 1    
[..]  WARN lmb::serve: no store path is specified, an in-memory store will be used and values will be lost when process ends
[..]  INFO lmb::serve: serving lua script bind=127.0.0.1:[..]

"#]]).stderr_eq(str![]);
}

#[test]
fn guide_cat() {
    Command::new(cargo_bin("lmb"))
        .args(["guide", "cat", "--name", "lua"])
        .assert()
        .success()
        .stderr_eq(str![]);
}

#[test]
fn guide_cat_absent() {
    Command::new(cargo_bin("lmb"))
        .args(["guide", "cat", "--name", "__absent__"])
        .assert()
        .failure()
        .stderr_eq(str![[r#"
guide with __absent__ not found

"#]])
        .stderr_eq(str![[r#"
guide with __absent__ not found

"#]]);
}

#[test]
fn guide_list() {
    Command::new(cargo_bin("lmb"))
        .args(["guide", "list"])
        .assert()
        .success()
        .stderr_eq(str![]);
}

#[test]
fn list_themes() {
    Command::new(cargo_bin("lmb"))
        .args(["list-themes"])
        .assert()
        .success()
        .stdout_eq(str![[r#"
1337
Coldark-Cold
Coldark-Dark
DarkNeon
Dracula
GitHub
Monokai Extended
Monokai Extended Bright
Monokai Extended Light
Monokai Extended Origin
Nord
OneHalfDark
OneHalfLight
Solarized (dark)
Solarized (light)
Sublime Snazzy
TwoDark
Visual Studio Dark+
ansi
base16
base16-256
gruvbox-dark
gruvbox-light
zenburn

"#]])
        .stderr_eq(str![]);
}

#[test]
fn schedule() {
    let store = NamedTempFile::new("db.sqlite3").unwrap();
    let store_path = store.path().to_string_lossy();

    Command::new(cargo_bin("lmb"))
        .stdin("require('@lmb').store.a = 1; return true")
        .args([
            "--no-color",
            "--store-path",
            &store_path,
            "--run-migrations",
            "schedule",
            "--cron",
            "* * * * * *",
            "--initial-run",
            "--file",
            "-",
        ])
        .timeout(Duration::from_secs(2))
        .assert()
        .stderr_eq(str![]);

    Command::new(cargo_bin("lmb"))
        .args([
            "--store-path",
            &store_path,
            "--run-migrations",
            "store",
            "get",
            "--name",
            "a",
        ])
        .assert()
        .stdout_eq(str!["1"]);
}

#[test]
fn serve() {
    Command::new(cargo_bin("lmb"))
        .args([
            "--no-color",
            "serve",
            "--bind",
            "127.0.0.1:3001",
            "--file",
            "lua-examples/hello.lua",
        ])
        .timeout(Duration::from_secs(2))
        .assert()
        .stdout_eq(str![[r#"
[..]  INFO rusqlite_migration: Database migrated to version 1    
[..]  WARN lmb::serve: no store path is specified, an in-memory store will be used and values will be lost when process ends
[..]  INFO lmb::serve: serving lua script bind=127.0.0.1:3001

"#]]);
}

#[test]
fn store_delete() {
    let store = NamedTempFile::new("db.sqlite3").unwrap();
    let store_path = store.path().to_string_lossy();

    Command::new(cargo_bin("lmb"))
        .stdin("1")
        .args([
            "--no-color",
            "--store-path",
            &store_path,
            "--run-migrations",
            "store",
            "put",
            "--name",
            "a",
            "--value",
            "-",
        ])
        .assert()
        .success()
        .stdout_eq(str![[r#"
[..]  INFO rusqlite_migration: Database migrated to version 1    
1
"#]])
        .stderr_eq(str![]);

    Command::new(cargo_bin("lmb"))
        .args([
            "--store-path",
            &store_path,
            "--run-migrations",
            "store",
            "delete",
            "--name",
            "a",
        ])
        .assert()
        .success()
        .stdout_eq(str!["1"])
        .stderr_eq(str![]);
}

#[test]
fn store_get() {
    let store = NamedTempFile::new("db.sqlite3").unwrap();
    let store_path = store.path().to_string_lossy();
    Command::new(cargo_bin("lmb"))
        .args([
            "--no-color",
            "--store-path",
            &store_path,
            "--run-migrations",
            "store",
            "get",
            "--name",
            "a",
        ])
        .assert()
        .success()
        .stdout_eq(str![[r#"
[..]  INFO rusqlite_migration: Database migrated to version 1    
null
"#]])
        .stderr_eq(str![]);
}

#[test]
fn store_get_list_put() {
    let store = NamedTempFile::new("db.sqlite3").unwrap();
    let store_path = store.path().to_string_lossy();

    Command::new(cargo_bin("lmb"))
        .stdin("1")
        .args([
            "--no-color",
            "--store-path",
            &store_path,
            "--run-migrations",
            "store",
            "put",
            "--name",
            "a",
            "--value",
            "-",
        ])
        .assert()
        .success()
        .stdout_eq(str![[r#"
[..]  INFO rusqlite_migration: Database migrated to version 1    
1
"#]])
        .stderr_eq(str![]);

    Command::new(cargo_bin("lmb"))
        .args([
            "--no-color",
            "--store-path",
            &store_path,
            "--run-migrations",
            "store",
            "list",
        ])
        .assert()
        .success()
        .stdout_eq(str![[r#"
 name  type    size  created at                 updated at                
 a     number  8     [..]

"#]])
        .stderr_eq(str![]);

    Command::new(cargo_bin("lmb"))
        .args([
            "--no-color",
            "--store-path",
            &store_path,
            "--run-migrations",
            "store",
            "get",
            "--name",
            "a",
        ])
        .assert()
        .success()
        .stdout_eq(str!["1"])
        .stderr_eq(str![]);
}

#[test]
fn store_list() {
    let store = NamedTempFile::new("db.sqlite3").unwrap();
    let store_path = store.path().to_string_lossy();
    Command::new(cargo_bin("lmb"))
        .args([
            "--no-color",
            "--store-path",
            &store_path,
            "--run-migrations",
            "store",
            "list",
        ])
        .assert()
        .success()
        .stdout_eq(str![[r#"
[..]  INFO rusqlite_migration: Database migrated to version 1    
 name  type  size  created at  updated at 

"#]])
        .stderr_eq(str![]);
}

#[test]
fn store_migrate() {
    let store = NamedTempFile::new("db.sqlite3").unwrap();
    let store_path = store.path().to_string_lossy();
    Command::new(cargo_bin("lmb"))
        .args([
            "--no-color",
            "--store-path",
            &store_path,
            "store",
            "migrate",
        ])
        .assert()
        .success()
        .stdout_eq(str![[r#"
[..]  INFO rusqlite_migration: Database migrated to version 1    

"#]])
        .stderr_eq(str![]);
}

#[test]
fn store_version() {
    let store = NamedTempFile::new("db.sqlite3").unwrap();
    let store_path = store.path().to_string_lossy();
    Command::new(cargo_bin("lmb"))
        .args(["--store-path", &store_path, "store", "version"])
        .assert()
        .success()
        .stdout_eq(str![[r#"
0 (no version set)

"#]])
        .stderr_eq(str![]);
}
