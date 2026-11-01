//! Reading a client's lines with a length cap.
//!
//! Tokio's own `lines()` grows its buffer without limit, so one client
//! sending bytes without a newline could make Core allocate until it dies.
//! This reader stops at a cap instead.

use std::io;

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

/// Why a line could not be read.
#[derive(Debug)]
pub enum LineError {
    /// The line grew past the cap before its newline arrived.
    TooLong,
    /// The line was not UTF-8.
    NotUtf8,
    Io(io::Error),
}

/// Reads newline-terminated lines of at most `max` bytes.
pub struct CappedLines<R> {
    reader: BufReader<R>,
    max: usize,
    /// The line read so far. A field rather than a local so that a call
    /// cancelled mid-line (in `tokio::select!`) loses nothing: the next call
    /// carries on where it stopped.
    partial: Vec<u8>,
}

impl<R: AsyncRead + Unpin> CappedLines<R> {
    pub fn new(reader: R, max: usize) -> Self {
        CappedLines {
            reader: BufReader::new(reader),
            max,
            partial: Vec::new(),
        }
    }

    /// The next line without its `\n` or `\r\n`, or `None` at end of input.
    /// A final line without a newline still counts as a line.
    pub async fn next_line(&mut self) -> Result<Option<String>, LineError> {
        loop {
            let available = self.reader.fill_buf().await.map_err(LineError::Io)?;
            if available.is_empty() {
                if self.partial.is_empty() {
                    return Ok(None);
                }
                return self.finish();
            }
            let (chunk, found_newline) = match available.iter().position(|&b| b == b'\n') {
                Some(at) => (&available[..at], true),
                None => (available, false),
            };
            if self.partial.len() + chunk.len() > self.max {
                return Err(LineError::TooLong);
            }
            self.partial.extend_from_slice(chunk);
            // Consume the newline too, when there was one.
            let used = chunk.len() + usize::from(found_newline);
            self.reader.consume(used);
            if found_newline {
                return self.finish();
            }
        }
    }

    fn finish(&mut self) -> Result<Option<String>, LineError> {
        let mut bytes = std::mem::take(&mut self.partial);
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| LineError::NotUtf8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn read_all(input: &[u8], max: usize) -> Vec<Result<String, String>> {
        let mut lines = CappedLines::new(input, max);
        let mut out = Vec::new();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => out.push(Ok(line)),
                Ok(None) => return out,
                Err(e) => {
                    out.push(Err(format!("{e:?}")));
                    return out;
                }
            }
        }
    }

    #[tokio::test]
    async fn splits_lines_and_strips_terminators() {
        let got = read_all(b"one\ntwo\r\n\nlast", 100).await;
        let want: Vec<Result<String, String>> = vec![
            Ok("one".into()),
            Ok("two".into()),
            Ok("".into()),
            Ok("last".into()),
        ];
        assert_eq!(got, want);
    }

    #[tokio::test]
    async fn a_line_at_the_cap_passes_and_one_over_fails() {
        assert_eq!(read_all(b"12345\n", 5).await, vec![Ok("12345".into())]);
        assert_eq!(read_all(b"123456\n", 5).await, vec![Err("TooLong".into())]);
        // Without a newline at all, the cap still applies.
        let endless = vec![b'x'; 10_000];
        assert_eq!(read_all(&endless, 100).await, vec![Err("TooLong".into())]);
    }

    #[tokio::test]
    async fn invalid_utf8_is_reported() {
        assert_eq!(
            read_all(b"ok\n\xff\xfe\n", 100).await,
            vec![Ok("ok".into()), Err("NotUtf8".into())]
        );
    }
}
