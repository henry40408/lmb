use tokio::io::AsyncRead;

use crate::{LmbResult, Runner};

pub(crate) fn bind<R>(runner: &mut Runner<R>) -> LmbResult<()>
where
    for<'lua> R: 'lua + AsyncRead + Unpin,
{
    let globals = runner.vm.globals();

    globals.set(
        "sleep_ms",
        runner.vm.create_async_function(|_, ms: u64| async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(ms)).await;
            Ok(())
        })?,
    )?;

    Ok(())
}
