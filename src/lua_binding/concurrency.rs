use mlua::prelude::*;
use parking_lot::Mutex;
use std::{sync::Arc, time::Duration};
use tokio::task::JoinHandle;

struct AsyncTask<T: 'static + Send> {
    handle: Arc<Mutex<Option<JoinHandle<T>>>>,
}

impl<T: 'static + Send> AsyncTask<T> {
    fn new<F>(future: F) -> Self
    where
        F: Future<Output = T> + Send + 'static,
    {
        let handle = tokio::spawn(future);
        Self {
            handle: Arc::new(Mutex::new(Some(handle))),
        }
    }
}

impl<T: 'static + Send + Clone + mlua::IntoLuaMulti> LuaUserData for AsyncTask<T> {
    fn add_methods<'lua, M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method("join", |lua, this, ()| {
            let handle_arc = Arc::clone(&this.handle);
            async move {
                let handle = {
                    let mut guard = handle_arc.lock();
                    guard.take()
                };
                if let Some(handle) = handle {
                    match handle.await {
                        Ok(result) => result.into_lua_multi(&lua),
                        Err(e) => Err(LuaError::runtime(format!("Task join error: {}", e))),
                    }
                } else {
                    Err(LuaError::runtime("Task already joined"))
                }
            }
        });
    }
}

pub struct LuaModAsync {}

impl LuaUserData for LuaModAsync {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method("join_all", |vm, _this, (tasks,): (LuaTable,)| async move {
            let mut futures = vec![];
            for (_, task_userdata) in tasks.pairs::<LuaValue, LuaAnyUserData>().flatten() {
                if let Ok(task) = task_userdata.borrow::<AsyncTask<LuaResult<LuaValue>>>() {
                    let handle_arc = Arc::clone(&task.handle);
                    let future = async move {
                        let handle = {
                            let mut guard = handle_arc.lock();
                            guard.take()
                        };
                        if let Some(handle) = handle {
                            match handle.await {
                                Ok(result) => Ok(result),
                                Err(e) => Err(format!("Task join error: {}", e)),
                            }
                        } else {
                            Err("Task already joined".to_string())
                        }
                    };
                    futures.push(future);
                } else {
                    return Err(mlua::Error::runtime("Table contains non-AsyncTask values"));
                }
            }
            let results = futures::future::join_all(futures).await;
            let results_table = vm.create_table()?;
            for (i, result) in results.into_iter().enumerate() {
                match result {
                    Ok(value) => results_table.set(i + 1, value?)?,
                    Err(err) => results_table.set(i + 1, err)?,
                }
            }
            Ok(results_table)
        });
        methods.add_method("sleep_async", |_, _, secs: f64| {
            let sleep_future = async move {
                let duration = Duration::from_secs_f64(secs);
                tokio::time::sleep(duration).await;
                Ok::<LuaValue, LuaError>(LuaValue::Number(secs))
            };
            Ok(AsyncTask::new(sleep_future))
        });
    }
}
