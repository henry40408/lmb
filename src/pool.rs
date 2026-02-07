use bon::bon;
use deadpool::managed;
use mlua::AsChunk;

use crate::{LmbError, LmbInput, LmbStore, Runner};

/// A manager for Lua script runners
#[derive(Debug)]
pub struct RunnerManager<S>
where
    S: AsChunk + Clone,
{
    source: S,
    reader: LmbInput,
    store: Option<LmbStore>,
}

#[bon]
impl<S> RunnerManager<S>
where
    S: AsChunk + Clone,
{
    /// Creates a new `RunnerManager`
    #[builder]
    pub fn new(
        #[builder(start_fn)] source: S,
        #[builder(start_fn)] reader: LmbInput,
        store: Option<LmbStore>,
    ) -> Self {
        Self {
            source,
            reader,
            store,
        }
    }
}

impl<S> managed::Manager for RunnerManager<S>
where
    S: AsChunk + Clone + Send + Sync,
{
    type Type = Runner;
    type Error = LmbError;

    async fn create(&self) -> Result<Self::Type, Self::Error> {
        Runner::from_shared_reader(self.source.clone(), self.reader.clone())
            .maybe_store(self.store.clone())
            .call()
    }

    async fn recycle(
        &self,
        _obj: &mut Self::Type,
        _metrics: &managed::Metrics,
    ) -> managed::RecycleResult<Self::Error> {
        Ok(())
    }
}

/// A pool of Lua script runners
pub type Pool<S> = managed::Pool<RunnerManager<S>>;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parking_lot::Mutex;
    use rusqlite::Connection;
    use serde_json::json;
    use tokio::io::empty;

    use crate::{
        pool::{Pool, RunnerManager},
        reader::SharedReader,
        store::Store,
    };

    #[tokio::test]
    async fn test_pool() {
        let source = include_str!("./fixtures/bindings/store/store.lua");
        let reader = Arc::new(SharedReader::new(empty()));

        let store = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        let manager = RunnerManager::builder(source, reader)
            .store(store.clone())
            .build();

        let pool = Pool::builder(manager).build().unwrap();
        let mut tasks = vec![];
        for _i in 0..10 {
            let pool = pool.clone();
            tasks.push(tokio::spawn(async move {
                let runner = pool.get().await.unwrap();
                runner.invoke().call().await.unwrap();
            }));
        }

        futures::future::join_all(tasks).await;

        let store = Store::builder(store).build();
        let value = store.get("a").unwrap().unwrap();
        assert_eq!(json!(true), value);
    }

    #[tokio::test]
    async fn test_pool_respects_max_size() {
        let source = r#"return function() return true end"#;
        let reader = Arc::new(SharedReader::new(empty()));

        let manager = RunnerManager::builder(source, reader).build();
        let pool = Pool::builder(manager).max_size(2).build().unwrap();

        // Get two runners, which should exhaust the pool
        let runner1 = pool.get().await.unwrap();
        let runner2 = pool.get().await.unwrap();

        // Pool status should reflect the usage
        let status = pool.status();
        assert_eq!(2, status.max_size);
        assert_eq!(2, status.size);

        // Return runners to pool
        drop(runner1);
        drop(runner2);
    }

    #[tokio::test]
    async fn test_pool_concurrent_requests() {
        let source = r#"
            local count = 0
            return function()
                count = count + 1
                return count
            end
        "#;
        let reader = Arc::new(SharedReader::new(empty()));

        let manager = RunnerManager::builder(source, reader).build();
        let pool = Pool::builder(manager).max_size(4).build().unwrap();

        let mut handles = vec![];
        for _ in 0..20 {
            let pool = pool.clone();
            handles.push(tokio::spawn(async move {
                let runner = pool.get().await.unwrap();
                runner.invoke().call().await.unwrap().result.unwrap()
            }));
        }

        let results: Vec<_> = futures::future::join_all(handles)
            .await
            .into_iter()
            .map(|r| r.unwrap())
            .collect();

        // All invocations should succeed
        assert_eq!(20, results.len());
    }

    #[tokio::test]
    async fn test_pool_recycles_runners() {
        let source = r#"
            local count = 0
            return function()
                count = count + 1
                return count
            end
        "#;
        let reader = Arc::new(SharedReader::new(empty()));

        let manager = RunnerManager::builder(source, reader).build();
        let pool = Pool::builder(manager).max_size(1).build().unwrap();

        // First call
        {
            let runner = pool.get().await.unwrap();
            let result = runner.invoke().call().await.unwrap().result.unwrap();
            assert_eq!(json!(1), result);
        }

        // Second call - should reuse the same runner and increment count
        {
            let runner = pool.get().await.unwrap();
            let result = runner.invoke().call().await.unwrap().result.unwrap();
            assert_eq!(json!(2), result);
        }

        // Third call - should still be the same runner
        {
            let runner = pool.get().await.unwrap();
            let result = runner.invoke().call().await.unwrap().result.unwrap();
            assert_eq!(json!(3), result);
        }
    }

    #[tokio::test]
    async fn test_pool_without_store() {
        let source = r#"return function() return "no store" end"#;
        let reader = Arc::new(SharedReader::new(empty()));

        let manager = RunnerManager::builder(source, reader).build();
        let pool = Pool::builder(manager).max_size(2).build().unwrap();

        let runner = pool.get().await.unwrap();
        let result = runner.invoke().call().await.unwrap().result.unwrap();
        assert_eq!(json!("no store"), result);
    }
}
