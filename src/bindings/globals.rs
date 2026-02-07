//! Global functions binding module.
//!
//! This module provides global Lua functions that are available without requiring any module import.
//!
//! # Available Functions
//!
//! - `sleep_ms(ms)` - Asynchronously sleep for the specified number of milliseconds.
//!
//! # Example
//!
//! ```lua
//! -- Sleep for 100 milliseconds
//! sleep_ms(100)
//! ```

use crate::{LmbResult, Runner};

pub(crate) fn bind(runner: &mut Runner) -> LmbResult<()> {
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

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use tokio::io::empty;

    use crate::Runner;

    #[tokio::test]
    async fn test_sleep_ms() {
        let source = include_str!("../fixtures/bindings/globals/sleep.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        let start = Instant::now();
        let result = runner.invoke().call().await.unwrap();
        let elapsed = start.elapsed();
        assert!(result.result.is_ok());
        // Verify that sleep actually waited (at least 40ms to allow for timing variance)
        assert!(elapsed.as_millis() >= 40);
    }

    #[tokio::test]
    async fn test_sleep_ms_zero() {
        let source = include_str!("../fixtures/bindings/globals/sleep-zero.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        let result = runner.invoke().call().await.unwrap();
        assert!(result.result.is_ok());
    }
}
