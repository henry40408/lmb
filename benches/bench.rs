#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use lmb::{Runner, State};
use rusqlite::Connection;
use serde_json::json;
use tokio::io::empty;

fn lmb_call(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    {
        let source = include_str!("../src/fixtures/core/true.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        c.bench_function("baseline", |b| {
            b.to_async(&rt)
                .iter(|| async { runner.invoke().call().await.unwrap().result.unwrap() });
        });
    }
    {
        let source = include_str!("../src/fixtures/core/true-expr.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        c.bench_function("baseline expr", |b| {
            b.to_async(&rt)
                .iter(|| async { runner.invoke().call().await.unwrap().result.unwrap() });
        });
    }
    {
        let source = include_str!("../src/fixtures/core/add.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        c.bench_function("add", |b| {
            b.to_async(&rt).iter(|| async {
                let state = State::builder().state(json!(1)).build();
                runner
                    .invoke()
                    .state(state)
                    .call()
                    .await
                    .unwrap()
                    .result
                    .unwrap()
            });
        });
    }
    {
        let source = include_str!("../src/fixtures/bindings/io/read-all.lua");
        let text = "";
        let runner = Runner::builder(source, Cursor::new(text)).build().unwrap();
        c.bench_function("read", |b| {
            b.to_async(&rt).iter_batched(
                || async {
                    runner.swap_reader(Cursor::new(text)).await;
                },
                |_| async { runner.invoke().call().await.unwrap().result.unwrap() },
                BatchSize::SmallInput,
            );
        });
    }
    {
        let source = include_str!("../src/fixtures/bindings/io/read-unicode.lua");
        let text = "你好，世界";
        let runner = Runner::builder(source, Cursor::new(text)).build().unwrap();
        c.bench_function("read unicode", |b| {
            b.to_async(&rt).iter_batched(
                || async {
                    runner.swap_reader(Cursor::new(text)).await;
                },
                |_| async { runner.invoke().call().await.unwrap().result.unwrap() },
                BatchSize::SmallInput,
            );
        });
    }
    {
        let source = include_str!("../src/fixtures/bindings/codecs/json.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        c.bench_function("json encode decode", |b| {
            b.to_async(&rt)
                .iter(|| async { runner.invoke().call().await.unwrap().result.unwrap() });
        });
    }
    {
        let source = include_str!("../src/fixtures/bindings/store/store.lua");
        let conn = Connection::open_in_memory().unwrap();
        let runner = Runner::builder(source, empty())
            .store(conn)
            .build()
            .unwrap();
        c.bench_function("store set get", |b| {
            b.to_async(&rt)
                .iter(|| async { runner.invoke().call().await.unwrap().result.unwrap() });
        });
    }
    {
        let source = include_str!("../src/fixtures/bindings/store/store-update.lua");
        let conn = Connection::open_in_memory().unwrap();
        let runner = Runner::builder(source, empty())
            .store(conn)
            .build()
            .unwrap();
        c.bench_function("store update", |b| {
            b.to_async(&rt)
                .iter(|| async { runner.invoke().call().await.unwrap().result.unwrap() });
        });
    }
    {
        let source = include_str!("../src/fixtures/bindings/crypto.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        c.bench_function("crypto", |b| {
            b.to_async(&rt)
                .iter(|| async { runner.invoke().call().await.unwrap().result.unwrap() });
        });
    }
}

criterion_group!(lmb, lmb_call);

criterion_main!(lmb);
