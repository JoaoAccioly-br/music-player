use std::io::{Read, Result, Seek, SeekFrom};

use symphonia::core::io::MediaSource;

/// Sentinel returned by `ProgressiveReader::seek(SeekFrom::End(0))` when a progressive stream
/// is still in flight and does not have an upfront total byte length (e.g. non-DASH container).
/// `ReadSeekSource::new()` maps this sentinel to `byte_len = None` so Symphonia never seeks
/// to a fake EOF offset or blocks waiting for the entire network stream before probing headers.
const UNKNOWN_PROGRESSIVE_END_SENTINEL: u64 = u64::MAX / 4;

pub struct ReadSeekSource<T: Read + Seek + Send + Sync> {
    inner: T,
    byte_len: Option<u64>,
}

impl<T: Read + Seek + Send + Sync> ReadSeekSource<T> {
    /// Instantiates a new `ReadSeekSource<T>` by taking ownership and wrapping the provided
    /// `Read + Seek`er, computing `byte_len` immediately when known so Symphonia's `isomp4`
    /// (M4A/MP4) demuxer can locate top-level atoms (`ftyp`, `moov`, `mdat`) and seek accurately.
    pub fn new(mut inner: T) -> Self {
        let byte_len = if let Ok(current_pos) = inner.stream_position() {
            let end_pos = inner.seek(SeekFrom::End(0)).ok();
            let _ = inner.seek(SeekFrom::Start(current_pos));
            // When streaming progressively without a known total length, `ProgressiveReader`
            // returns `UNKNOWN_PROGRESSIVE_END_SENTINEL` (`u64::MAX / 4`) for `SeekFrom::End(0)`.
            // Filter that out so `byte_len()` returns `None` and Symphonia never seeks to a fake EOF offset!
            end_pos.filter(|&len| len < UNKNOWN_PROGRESSIVE_END_SENTINEL)
        } else {
            None
        };
        ReadSeekSource { inner, byte_len }
    }
}

impl<T: Read + Seek + Send + Sync> MediaSource for ReadSeekSource<T> {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        self.byte_len
    }
}

impl<T: Read + Seek + Send + Sync> Read for ReadSeekSource<T> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        self.inner.read(buf)
    }
}

impl<T: Read + Seek + Send + Sync> Seek for ReadSeekSource<T> {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        self.inner.seek(pos)
    }
}
