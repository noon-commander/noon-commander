//! Reading and writing files as streams of chunks.

use std::fmt;
use std::ops::Range;

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
    /// when this returns: [`FileWriter::written`] tells how much has arrived, and
    /// [`FileWriter::settle`] and [`FileWriter::finish`] wait for the rest.
    fn write(&mut self, data: Vec<u8>) -> impl Future<Output = Result<(), VfsError>> + Send;

    /// Bytes that have arrived so far: handed to the system for a local file, taken by the
    /// server for a remote one.
    fn written(&self) -> u64;

    /// Waits until more of the writes on their way have arrived; `false` once none are left.
    fn settle(&mut self) -> impl Future<Output = Result<bool, VfsError>> + Send;

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
/// than it asked for is asked again for the rest, as `sftp(1)` does. No read reaches past the
/// end of the range: what a server is asked for it sends, wanted or not.
pub(crate) struct ReadPipeline {
    fetch: Fetch,
    /// Reads in flight, by offset: each gives its offset, its length, and what came back.
    pending: FuturesOrdered<BoxFuture<'static, (u64, u32, Fetched)>>,
    /// Where the next new read starts.
    next: u64,
    /// Where the last read ends.
    end: u64,
    chunk: u32,
    depth: usize,
    ended: bool,
}

impl ReadPipeline {
    /// Reads of `chunk` bytes of `range`, `depth` at a time; a range to `u64::MAX` has no end
    /// but the file's.
    pub(crate) fn new(fetch: Fetch, range: Range<u64>, chunk: u32, depth: usize) -> Self {
        Self {
            fetch,
            pending: FuturesOrdered::new(),
            next: range.start,
            end: range.end,
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
        while self.pending.len() < self.depth && self.next < self.end {
            let left = self.end - self.next;
            let length = u32::try_from(left).map_or(self.chunk, |left| left.min(self.chunk));
            let request = self.request(self.next, length);
            self.pending.push_back(request);
            self.next = self.next.saturating_add(u64::from(length));
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
    use std::assert_matches;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::fixture;

    /// Reads in flight now, the most at once, and where the furthest read asked for ends.
    type Flight = Arc<Mutex<(usize, usize, u64)>>;

    /// A file of `size` bytes whose reads at offsets divisible by 3 give half of what they
    /// ask for, and that fails from `fail_at` on. It counts the reads in flight at once, and
    /// notes how far they reach.
    fn fake(size: usize, fail_at: Option<u64>) -> (Vec<u8>, Fetch, Flight) {
        let data = fixture::pattern(size);
        let source = data.clone();
        let flight = Arc::new(Mutex::new((0, 0, 0)));
        let counter = Arc::clone(&flight);
        let fetch: Fetch = Box::new(move |offset, length| {
            let source = source.clone();
            let counter = Arc::clone(&counter);
            {
                let mut flight = counter.lock().unwrap();
                flight.0 += 1;
                flight.1 = flight.1.max(flight.0);
                flight.2 = flight.2.max(offset + u64::from(length));
            }
            Box::pin(async move {
                tokio::task::yield_now().await;
                counter.lock().unwrap().0 -= 1;
                if fail_at.is_some_and(|at| offset >= at) {
                    return Err(VfsError::PermissionDenied("fake".to_owned(), None));
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
            let mut pipeline = ReadPipeline::new(fetch, 0..u64::MAX, 64, 4);
            assert_eq!(read_all(&mut pipeline).await.unwrap(), data, "{size}");
            assert!(pipeline.read().await.unwrap().is_none(), "ended");
            assert!(flight.lock().unwrap().1 <= 4, "at most four at once");
        }
    }

    #[tokio::test]
    async fn keeps_several_reads_in_flight() {
        let (_, fetch, flight) = fake(10_000, None);
        let mut pipeline = ReadPipeline::new(fetch, 0..u64::MAX, 64, 8);
        read_all(&mut pipeline).await.unwrap();
        assert_eq!(flight.lock().unwrap().1, 8);
    }

    #[tokio::test]
    async fn stops_at_an_error() {
        let (_, fetch, _) = fake(1000, Some(320));
        let mut pipeline = ReadPipeline::new(fetch, 0..u64::MAX, 64, 4);
        let err = read_all(&mut pipeline).await.unwrap_err();
        assert_matches!(err, VfsError::PermissionDenied(..));
        assert!(pipeline.read().await.unwrap().is_none(), "nothing after it");
    }

    #[tokio::test]
    async fn reads_a_range_and_asks_for_nothing_past_it() {
        // Ends on and off the chunks, one within a single chunk, and the end of the file.
        for (start, end) in [
            (0, 1),
            (0, 64),
            (0, 640),
            (5, 70),
            (130, 150),
            (333, 9_999),
            (9_000, 10_007),
        ] {
            let (data, fetch, flight) = fake(10_007, None);
            let mut pipeline = ReadPipeline::new(fetch, start as u64..end as u64, 64, 4);
            assert_eq!(
                read_all(&mut pipeline).await.unwrap(),
                data[start..end],
                "{start}..{end}"
            );
            assert!(pipeline.read().await.unwrap().is_none(), "ended");
            let flight = flight.lock().unwrap();
            assert!(flight.1 <= 4, "at most four at once");
            assert_eq!(flight.2, end as u64, "{start}..{end}: not past it");
        }
    }

    #[tokio::test]
    async fn a_range_past_the_end_of_the_file_stops_there() {
        for (start, end) in [(900, 5000), (0, u64::MAX - 1), (1000, 5000), (4000, 5000)] {
            let (data, fetch, flight) = fake(1000, None);
            let mut pipeline = ReadPipeline::new(fetch, start as u64..end, 64, 4);
            let rest = data.get(start..).unwrap_or_default();
            assert_eq!(read_all(&mut pipeline).await.unwrap(), rest, "{start}");
            assert!(pipeline.read().await.unwrap().is_none(), "ended");
            assert!(flight.lock().unwrap().2 <= end, "{start}..{end}");
        }
    }

    #[tokio::test]
    async fn a_range_at_the_last_offset_does_not_overflow() {
        let (_, fetch, flight) = fake(1000, None);
        let mut pipeline = ReadPipeline::new(fetch, u64::MAX - 10..u64::MAX, 64, 4);
        assert_eq!(read_all(&mut pipeline).await.unwrap(), b"");
        assert_eq!(flight.lock().unwrap().2, u64::MAX);
    }

    #[tokio::test]
    async fn an_empty_range_asks_for_nothing() {
        for start in [0, 500, 5000] {
            let (_, fetch, flight) = fake(1000, None);
            let mut pipeline = ReadPipeline::new(fetch, start..start, 64, 4);
            assert!(pipeline.read().await.unwrap().is_none(), "{start}");
            assert_eq!(*flight.lock().unwrap(), (0, 0, 0), "{start}: no read");
        }
    }
}
