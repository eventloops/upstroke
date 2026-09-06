//! Extended notes: `docs/internals/agent/proc/pipe_io.md`

// Allowlist placement: the funnel section of `effects/allowlist.toml`
// registers native pipe creation, mode-setting and I/O in this module.

#![allow(clippy::disallowed_methods, clippy::disallowed_types)]
#![deny(clippy::disallowed_macros)]

use std::io;
use std::process::{Child, Command, Stdio};
use thiserror::Error;

pub(super) trait PollRead: Send + 'static {
    fn try_read(&mut self, bytes: &mut [u8]) -> io::Result<usize>;
}

pub(super) trait PollWrite: Send + 'static {
    fn try_write(&mut self, bytes: &[u8]) -> io::Result<usize>;
}

#[derive(Debug, Error)]
pub(super) enum SetupError {
    #[cfg(unix)]
    #[error("configured child {stream} pipe is absent")]
    Missing { stream: &'static str },
    #[error("{operation} for agent {stream} pipe: {source}")]
    Native {
        stream: &'static str,
        operation: &'static str,
        #[source]
        source: io::Error,
    },
}

impl SetupError {
    fn native(stream: &'static str, operation: &'static str) -> Self {
        Self::Native {
            stream,
            operation,
            source: io::Error::last_os_error(),
        }
    }
}

pub(super) struct Endpoints {
    pub(super) stdin: Writer,
    pub(super) stdout: Reader,
    pub(super) stderr: Reader,
}

#[cfg(unix)]
pub(super) struct Reader(std::fs::File);
#[cfg(unix)]
pub(super) struct Writer(std::fs::File);
#[cfg(windows)]
pub(super) struct Reader(std::os::windows::io::OwnedHandle);
#[cfg(windows)]
pub(super) struct Writer(std::os::windows::io::OwnedHandle);

pub(super) struct Prepared {
    #[cfg(windows)]
    endpoints: Endpoints,
}

impl Prepared {
    pub(super) fn configure(command: &mut Command) -> Result<Self, SetupError> {
        #[cfg(unix)]
        {
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            Ok(Self {})
        }
        #[cfg(windows)]
        {
            let (stdin, child_stdin) = windows::pair("stdin", false)?;
            let (stdout, child_stdout) = windows::pair("stdout", true)?;
            let (stderr, child_stderr) = windows::pair("stderr", true)?;
            command
                .stdin(Stdio::from(child_stdin))
                .stdout(Stdio::from(child_stdout))
                .stderr(Stdio::from(child_stderr));
            Ok(Self {
                endpoints: Endpoints {
                    stdin: Writer(stdin),
                    stdout: Reader(stdout),
                    stderr: Reader(stderr),
                },
            })
        }
    }

    pub(super) fn take(self, child: &mut Child) -> Result<Endpoints, SetupError> {
        #[cfg(unix)]
        {
            let stdin = child
                .stdin
                .take()
                .ok_or(SetupError::Missing { stream: "stdin" })?;
            let stdout = child
                .stdout
                .take()
                .ok_or(SetupError::Missing { stream: "stdout" })?;
            let stderr = child
                .stderr
                .take()
                .ok_or(SetupError::Missing { stream: "stderr" })?;
            Ok(Endpoints {
                stdin: Writer(unix::nonblocking("stdin", stdin)?),
                stdout: Reader(unix::nonblocking("stdout", stdout)?),
                stderr: Reader(unix::nonblocking("stderr", stderr)?),
            })
        }
        #[cfg(windows)]
        {
            let _ = child;
            Ok(self.endpoints)
        }
    }
}

#[cfg(unix)]
mod unix {
    use super::{PollRead, PollWrite, Reader, SetupError, Writer};
    use std::fs::File;
    use std::io::{self, Read, Write};
    use std::os::fd::{AsRawFd, OwnedFd};

