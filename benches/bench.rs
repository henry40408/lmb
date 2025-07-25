use criterion::{Criterion, criterion_group, criterion_main};
use lmb::Runner;
use serde_json::json;

const SOURCE: &str = include_str!("fixtures/true.lua");

fn lmb_call(c: &mut Criterion) {
    {
        let runner = Runner::builder().source(SOURCE).build().unwrap();
        c.bench_function("true", |b| {
            b.iter(|| runner.call().maybe_state(None).call().unwrap())
        });
    }
    {
        let source = include_str!("fixtures/add.lua");
        let runner = Runner::builder().source(source).build().unwrap();
        c.bench_function("add", |b| {
            b.iter(|| runner.call().state(json!(1)).call().unwrap())
        });
    }
}

criterion_group!(lmb, lmb_call);

criterion_main!(lmb);
