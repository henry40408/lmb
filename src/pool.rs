use bon::bon;
use deadpool::managed;
use mlua::AsChunk;
use tokio::io::AsyncRead;

use crate::{LmbError, LmbInput, LmbStore, Runner};

/// A manager for Lua script runners
#[derive(Debug)]
pub struct RunnerManager<R, S>
where
    for<'lua> R: 'lua + AsyncRead + Unpin,
    S: AsChunk + Clone,
{
    source: S,
    reader: LmbInput<R>,
    store: Option<LmbStore>,
}

#[bon]
impl<R, S> RunnerManager<R, S>
where
    for<'lua> R: 'lua + AsyncRead + Unpin,
    S: AsChunk + Clone,
{
    /// Creates a new `RunnerManager`
    #[builder]
    pub fn new(
        #[builder(start_fn)] source: S,
        #[builder(start_fn)] reader: LmbInput<R>,
        store: Option<LmbStore>,
    ) -> Self {
        Self {
            source,
            reader,
            store,
        }
    }
}

impl<R, S> managed::Manager for RunnerManager<R, S>
where
    for<'lua> R: 'lua + AsyncRead + Send + Sync + Unpin,
    S: AsChunk + Clone + Send + Sync,
{
    type Type = Runner<R>;
    type Error = LmbError;

    async fn create(&self) -> Result<Self::Type, Self::Error> {
        Runner::from_arc_mutex(self.source.clone(), self.reader.clone())
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
pub type Pool<R, S> = managed::Pool<RunnerManager<R, S>>;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parking_lot::Mutex;
    use rusqlite::Connection;
    use serde_json::json;
    use tokio::{
        io::{BufReader, empty},
        sync::Mutex as AsyncMutex,
    };

    use crate::{
        pool::{Pool, RunnerManager},
        store::Store,
    };

    #[tokio::test]
    async fn test_pool() {
        let source = include_str!("./fixtures/store.lua");
        let reader = Arc::new(AsyncMutex::new(BufReader::new(empty())));

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
        assert_eq!(json!(10), value);
    }
}