    pub(super) fn nonblocking(
        stream: &'static str,
        pipe: impl Into<OwnedFd>,
    ) -> Result<File, SetupError> {
        let file = File::from(pipe.into());
        // SAFETY: file owns this valid descriptor throughout both calls.
        // F_GETFL takes no third argument and does not transfer ownership.
        let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
        if flags < 0 {
            return Err(SetupError::native(
                stream,
                "reading status flags with F_GETFL",
            ));
        }
        // SAFETY: F_SETFL takes the integer status flags. Preserve every
        // existing flag and add nonblocking mode only on this parent endpoint.
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(SetupError::native(
                stream,
                "setting nonblocking mode with F_SETFL",
            ));
        }
        Ok(file)
    }

    impl PollRead for Reader {
        fn try_read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            self.0.read(bytes)
        }
    }

    impl PollWrite for Writer {
        fn try_write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.write(bytes)
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::{PollRead, PollWrite, Reader, SetupError, Writer};
    use std::io;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::ptr;
    use windows_sys::Win32::Foundation::{ERROR_BROKEN_PIPE, ERROR_NO_DATA};
    use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
    use windows_sys::Win32::System::Pipes::{CreatePipe, PIPE_NOWAIT, SetNamedPipeHandleState};

    pub(super) fn pair(
        stream: &'static str,
        parent_reads: bool,
    ) -> Result<(OwnedHandle, OwnedHandle), SetupError> {
        let mut reader = ptr::null_mut();
        let mut writer = ptr::null_mut();
        // SAFETY: the output slots are writable HANDLE storage. Null security
        // attributes make both newly created handles noninheritable here.
        if unsafe { CreatePipe(&mut reader, &mut writer, ptr::null(), 64 * 1024) } == 0 {
            return Err(SetupError::native(
                stream,
                "creating endpoints with CreatePipe",
            ));
        }
        // SAFETY: successful CreatePipe returned distinct owned valid handles.
        // Each enters one OwnedHandle and is closed on every subsequent error.
        let reader = unsafe { OwnedHandle::from_raw_handle(reader) };
        // SAFETY: writer is the other independently owned output handle.
        let writer = unsafe { OwnedHandle::from_raw_handle(writer) };
        let (parent, child) = if parent_reads {
            (reader, writer)
        } else {
            (writer, reader)
        };
        let mode = PIPE_NOWAIT;
        // SAFETY: parent owns a synchronous anonymous pipe. mode is readable
        // u32 storage; optional network buffering arguments are unused.
        if unsafe {
            SetNamedPipeHandleState(parent.as_raw_handle(), &mode, ptr::null(), ptr::null())
        } == 0
        {
            return Err(SetupError::native(
                stream,
                "setting nonblocking mode with SetNamedPipeHandleState",
            ));
        }
        Ok((parent, child))
    }

    fn bounded_len(length: usize) -> u32 {
        u32::try_from(length).unwrap_or(u32::MAX)
    }

    impl PollRead for Reader {
        fn try_read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            if bytes.is_empty() {
                return Ok(0);
            }
            let mut read = 0;
            // SAFETY: this owned handle is synchronous and nonblocking. bytes
            // is writable for the requested prefix, read is valid u32 output,
            // and no asynchronous operation or buffer lifetime is introduced.
            let ok = unsafe {
                ReadFile(
                    self.0.as_raw_handle(),
                    bytes.as_mut_ptr(),
                    bounded_len(bytes.len()),
                    &mut read,
                    ptr::null_mut(),
                )
            };
            if ok != 0 {
                return Ok(read as usize);
            }
            let error = io::Error::last_os_error();
            match error
                .raw_os_error()
                .and_then(|code| u32::try_from(code).ok())
            {
                Some(ERROR_NO_DATA) => Err(io::ErrorKind::WouldBlock.into()),
                Some(ERROR_BROKEN_PIPE) => Ok(0),
                _ => Err(error),
            }
        }
    }

    impl PollWrite for Writer {
        fn try_write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.is_empty() {
                return Ok(0);
            }
            let mut written = 0;
            // SAFETY: this owned handle is synchronous and nonblocking. bytes
            // remains readable for the bounded prefix until WriteFile returns;
            // written is valid output and no OVERLAPPED request is issued.
            // Native CreatePipe/PIPE_NOWAIT evidence showed an oversized
            // request can report zero even while the pipe is empty. Keep each
            // attempt within the 4 KiB size established by the native witness.
            let count = bounded_len(bytes.len().min(4096));
            let ok = unsafe {
                WriteFile(
                    self.0.as_raw_handle(),
                    bytes.as_ptr(),
                    count,
                    &mut written,
                    ptr::null_mut(),
                )
            };
            if ok == 0 {
                let error = io::Error::last_os_error();
                return match error
                    .raw_os_error()
                    .and_then(|code| u32::try_from(code).ok())
                {
                    Some(ERROR_NO_DATA | ERROR_BROKEN_PIPE) => {
                        Err(io::Error::new(io::ErrorKind::BrokenPipe, error))
                    }
                    _ => Err(error),
                };
            }
            if written == 0 {
                return Err(io::ErrorKind::WouldBlock.into());
            }
            Ok(written as usize)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::proc::drain::{Drain, Stream};
    use crate::agent::proc::input::Feeder;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, mpsc};
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn native_setup_diagnostics_keep_the_stream_operation_and_io_cause() {
        let error = SetupError::Native {
            stream: "stderr",
            operation: "setting nonblocking mode",
            source: io::Error::new(io::ErrorKind::PermissionDenied, "mode refused"),
        };
        assert_eq!(
            error.to_string(),
            "setting nonblocking mode for agent stderr pipe: mode refused"
        );
        let cause = std::error::Error::source(&error).expect("native cause retained");
        let cause = cause.downcast_ref::<io::Error>().expect("typed I/O cause");
        assert_eq!(cause.kind(), io::ErrorKind::PermissionDenied);
    }

    #[cfg(unix)]
    fn pair(parent_reads: bool) -> (ReaderOrWriter, std::os::fd::OwnedFd) {
        use std::os::fd::{FromRawFd, OwnedFd};
        let mut descriptors = [-1; 2];
        // SAFETY: the array provides two writable descriptor slots. Success
        // gives each slot an independently owned descriptor.
        assert_eq!(unsafe { libc::pipe(descriptors.as_mut_ptr()) }, 0);
        let [read, write] = descriptors;
        // SAFETY: successful pipe returned this fresh owned read descriptor.
        let read = unsafe { OwnedFd::from_raw_fd(read) };
        // SAFETY: this is the independently owned write descriptor.
        let write = unsafe { OwnedFd::from_raw_fd(write) };
        if parent_reads {
            (
                ReaderOrWriter::Read(Reader(
                    unix::nonblocking("stdout", read).expect("read mode"),
                )),
                write,
            )
        } else {
            (
                ReaderOrWriter::Write(Writer(
                    unix::nonblocking("stdin", write).expect("write mode"),
                )),
                read,
            )
        }
    }

    #[cfg(windows)]
    fn pair(parent_reads: bool) -> (ReaderOrWriter, std::os::windows::io::OwnedHandle) {
        let (parent, peer) =
            windows::pair("test stream", parent_reads).expect("native pipe and parent mode");
        let endpoint = if parent_reads {
            ReaderOrWriter::Read(Reader(parent))
        } else {
            ReaderOrWriter::Write(Writer(parent))
        };
        (endpoint, peer)
    }

    enum ReaderOrWriter {
        Read(Reader),
        Write(Writer),
    }

    struct Observed<T> {
        endpoint: Option<T>,
        pending: mpsc::SyncSender<()>,
        closed: Arc<AtomicBool>,
    }

    impl PollRead for Observed<Reader> {
        fn try_read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            let result = self
                .endpoint
                .as_mut()
                .expect("owned reader")
                .try_read(bytes);
            if matches!(&result, Err(error) if error.kind() == io::ErrorKind::WouldBlock) {
                let _already_reported = self.pending.try_send(());
            }
            result
        }
    }

    impl PollWrite for Observed<Writer> {
        fn try_write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let result = self
                .endpoint
                .as_mut()
                .expect("owned writer")
                .try_write(bytes);
            if matches!(&result, Err(error) if error.kind() == io::ErrorKind::WouldBlock) {
                let _already_reported = self.pending.try_send(());
            }
            result
        }
    }

    impl<T> Drop for Observed<T> {
        fn drop(&mut self) {
            drop(self.endpoint.take());
            self.closed.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    #[ignore = "isolated native I/O witness invoked by its bounded parent"]
    fn native_pipe_cancellation_helper() {
        for collect in [true, false] {
            for parent_reads in [true, false] {
                let (endpoint, peer) = pair(parent_reads);
                let (pending, observed) = mpsc::sync_channel(1);
                let closed = Arc::new(AtomicBool::new(false));
                match endpoint {
                    ReaderOrWriter::Read(reader) => {
                        let drain = Drain::start(
                            Stream::Stdout,
                            Observed {
                                endpoint: Some(reader),
                                pending,
                                closed: Arc::clone(&closed),
                            },
                            32,
                        )
                        .expect("native reader worker");
                        observed
                            .recv_timeout(Duration::from_secs(2))
                            .expect("empty live read returns WouldBlock");
                        let started = Instant::now();
                        if collect {
                            let captured =
                                drain.collect_bytes(Duration::ZERO).expect("release reader");
                            assert!(captured.bytes.is_empty());
                            assert!(!captured.ended, "the peer writer remains open");
                            assert!(!captured.limited);
                        } else {
                            drop(drain);
                        }
                        assert!(
                            started.elapsed() < Duration::from_secs(2),
                            "reader release waited for its peer"
                        );
                    }
                    ReaderOrWriter::Write(mut writer) => {
                        let mut full = false;
                        for _ in 0..2048 {
                            match writer.try_write(&[b'x'; 4096]) {
                                Ok(count) => assert!(count > 0),
                                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                                    full = true;
                                    break;
                                }
                                Err(error) => panic!("filling live pipe: {error}"),
                            }
                        }
                        assert!(full, "fixture pipe exceeded its bounded fill budget");
                        let feeder = Feeder::start(
                            Observed {
                                endpoint: Some(writer),
                                pending,
                                closed: Arc::clone(&closed),
                            },
                            vec![b'y'; 4096],
                        )
                        .expect("native writer worker");
                        observed
                            .recv_timeout(Duration::from_secs(2))
                            .expect("full live write returns WouldBlock");
                        let started = Instant::now();
                        if collect {
                            feeder.collect(Duration::ZERO).expect("release writer");
                        } else {
                            drop(feeder);
                        }
                        assert!(
                            started.elapsed() < Duration::from_secs(2),
                            "writer release waited for its peer"
                        );
                    }
                }
                assert!(
                    closed.load(Ordering::SeqCst),
                    "worker endpoint closed before its owner returns"
                );
                drop(peer);
            }
        }
    }

    #[test]
    fn native_pipe_workers_join_while_the_peer_stays_open_and_inactive() {
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "agent::proc::pipe_io::tests::native_pipe_cancellation_helper",
                "--exact",
                "--ignored",
                "--nocapture",
            ])
            .stdin(Stdio::null())
            .spawn()
            .expect("isolated native pipe witness");
        let deadline = Instant::now() + Duration::from_secs(15);
        let result = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(Some(status)),
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                Ok(None) => break Ok(None),
                Err(error) => break Err(error),
            }
        };
        if !matches!(&result, Ok(Some(_))) {
            let _already_exited = child.kill();
            child.wait().expect("reap isolated native pipe witness");
        }
        assert!(
            matches!(result, Ok(Some(status)) if status.success()),
            "native pipe cancellation failed: {result:?}"
        );
    }
}
