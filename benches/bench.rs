use std::io::{Cursor, empty};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use lmb::Runner;
use serde_json::json;

const SOURCE: &str = include_str!("fixtures/true.lua");

fn lmb_call(c: &mut Criterion) {
    {
        let runner = Runner::builder(SOURCE, empty()).build().unwrap();
        c.bench_function("true", |b| b.iter(|| runner.invoke().call().unwrap()));
    }
    {
        let source = include_str!("fixtures/add.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        c.bench_function("add", |b| {
            b.iter(|| runner.invoke().state(json!(1)).call().unwrap())
        });
    }
    {
        let source = include_str!("fixtures/read.lua");
        let text = "";
        let input = Cursor::new(text);
        let runner = Runner::builder(source, input).build().unwrap();
        c.bench_function("read", |b| {
            b.iter_batched(
                || {
                    runner.rewind_input().unwrap();
                },
                |_| runner.invoke().call().unwrap(),
                BatchSize::SmallInput,
            )
        });
    }
    {
        let source = include_str!("fixtures/read-unicode.lua");
        let text = "你好，世界";
        let input = Cursor::new(text);
        let runner = Runner::builder(source, input).build().unwrap();
        c.bench_function("read unicode", |b| {
            b.iter_batched(
                || {
                    runner.rewind_input().unwrap();
                },
                |_| runner.invoke().call().unwrap(),
                BatchSize::SmallInput,
            )
        });
    }
}

criterion_group!(lmb, lmb_call);

criterion_main!(lmb);
