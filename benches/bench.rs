use std::io::{BufReader, Cursor, Read as _};

use bencher::{Bencher, benchmark_group, benchmark_main};
use lmb::{Evaluation, Store};
use mlua::Lua;
use serde_json::json;
use tokio::io::empty;

static SCRIPT: &str = "return true";

fn mlua_call(bencher: &mut Bencher) {
    let vm = Lua::new();
    vm.sandbox(true).unwrap();
    let f = vm.load(SCRIPT).into_function().unwrap();
    bencher.iter(|| f.call::<bool>(()).unwrap());
}

fn mlua_eval(bencher: &mut Bencher) {
    let vm = Lua::new();
    bencher.iter(|| vm.load(SCRIPT).eval::<bool>());
}

fn mlua_sandbox_eval(bencher: &mut Bencher) {
    let vm = Lua::new();
    vm.sandbox(true).unwrap();
    bencher.iter(|| vm.load(SCRIPT).eval::<bool>());
}

fn lmb_evaluate(bencher: &mut Bencher) {
    let e = Evaluation::builder(SCRIPT, empty()).build().unwrap();
    bencher.iter(|| e.evaluate().call().unwrap());
}
fn lmb_no_store(bencher: &mut Bencher) {
    let e = Evaluation::builder(SCRIPT, empty()).build().unwrap();
    bencher.iter(|| e.evaluate().call().unwrap());
}

fn store_update(bencher: &mut Bencher) {
    let store = Store::default();
    store.put("a", &json!(0)).unwrap();
    bencher.iter(|| {
        store.update(
            &["a"],
            |old| {
                old.insert("a".into(), json!(1));
                Ok(())
            },
            None,
        )
    });
}

fn lmb_default_store(bencher: &mut Bencher) {
    let store = Store::default();
    let e = Evaluation::builder(SCRIPT, empty())
        .store(store)
        .build()
        .unwrap();
    bencher.iter(|| e.evaluate().call().unwrap());
}

fn lmb_update(bencher: &mut Bencher) {
    let script = r#"
        return require("@lmb").store:update({ "a" }, function(s)
          s.a = s.a + 1
        end, { a = 0 })
        "#;
    let store = Store::default();
    let e = Evaluation::builder(script, empty())
        .store(store)
        .build()
        .unwrap();
    bencher.iter(|| e.evaluate().call().unwrap());
}

fn lmb_read_all(bencher: &mut Bencher) {
    let input = Cursor::new("0");
    let script = "return io.read('*a')";
    let e = Evaluation::builder(script, input).build().unwrap();
    bencher.iter(|| {
        let _ = e.rewind_input();
        e.evaluate().call().unwrap()
    });
}

fn lmb_read_line(bencher: &mut Bencher) {
    let input = Cursor::new("0");
    let script = "return io.read('*l')";
    let e = Evaluation::builder(script, input).build().unwrap();
    bencher.iter(|| {
        let _ = e.rewind_input();
        e.evaluate().call().unwrap()
    });
}

fn lmb_read_number(bencher: &mut Bencher) {
    let input = Cursor::new("0");
    let script = "return io.read('*n')";
    let e = Evaluation::builder(script, input).build().unwrap();
    bencher.iter(|| {
        let _ = e.rewind_input();
        e.evaluate().call().unwrap()
    });
}

fn lmb_read_unicode(bencher: &mut Bencher) {
    let input = Cursor::new("0");
    let script = "return require('@lmb'):read_unicode(1)";
    let e = Evaluation::builder(script, input).build().unwrap();
    bencher.iter(|| {
        let _ = e.rewind_input();
        e.evaluate().call().unwrap()
    });
}

fn read_from_buf_reader(bencher: &mut Bencher) {
    let mut r = BufReader::new(Cursor::new("1"));
    bencher.iter(|| {
        let mut buf = vec![0; 1];
        let _ = r.read(&mut buf);
    });
}

benchmark_group!(
    evaluation,
    mlua_call,
    mlua_eval,
    mlua_sandbox_eval,
    lmb_evaluate,
);
benchmark_group!(
    read,
    read_from_buf_reader,
    lmb_read_all,
    lmb_read_line,
    lmb_read_number,
    lmb_read_unicode,
);
benchmark_group!(
    store,
    store_update,
    lmb_no_store,
    lmb_default_store,
    lmb_update,
);
benchmark_main!(evaluation, read, store);
