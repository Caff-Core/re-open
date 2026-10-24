//! Agent socket framing: a four-byte little-endian length, then the UTF-8 JSON
//! envelope (docs/PROTOCOL.md, "Framing and limits").

use reopen_protocol::MAX_MESSAGE_BYTES;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};

#[derive(Debug)]
pub enum FrameError {
    /// The declared length exceeds the limit; nothing was allocated for it.
    TooLarge(usize),
    Io(std::io::Error),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge(len) => {
                write!(f, "frame of {len} bytes exceeds {MAX_MESSAGE_BYTES} bytes")
            }
            Self::Io(error) => write!(f, "socket error: {}", error.kind()),
        }
    }
}

impl std::error::Error for FrameError {}

/// Reads one frame. `Ok(None)` means the peer closed the connection cleanly.
///
/// # Errors
///
/// [`FrameError::TooLarge`] before allocating an oversized frame; I/O errors.
pub async fn read_frame<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<Option<Vec<u8>>, FrameError> {
    let mut header = [0_u8; 4];
    match reader.read_exact(&mut header).await {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(FrameError::Io(error)),
    }
    let len = usize::try_from(u32::from_le_bytes(header)).unwrap_or(usize::MAX);
    if len > MAX_MESSAGE_BYTES {
        return Err(FrameError::TooLarge(len));
    }
    let mut body = vec![0_u8; len];
    reader.read_exact(&mut body).await.map_err(FrameError::Io)?;
    Ok(Some(body))
}

/// Writes one frame.
///
/// # Errors
///
/// [`FrameError::TooLarge`] for an oversized body; I/O errors.
pub async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    body: &[u8],
) -> Result<(), FrameError> {
    if body.len() > MAX_MESSAGE_BYTES {
        return Err(FrameError::TooLarge(body.len()));
    }
    let len = u32::try_from(body.len()).map_err(|_| FrameError::TooLarge(body.len()))?;
    writer
        .write_all(&len.to_le_bytes())
        .await
        .map_err(FrameError::Io)?;
    writer.write_all(body).await.map_err(FrameError::Io)?;
    writer.flush().await.map_err(FrameError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn round_trip_and_clean_eof() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        write_frame(&mut a, b"{}").await.unwrap();
        drop(a);
        assert_eq!(read_frame(&mut b).await.unwrap(), Some(b"{}".to_vec()));
        assert_eq!(read_frame(&mut b).await.unwrap(), None);
    }

    #[tokio::test]
    async fn oversized_lengths_are_rejected_from_the_header_alone() {
        let (mut a, mut b) = tokio::io::duplex(64);
        let declared = u32::try_from(MAX_MESSAGE_BYTES + 1).unwrap();
        a.write_all(&declared.to_le_bytes()).await.unwrap();
        assert!(matches!(
            read_frame(&mut b).await,
            Err(FrameError::TooLarge(len)) if len == MAX_MESSAGE_BYTES + 1
        ));
        // u32::MAX is refused too, without allocating 4 GiB.
        a.write_all(&u32::MAX.to_le_bytes()).await.unwrap();
        assert!(matches!(
            read_frame(&mut b).await,
            Err(FrameError::TooLarge(_))
        ));
    }
}
