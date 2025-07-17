use std::io::{BufReader, Cursor, Read as _};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use lmb::{Evaluation, Store};
use mlua::Lua;
use serde_json::json;
use tokio::io::empty;

static SCRIPT: &str = "return true";

fn mlua_call(c: &mut Criterion) {
    let vm = Lua::new();
    let f = vm.load(SCRIPT).into_function().unwrap();
    c.bench_function("mlua call", |b| {
        b.iter(|| f.call::<bool>(()).unwrap());
    });
}

fn mlua_eval(c: &mut Criterion) {
    let vm = Lua::new();
    c.bench_function("mlua eval", |b| {
        b.iter(|| vm.load(SCRIPT).eval::<bool>());
    });
}

fn mlua_sandbox_call(c: &mut Criterion) {
    let vm = Lua::new();
    vm.sandbox(true).unwrap();
    let f = vm.load(SCRIPT).into_function().unwrap();
    c.bench_function("mlua sandbox call", |b| {
        b.iter(|| f.call::<bool>(()).unwrap());
    });
}

fn mlua_sandbox_eval(c: &mut Criterion) {
    let vm = Lua::new();
    vm.sandbox(true).unwrap();
    c.bench_function("mlua sandbox eval", |b| {
        b.iter(|| vm.load(SCRIPT).eval::<bool>());
    });
}

fn lmb_evaluate(c: &mut Criterion) {
    let e = Evaluation::builder(SCRIPT, empty()).build().unwrap();
    c.bench_function("lmb evaluate", |b| {
        b.iter(|| e.evaluate().call().unwrap());
    });
}
fn lmb_no_store(c: &mut Criterion) {
    let e = Evaluation::builder(SCRIPT, empty()).build().unwrap();
    c.bench_function("lmb no store", |b| {
        b.iter(|| e.evaluate().call().unwrap());
    });
}

fn store_update(c: &mut Criterion) {
    let store = Store::default();
    store.put("a", &json!(0)).unwrap();

    c.bench_function("store update", |b| {
        b.iter(|| {
            store.update(
                &["a"],
                |old| {
                    old.insert("a".into(), json!(1));
                    Ok(())
                },
                None,
            )
        });
    });
}

fn lmb_default_store(c: &mut Criterion) {
    let store = Store::default();
    let e = Evaluation::builder(SCRIPT, empty())
        .store(store)
        .build()
        .unwrap();
    c.bench_function("lmb default store", |b| {
        b.iter(|| e.evaluate().call().unwrap());
    });
}

fn lmb_update(c: &mut Criterion) {
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
    c.bench_function("lmb update", |b| {
        b.iter(|| e.evaluate().call().unwrap());
    });
}

fn lmb_read_all(c: &mut Criterion) {
    let input = Cursor::new("0");
    let script = "return io.read('*a')";
    let e = Evaluation::builder(script, input).build().unwrap();
    c.bench_function("lmb read all", |b| {
        b.iter_batched(
            || {
                let _ = e.rewind_input();
            },
            |_| e.evaluate().call().unwrap(),
            BatchSize::SmallInput,
        );
    });
}

fn lmb_read_line(c: &mut Criterion) {
    let input = Cursor::new("0");
    let script = "return io.read('*l')";
    let e = Evaluation::builder(script, input).build().unwrap();
    c.bench_function("lmb read line", |b| {
        b.iter_batched(
            || {
                let _ = e.rewind_input();
            },
            |_| e.evaluate().call().unwrap(),
            BatchSize::SmallInput,
        );
    });
}

fn lmb_read_number(c: &mut Criterion) {
    let input = Cursor::new("0");
    let script = "return io.read('*n')";
    let e = Evaluation::builder(script, input).build().unwrap();
    c.bench_function("lmb read number", |b| {
        b.iter_batched(
            || {
                let _ = e.rewind_input();
            },
            |_| e.evaluate().call().unwrap(),
            BatchSize::SmallInput,
        );
    });
}

fn lmb_read_unicode(c: &mut Criterion) {
    let input = Cursor::new("0");
    let script = "return require('@lmb'):read_unicode(1)";
    let e = Evaluation::builder(script, input).build().unwrap();
    c.bench_function("lmb read unicode", |b| {
        b.iter_batched(
            || {
                let _ = e.rewind_input();
            },
            |_| e.evaluate().call().unwrap(),
            BatchSize::SmallInput,
        );
    });
}

fn read_from_buf_reader(c: &mut Criterion) {
    let mut r = BufReader::new(Cursor::new("1"));
    c.bench_function("read from buf reader", |b| {
        b.iter_batched_ref(
            || vec![0; 1],
            |mut buf| {
                let _ = r.read(&mut buf);
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    evaluation,
    mlua_call,
    mlua_eval,
    mlua_sandbox_call,
    mlua_sandbox_eval,
    lmb_evaluate,
);
criterion_group!(
    read,
    read_from_buf_reader,
    lmb_read_all,
    lmb_read_line,
    lmb_read_number,
    lmb_read_unicode,
);
criterion_group!(
    store,
    store_update,
    lmb_no_store,
    lmb_default_store,
    lmb_update,
);
criterion_main!(evaluation, read, store);
