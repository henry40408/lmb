use assert_fs::NamedTempFile;
use snapbox::{
    cmd::{cargo_bin, Command},
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
        .stderr_eq(str![[r#"
Error: leftover token
   ,-[-:1:1]
 1 |ret true
   | `-- leftover token

"#]]);
}

#[test]
fn check_stdin_tokenizer_error() {
    Command::new(cargo_bin("lmb"))
        .stdin("return !true")
        .args(["--no-color", "check", "--file", "-"])
        .assert()
        .failure()
        .stderr_eq(str![[r#"
Error: unexpected character !
   ,-[-:1:8]
 1 |return !true
   |       `- unexpected character !

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

"#]]);
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
"#]]);
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
"#]]);
}

#[test]
fn eval_stdin_syntax_error() {
    Command::new(cargo_bin("lmb"))
        .stdin("return !true")
        .args(["--no-color", "eval", "--file", "-"])
        .assert()
        .failure()
        .stderr_eq(str![[r#"
Error: Unexpected '!'; did you mean 'not'?
   ,-[-:1:1]
 1 |return !true
   |      `------ Unexpected '!'; did you mean 'not'?

"#]]);
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
"#]]);
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
"#]]);
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
 error         Demonstrate how the runner reacts when an error is thrown.             
 hello         Hello, world!                                                          
 http-echo     Echo headers and body from HTTP request.                               
 input         Echo the standard input.                                               
 read-unicode  Read 2 unicode characters from the standard input.                     
 return-table  The function can also return a table.                                  
               Please note that JSON mode is needed to show the whole table,          
               otherwise "table: 0x..." will be printed, which aligns how Lua works.  
 store         Update an absent key 'a' in store and return the new value.            
               Please note that since store is epheremal the output will always be 1. 
                                                                                      

"#]]);
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
            "127.0.0.1:3000",
            "--name",
            "hello",
        ])
        .assert()
        .stdout_eq(str![[r#"
[..]  INFO rusqlite_migration: Database migrated to version 1    
[..]  WARN lmb::serve: no store path is specified, an in-memory store will be used and values will be lost when process ends
[..]  INFO lmb::serve: serving lua script bind=127.0.0.1:3000

"#]]);
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

"#]]);
}

#[test]
fn schedule() {
    let store = NamedTempFile::new("db.sqlite3").unwrap();
    let store_path = store.path().to_string_lossy();

    Command::new(cargo_bin("lmb"))
        .stdin("require('@lmb'):put('a', 1); return true")
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
"#]]);

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
        .stdout_eq(str!["1"]);
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
"#]]);
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
"#]]);

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

"#]]);

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
        .stdout_eq(str!["1"]);
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

"#]]);
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

"#]]);
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

"#]]);
}
