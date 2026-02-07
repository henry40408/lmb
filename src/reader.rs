//! Shared reader module for swappable async readers.

use std::fmt;

use tokio::io::{AsyncBufRead, AsyncRead, BufReader};
use tokio::sync::Mutex;

/// A trait object for async buffered reading.
pub type DynAsyncBufRead = dyn AsyncBufRead + Send + Unpin;

/// A shared reader that can be swapped at runtime.
///
/// This allows reusing a Runner across multiple requests by swapping
/// the underlying reader between invocations.
pub struct SharedReader {
    inner: Mutex<Box<DynAsyncBufRead>>,
}

impl fmt::Debug for SharedReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharedReader").finish_non_exhaustive()
    }
}

impl SharedReader {
    /// Creates a new `SharedReader` with the given reader.
    pub fn new<R>(reader: R) -> Self
    where
        R: AsyncRead + Send + Unpin + 'static,
    {
        Self {
            inner: Mutex::new(Box::new(BufReader::new(reader))),
        }
    }

    /// Creates a new `SharedReader` with an already buffered reader.
    pub fn from_buf_reader<R>(reader: BufReader<R>) -> Self
    where
        R: AsyncRead + Send + Unpin + 'static,
    {
        Self {
            inner: Mutex::new(Box::new(reader)),
        }
    }

    /// Swaps the underlying reader with a new one.
    pub async fn swap<R>(&self, reader: R)
    where
        R: AsyncRead + Send + Unpin + 'static,
    {
        *self.inner.lock().await = Box::new(BufReader::new(reader));
    }

    /// Swaps the underlying reader with an already buffered reader.
    pub async fn swap_buf_reader<R>(&self, reader: BufReader<R>)
    where
        R: AsyncRead + Send + Unpin + 'static,
    {
        *self.inner.lock().await = Box::new(reader);
    }

    /// Returns a guard that provides access to the underlying reader.
    pub async fn lock(&self) -> tokio::sync::MutexGuard<'_, Box<DynAsyncBufRead>> {
        self.inner.lock().await
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use tokio::io::AsyncReadExt;

    use super::*;

    #[tokio::test]
    async fn test_shared_reader_basic() {
        let reader = SharedReader::new(Cursor::new(b"hello"));
        let mut buf = vec![0u8; 5];
        reader.lock().await.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello");
    }

    #[tokio::test]
    async fn test_shared_reader_swap() {
        let reader = SharedReader::new(Cursor::new(b"first"));
        let mut buf = vec![0u8; 5];
        reader.lock().await.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"first");

        reader.swap(Cursor::new(b"second")).await;
        let mut buf = vec![0u8; 6];
        reader.lock().await.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"second");
    }
}
