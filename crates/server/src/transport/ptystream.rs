//! Async adapter for a raw PTY file descriptor.
//!
//! `tokio-serial`'s `open_native_async` calls baud-rate/termios ioctls that
//! Darwin's PTY devices reject with `ENOTTY`. This adapter bypasses all of
//! that: it wraps an `OwnedFd` set to non-blocking mode and exposes the
//! standard [`AsyncRead`] + [`AsyncWrite`] traits on top of tokio's
//! `AsyncFd`. Bytes flow byte-transparent, which is exactly what Modbus RTU
//! needs.
//!
//! Unix-only — Windows has no PTY concept and this module is cfg-gated out.

#![allow(unsafe_code)]

use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd};
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::unix::AsyncFd;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Put a PTY endpoint into raw mode + non-blocking so that binary data is
/// not line-buffered or cooked.
pub fn prepare_raw(fd: &OwnedFd) -> io::Result<()> {
    let raw = fd.as_raw_fd();

    // Non-blocking: required so reads return WouldBlock and we can yield.
    let flags = unsafe { libc::fcntl(raw, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    let rc = unsafe { libc::fcntl(raw, libc::F_SETFL, flags | libc::O_NONBLOCK) };
    if rc < 0 {
        return Err(io::Error::last_os_error());
    }

    // Raw termios so binary bytes pass through unmolested.
    let borrow = unsafe { BorrowedFd::borrow_raw(raw) };
    if let Ok(mut t) = nix::sys::termios::tcgetattr(borrow) {
        nix::sys::termios::cfmakeraw(&mut t);
        let _ = nix::sys::termios::tcsetattr(borrow, nix::sys::termios::SetArg::TCSANOW, &t);
    }
    Ok(())
}

pub struct PtyStream {
    inner: AsyncFd<OwnedFd>,
}

impl PtyStream {
    pub fn new(fd: OwnedFd) -> io::Result<Self> {
        prepare_raw(&fd)?;
        Ok(Self {
            inner: AsyncFd::new(fd)?,
        })
    }
}

impl AsyncRead for PtyStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            let mut guard = match self.inner.poll_read_ready(cx) {
                Poll::Ready(Ok(g)) => g,
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            };
            let fd = guard.get_inner().as_raw_fd();
            let n = unsafe {
                libc::read(
                    fd,
                    buf.initialize_unfilled().as_mut_ptr().cast(),
                    buf.remaining(),
                )
            };
            if n < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::WouldBlock {
                    guard.clear_ready();
                    continue;
                }
                return Poll::Ready(Err(e));
            }
            let n = n as usize;
            buf.advance(n);
            return Poll::Ready(Ok(()));
        }
    }
}

impl AsyncWrite for PtyStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        loop {
            let mut guard = match self.inner.poll_write_ready(cx) {
                Poll::Ready(Ok(g)) => g,
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            };
            let fd = guard.get_inner().as_raw_fd();
            let n = unsafe { libc::write(fd, buf.as_ptr().cast(), buf.len()) };
            if n < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::WouldBlock {
                    guard.clear_ready();
                    continue;
                }
                return Poll::Ready(Err(e));
            }
            return Poll::Ready(Ok(n as usize));
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
