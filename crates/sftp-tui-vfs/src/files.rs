//! Reading and writing files as streams of chunks.

use std::fmt;

use futures_util::StreamExt as _;
use futures_util::future::BoxFuture;
use futures_util::stream::FuturesOrdered;

use crate::VfsError;

/// A file open for reading, from its start.
pub trait FileReader: Send + fmt::Debug {
    /// The next part of the file, in order; `None` at its end.
    fn read(&mut self) -> impl Future<Output = Result<Option<Vec<u8>>, VfsError>> + Send;
}

/// A file open for writing, from its start.
pub trait FileWriter: Send + fmt::Debug {
    /// Writes all of `data` after what was written before. Writes may still be on their way
    /// when this returns; [`FileWriter::finish`] waits for them.
    fn write(&mut self, data: Vec<u8>) -> impl Future<Output = Result<(), VfsError>> + Send;

    /// Waits for the writes and closes the file. Some errors, such as a full disk on a server,
    /// show only here; a writer dropped without this may leave a short file.
    fn finish(self) -> impl Future<Output = Result<(), VfsError>> + Send
    where
        Self: Sized;
}

/// What a read at an offset gave: data, `None` at the end of the file, or an error.
pub(crate) type Fetched = Result<Option<Vec<u8>>, VfsError>;

/// Reads at an offset: `(offset, length)` to a future of what came back, which may be less.
pub(crate) type Fetch = Box<dyn Fn(u64, u32) -> BoxFuture<'static, Fetched> + Send>;

/// Keeps several reads of a file in flight, for links where waiting for each answer in turn
/// would leave the line idle, and hands out what they read in order. A read that gives less
/// than it asked for is asked again for the rest, as `sftp(1)` does.
pub(crate) struct ReadPipeline {
    fetch: Fetch,
    /// Reads in flight, by offset: each gives its offset, its length, and what came back.
    pending: FuturesOrdered<BoxFuture<'static, (u64, u32, Fetched)>>,
    /// Where the next new read starts.
    next: u64,
    chunk: u32,
    depth: usize,
    ended: bool,
}

impl ReadPipeline {
    /// Reads of `chunk` bytes, `depth` at a time.
    pub(crate) fn new(fetch: Fetch, chunk: u32, depth: usize) -> Self {
        Self {
            fetch,
            pending: FuturesOrdered::new(),
            next: 0,
            chunk: chunk.max(1),
            depth: depth.max(1),
            ended: false,
        }
    }

    fn request(&self, offset: u64, length: u32) -> BoxFuture<'static, (u64, u32, Fetched)> {
        let fetched = (self.fetch)(offset, length);
        Box::pin(async move { (offset, length, fetched.await) })
    }

    pub(crate) async fn read(&mut self) -> Fetched {
        if self.ended {
            return Ok(None);
        }
        while self.pending.len() < self.depth {
            let request = self.request(self.next, self.chunk);
            self.pending.push_back(request);
            self.next += u64::from(self.chunk);
        }
        let Some((offset, length, fetched)) = self.pending.next().await else {
            return Ok(None);
        };
        match fetched {
            Ok(Some(data)) if !data.is_empty() => {
                let got = u32::try_from(data.len()).unwrap_or(u32::MAX).min(length);
                if got < length {
                    let rest = self.request(offset + u64::from(got), length - got);
                    self.pending.push_front(rest);
                }
                Ok(Some(data))
            }
            // The end: reads after it have nothing either.
            other => {
                self.ended = true;
                self.pending = FuturesOrdered::new();
                other.map(|_| None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::fixture;

    /// Reads in flight now, and the most at once.
    type Flight = Arc<Mutex<(usize, usize)>>;

    /// A file of `size` bytes whose reads at offsets divisible by 3 give half of what they
    /// ask for, and that fails from `fail_at` on. It counts the reads in flight at once.
    fn fake(size: usize, fail_at: Option<u64>) -> (Vec<u8>, Fetch, Flight) {
        let data = fixture::pattern(size);
        let source = data.clone();
        let flight = Arc::new(Mutex::new((0, 0)));
        let counter = Arc::clone(&flight);
        let fetch: Fetch = Box::new(move |offset, length| {
            let source = source.clone();
            let counter = Arc::clone(&counter);
            {
                let mut flight = counter.lock().unwrap();
                flight.0 += 1;
                flight.1 = flight.1.max(flight.0);
            }
            Box::pin(async move {
                tokio::task::yield_now().await;
                counter.lock().unwrap().0 -= 1;
                if fail_at.is_some_and(|at| offset >= at) {
                    return Err(VfsError::PermissionDenied("fake".to_owned()));
                }
                let start = usize::try_from(offset).unwrap();
                if start >= source.len() {
                    return Ok(None);
                }
                let mut length = length as usize;
                if offset % 3 == 0 {
                    length = length.div_ceil(2);
                }
                let end = (start + length).min(source.len());
                Ok(Some(source[start..end].to_vec()))
            })
        });
        (data, fetch, flight)
    }

    async fn read_all(pipeline: &mut ReadPipeline) -> Result<Vec<u8>, VfsError> {
        let mut all = Vec::new();
        while let Some(chunk) = pipeline.read().await? {
            all.extend(chunk);
        }
        Ok(all)
    }

    #[tokio::test]
    async fn reads_in_order_and_asks_again_for_short_reads() {
        for size in [0, 1, 63, 64, 1000, 10_007] {
            let (data, fetch, flight) = fake(size, None);
            let mut pipeline = ReadPipeline::new(fetch, 64, 4);
            assert_eq!(read_all(&mut pipeline).await.unwrap(), data, "{size}");
            assert!(pipeline.read().await.unwrap().is_none(), "ended");
            assert!(flight.lock().unwrap().1 <= 4, "at most four at once");
        }
    }

    #[tokio::test]
    async fn keeps_several_reads_in_flight() {
        let (_, fetch, flight) = fake(10_000, None);
        let mut pipeline = ReadPipeline::new(fetch, 64, 8);
        read_all(&mut pipeline).await.unwrap();
        assert_eq!(flight.lock().unwrap().1, 8);
    }

    #[tokio::test]
    async fn stops_at_an_error() {
        let (_, fetch, _) = fake(1000, Some(320));
        let mut pipeline = ReadPipeline::new(fetch, 64, 4);
        let err = read_all(&mut pipeline).await.unwrap_err();
        assert!(matches!(err, VfsError::PermissionDenied(_)), "{err:?}");
        assert!(pipeline.read().await.unwrap().is_none(), "nothing after it");
    }
}
