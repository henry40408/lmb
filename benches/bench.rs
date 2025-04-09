#![allow(clippy::unwrap_used)]

static SCRIPT: &str = "return true";

mod evaluation {
    use divan::Bencher;
    use lmb::Evaluation;
    use mlua::prelude::*;
    use tokio::io::empty;

    use crate::SCRIPT;

    #[divan::bench]
    fn lmb_evaluate(bencher: Bencher<'_, '_>) {
        let e = Evaluation::builder(SCRIPT, empty()).build().unwrap();
        bencher.bench(|| e.evaluate().call().unwrap());
    }

    #[divan::bench]
    fn mlua_call(bencher: Bencher<'_, '_>) {
        let vm = Lua::new();
        vm.sandbox(true).unwrap();
        let f = vm.load(SCRIPT).into_function().unwrap();
        bencher.bench(|| f.call::<bool>(()).unwrap());
    }

    #[divan::bench]
    fn mlua_eval(bencher: Bencher<'_, '_>) {
        let vm = Lua::new();
        bencher.bench(|| vm.load(SCRIPT).eval::<bool>());
    }

    #[divan::bench]
    fn mlua_sandbox_eval(bencher: Bencher<'_, '_>) {
        let vm = Lua::new();
        vm.sandbox(true).unwrap();
        bencher.bench(|| vm.load(SCRIPT).eval::<bool>());
    }
}

mod store {
    use divan::Bencher;
    use lmb::{Evaluation, Store};
    use serde_json::json;
    use tokio::io::empty;

    use crate::SCRIPT;

    #[divan::bench]
    fn lmb_no_store(bencher: Bencher<'_, '_>) {
        let e = Evaluation::builder(SCRIPT, empty()).build().unwrap();
        bencher.bench(|| e.evaluate().call().unwrap());
    }

    #[divan::bench]
    fn store_update(bencher: Bencher<'_, '_>) {
        let store = Store::default();
        store.put("a", &json!(0)).unwrap();
        bencher.bench(|| {
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

    #[divan::bench]
    fn lmb_default_store(bencher: Bencher<'_, '_>) {
        let store = Store::default();
        let e = Evaluation::builder(SCRIPT, empty())
            .store(store)
            .build()
            .unwrap();
        bencher.bench(|| e.evaluate().call().unwrap());
    }

    #[divan::bench]
    fn lmb_update(bencher: Bencher<'_, '_>) {
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
        bencher.bench(|| e.evaluate().call().unwrap());
    }
}

mod read {
    use std::io::{BufReader, Cursor, Read as _};

    use divan::Bencher;
    use lmb::Evaluation;

    #[divan::bench]
    fn lmb_read_all(bencher: Bencher<'_, '_>) {
        let input = Cursor::new("0");
        let script = "return io.read('*a')";
        let e = Evaluation::builder(script, input).build().unwrap();
        bencher.bench(|| {
            let _ = e.rewind_input();
            e.evaluate().call().unwrap()
        });
    }

    #[divan::bench]
    fn lmb_read_line(bencher: Bencher<'_, '_>) {
        let input = Cursor::new("0");
        let script = "return io.read('*l')";
        let e = Evaluation::builder(script, input).build().unwrap();
        bencher.bench(|| {
            let _ = e.rewind_input();
            e.evaluate().call().unwrap()
        });
    }

    #[divan::bench]
    fn lmb_read_number(bencher: Bencher<'_, '_>) {
        let input = Cursor::new("0");
        let script = "return io.read('*n')";
        let e = Evaluation::builder(script, input).build().unwrap();
        bencher.bench(|| {
            let _ = e.rewind_input();
            e.evaluate().call().unwrap()
        });
    }

    #[divan::bench]
    fn lmb_read_unicode(bencher: Bencher<'_, '_>) {
        let input = Cursor::new("0");
        let script = "return require('@lmb'):read_unicode(1)";
        let e = Evaluation::builder(script, input).build().unwrap();
        bencher.bench(|| {
            let _ = e.rewind_input();
            e.evaluate().call().unwrap()
        });
    }

    #[divan::bench]
    fn read_from_buf_reader(bencher: Bencher<'_, '_>) {
        bencher
            .with_inputs(|| BufReader::new(Cursor::new("1")))
            .bench_refs(|r: &mut BufReader<_>| {
                let mut buf = vec![0; 1];
                let _ = r.read(&mut buf);
            });
    }
}

fn main() {
    divan::main();
}
