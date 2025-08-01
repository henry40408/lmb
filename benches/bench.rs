#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use lmb::Runner;
use rusqlite::Connection;
use serde_json::json;
use tokio::io::empty;

const SOURCE: &str = include_str!("fixtures/true.lua");

fn lmb_call(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    {
        let runner = Runner::builder(SOURCE, empty()).build().unwrap();
        c.bench_function("baseline", |b| {
            b.to_async(&rt)
                .iter(async || runner.invoke().call().await.unwrap().result.unwrap());
        });
    }
    {
        let source = include_str!("fixtures/add.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        c.bench_function("add", |b| {
            b.to_async(&rt).iter(async || {
                runner
                    .invoke()
                    .state(json!(1))
                    .call()
                    .await
                    .unwrap()
                    .result
                    .unwrap()
            });
        });
    }
    {
        let source = include_str!("fixtures/read.lua");
        let text = "";
        let input = Cursor::new(text);
        let runner = Runner::builder(source, input).build().unwrap();
        c.bench_function("read", |b| {
            b.to_async(&rt).iter_batched(
                async || {
                    runner.rewind_input().await.unwrap();
                },
                async |_| runner.invoke().call().await.unwrap().result.unwrap(),
                BatchSize::SmallInput,
            );
        });
    }
    {
        let source = include_str!("fixtures/read-unicode.lua");
        let text = "你好，世界";
        let input = Cursor::new(text);
        let runner = Runner::builder(source, input).build().unwrap();
        c.bench_function("read unicode", |b| {
            b.to_async(&rt).iter_batched(
                async || {
                    runner.rewind_input().await.unwrap();
                },
                async |_| runner.invoke().call().await.unwrap().result.unwrap(),
                BatchSize::SmallInput,
            );
        });
    }
    {
        let source = include_str!("fixtures/json.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        c.bench_function("json encode decode", |b| {
            b.to_async(&rt)
                .iter(async || runner.invoke().call().await.unwrap().result.unwrap());
        });
    }
    {
        let source = include_str!("fixtures/store.lua");
        let conn = Connection::open_in_memory().unwrap();
        let runner = Runner::builder(source, empty())
            .store(conn)
            .build()
            .unwrap();
        c.bench_function("store set get", |b| {
            b.to_async(&rt)
                .iter(async || runner.invoke().call().await.unwrap().result.unwrap());
        });
    }
    {
        let source = include_str!("fixtures/store-update.lua");
        let conn = Connection::open_in_memory().unwrap();
        let runner = Runner::builder(source, empty())
            .store(conn)
            .build()
            .unwrap();
        c.bench_function("store update", |b| {
            b.to_async(&rt)
                .iter(async || runner.invoke().call().await.unwrap().result.unwrap());
        });
    }
    {
        let source = include_str!("fixtures/crypto.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        c.bench_function("crypto", |b| {
            b.to_async(&rt)
                .iter(async || runner.invoke().call().await.unwrap().result.unwrap());
        });
    }
}

criterion_group!(lmb, lmb_call);

criterion_main!(lmb);
