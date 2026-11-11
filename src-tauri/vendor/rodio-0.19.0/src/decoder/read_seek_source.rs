use std::io::{Read, Result, Seek, SeekFrom};

use symphonia::core::io::MediaSource;

pub struct ReadSeekSource<T: Read + Seek + Send + Sync> {
    inner: T,
    byte_len: Option<u64>,
}

impl<T: Read + Seek + Send + Sync> ReadSeekSource<T> {
    /// Instantiates a new `ReadSeekSource<T>` by taking ownership and wrapping the provided
    /// `Read + Seek`er, computing `byte_len` immediately so Symphonia's `isomp4` (M4A/MP4)
    /// demuxer can locate top-level atoms (`ftyp`, `moov`, `mdat`) and seek accurately.
    pub fn new(mut inner: T) -> Self {
        let byte_len = if let Ok(current_pos) = inner.stream_position() {
            let end_pos = inner.seek(SeekFrom::End(0)).ok();
            let _ = inner.seek(SeekFrom::Start(current_pos));
            end_pos
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
