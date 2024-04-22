#![allow(clippy::unwrap_used)]

use bencher::{benchmark_group, benchmark_main, Bencher};
use lam::{EvalBuilder, LamStore};
use mlua::prelude::*;
use std::io::{BufReader, Cursor, Read as _};

static SCRIPT: &str = "return true";

/// evaluation

fn lam_evaluate(bencher: &mut Bencher) {
    let e = EvalBuilder::new(SCRIPT).build();
    bencher.iter(|| e.evaluate().unwrap());
}

fn mlua_load_eval(bencher: &mut Bencher) {
    let vm = Lua::new();
    bencher.iter(|| vm.load(SCRIPT).eval::<bool>());
}

fn mlua_call_function(bencher: &mut Bencher) {
    let vm = Lua::new();
    let f = vm.load(SCRIPT).into_function().unwrap();
    bencher.iter(|| f.call::<_, bool>(()).unwrap());
}

/// store

fn lam_no_store(bencher: &mut Bencher) {
    bencher.iter(|| {
        let e = EvalBuilder::new(SCRIPT).build();
        e.evaluate().unwrap()
    });
}

fn lam_default_store(bencher: &mut Bencher) {
    bencher.iter(|| {
        let store = LamStore::default();
        let e = EvalBuilder::new(SCRIPT).set_store(store).build();
        e.evaluate().unwrap()
    });
}

/// read

fn lam_read_all(bencher: &mut Bencher) {
    let input = "1";
    let script = "return require('@lam'):read('*a')";
    bencher.iter(|| {
        let e = EvalBuilder::new(script)
            .set_input(Some(input.as_bytes()))
            .build();
        e.evaluate().unwrap()
    });
}

fn lam_read_line(bencher: &mut Bencher) {
    let input = "1";
    let script = "return require('@lam'):read('*l')";
    bencher.iter(|| {
        let e = EvalBuilder::new(script)
            .set_input(Some(input.as_bytes()))
            .build();
        e.evaluate().unwrap()
    });
}

fn lam_read_number(bencher: &mut Bencher) {
    let input = "1";
    let script = "return require('@lam'):read('*n')";
    bencher.iter(|| {
        let e = EvalBuilder::new(script)
            .set_input(Some(input.as_bytes()))
            .build();
        e.evaluate().unwrap()
    });
}

fn read_from_buf_reader(bencher: &mut Bencher) {
    bencher.iter(|| {
        let mut r = BufReader::new(Cursor::new("1"));
        let mut buf = vec![0; 1];
        let _ = r.read(&mut buf);
    });
}

benchmark_group!(evaluation, lam_evaluate, mlua_load_eval, mlua_call_function);
benchmark_group!(
    read,
    lam_read_all,
    lam_read_line,
    lam_read_number,
    read_from_buf_reader,
);
benchmark_group!(store, lam_default_store, lam_no_store);
benchmark_main!(evaluation, read, store);
