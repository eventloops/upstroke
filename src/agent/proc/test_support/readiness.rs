//! Extended notes: `docs/internals/agent/proc/test_support/readiness.md`

#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(10);

const TERMINATOR: char = '\n';

const STAGING: &str = ".publishing";

#[derive(Debug)]
pub(crate) enum Waited {
    Ready(Vec<String>),
    ProducerGone(String),
    TimedOut(Duration),
    Torn(String),
}

impl Waited {
    pub(crate) fn or_fail(self, what: &str) -> Vec<String> {
        match self {
            Self::Ready(fields) => fields,
            Self::ProducerGone(why) => {
                panic!("{what}: the producer will never publish it ({why})")
            }
            Self::TimedOut(bound) => panic!(
                "{what}: the producer is still alive and had published nothing after \
                 {bound:?}"
            ),
            Self::Torn(why) => panic!("{what}: {why}"),
        }
    }
}

fn staging_for(signal: &Path) -> PathBuf {
    let name = signal
        .file_name()
        .expect("a readiness signal is a file, so it has a file name");
    let mut staged = name.to_os_string();
    staged.push(format!(
        ".{}.{}{STAGING}",
        std::process::id(),
        crate::ulid::ulid()
    ));
    signal.with_file_name(staged)
}

pub(crate) fn publish(signal: &Path, fields: &[&str]) -> std::io::Result<()> {
    publish_between(signal, fields, &mut || {})
}

pub(crate) fn publish_between(
    signal: &Path,
    fields: &[&str],
    between: &mut dyn FnMut(),
) -> std::io::Result<()> {
    let mut record = String::new();
    for field in fields {
        if field.contains('\n') || field.contains('\r') {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "a readiness field carries the framing's own delimiter and would \
                     arrive as more than one record: {field:?}"
                ),
            ));
        }
        record.push_str(field);
        record.push(TERMINATOR);
    }
    let staged = staging_for(signal);
    #[expect(
        clippy::disallowed_methods,
        reason = "UPSTROKE-EFFECT: site 1 of 6. Brings the staging sibling into \
                  existence exclusively, which is what makes every removal below \
                  a removal of a file this call owns"
    )]
    let mut file = std::fs::File::create_new(&staged)?;
    #[expect(
        clippy::disallowed_methods,
        reason = "UPSTROKE-EFFECT: site 2 of 6. Writes the framed record into the \
                  staging name, never into the signal name"
    )]
    let written = file.write_all(record.as_bytes());
    #[expect(
        clippy::disallowed_methods,
        reason = "UPSTROKE-EFFECT: site 3 of 6. Flushes before the rename, so the \
                  bytes are in the file the rename publishes"
    )]
    let staged_write = written.and_then(|()| file.flush());
    drop(file);
    if let Err(error) = staged_write {
        #[expect(
            clippy::disallowed_methods,
            reason = "UPSTROKE-EFFECT: site 4 of 6. Removes the staging name on the \
                      write-side failure path; the signal name was never created"
        )]
        let _ = std::fs::remove_file(&staged);
        return Err(error);
    }
    between();
    #[expect(
        clippy::disallowed_methods,
        reason = "UPSTROKE-EFFECT: site 5 of 6. The rename that makes the name and \
                  the bytes visible together — §12's atomic publication"
    )]
    let renamed = std::fs::rename(&staged, signal);
    if let Err(error) = renamed {
        #[expect(
            clippy::disallowed_methods,
            reason = "UPSTROKE-EFFECT: site 6 of 6. Removes the staging name on the \
                      rename-side failure path; this is the second of the two \
                      removals, and the reason the six sites are five paths"
        )]
        let _ = std::fs::remove_file(&staged);
        return Err(error);
    }
    Ok(())
}

pub(crate) fn publish_marker(signal: &Path) -> std::io::Result<()> {
    publish(signal, &[])
}

pub(crate) fn read_published(signal: &Path) -> std::io::Result<Vec<String>> {
    let text = std::fs::read_to_string(signal)?;
    if text.is_empty() {
        return Ok(Vec::new());
    }
    if !text.ends_with(TERMINATOR) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            format!(
                "{} ends without the record terminator, so its last {} byte(s) are a \
                 truncated write rather than a record",
                signal.display(),
                text.len() - text.rfind(TERMINATOR).map_or(0, |at| at + 1)
            ),
        ));
    }
    Ok(text.lines().map(str::to_owned).collect())
}

pub(crate) fn await_signal(signal: &Path, producer: &mut Child, bound: Duration) -> Waited {
    let deadline = Instant::now() + bound;
    loop {
        if signal.exists() {
            return published(signal);
        }
        match producer.try_wait() {
            Ok(Some(status)) => {
                if signal.exists() {
                    return published(signal);
                }
                return Waited::ProducerGone(format!(
                    "it exited {status:?} without publishing {}",
                    signal.display()
                ));
            }
            Ok(None) => {}
            Err(error) => {
                return Waited::ProducerGone(format!("waiting on it failed: {error}"));
            }
        }
        if Instant::now() >= deadline {
            return Waited::TimedOut(bound);
        }
        thread::sleep(POLL);
    }
}

fn published(signal: &Path) -> Waited {
    match read_published(signal) {
        Ok(fields) => Waited::Ready(fields),
        Err(error) => Waited::Torn(error.to_string()),
    }
}

enum Framed {
    Line(String),
    Unterminated(String),
    Eof,
    Flooded(usize),
    Failed(String),
}

fn read_frames(stdout: ChildStdout, framed: &Sender<Framed>) {
    let mut reader = BufReader::new(stdout);
    let mut drained = 0_usize;
    loop {
        let allowance = super::super::OUTPUT_LIMIT_BYTES.saturating_sub(drained);
        if allowance == 0 {
            let _ = framed.send(Framed::Flooded(drained));
            return;
        }
        let mut line = String::new();
        let read = match (&mut reader).take(allowance as u64).read_line(&mut line) {
            Ok(read) => read,
            Err(error) => {
                let _ = framed.send(Framed::Failed(error.to_string()));
                return;
            }
        };
        if read == 0 {
            let _ = framed.send(Framed::Eof);
            return;
        }
        drained = drained.saturating_add(read);
        let complete = line.ends_with(TERMINATOR);
        let message = if complete {
            Framed::Line(line)
        } else if drained >= super::super::OUTPUT_LIMIT_BYTES {
            Framed::Flooded(drained)
        } else {
            Framed::Unterminated(line)
        };
        if framed.send(message).is_err() || !complete {
            return;
        }
    }
}

pub(crate) struct Producer {
    child: Child,
    reader: Option<JoinHandle<()>>,
    framed: Receiver<Framed>,
}

impl Producer {
    pub(crate) fn adopt(mut child: Child) -> Self {
        let (sender, framed) = mpsc::channel();
        let reader = child
            .stdout
            .take()
            .map(|stdout| thread::spawn(move || read_frames(stdout, &sender)));
        Self {
            child,
            reader,
            framed,
        }
    }

    pub(crate) fn child(&mut self) -> &mut Child {
        &mut self.child
    }

    pub(crate) fn alive(&mut self) -> bool {
        self.child
            .try_wait()
            .expect("wait on the readiness producer")
            .is_none()
    }

    pub(crate) fn await_line(&mut self, wanted: &str, bound: Duration) -> Waited {
        let deadline = Instant::now() + bound;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.framed.recv_timeout(remaining) {
                Ok(Framed::Line(line)) => {
                    if line.trim() == wanted {
                        return Waited::Ready(vec![line.trim().to_owned()]);
                    }
                    if Instant::now() >= deadline {
                        return Waited::TimedOut(bound);
                    }
                }
                Ok(Framed::Unterminated(partial)) => {
                    return Waited::Torn(format!(
                        "the producer's channel ended mid-record: {partial:?} is a \
                         truncated write, not a line"
                    ));
                }
                Ok(Framed::Flooded(bytes)) => {
                    return Waited::Torn(format!(
                        "the producer wrote {bytes} byte(s) without ever framing \
                         `{wanted}`, which is the whole per-stream output allowance"
                    ));
                }
                Ok(Framed::Eof) => {
                    return Waited::ProducerGone(format!(
                        "it closed its channel without framing `{wanted}`"
                    ));
                }
                Ok(Framed::Failed(error)) => {
                    return Waited::ProducerGone(format!("reading its channel failed: {error}"));
                }
                Err(RecvTimeoutError::Timeout) => return Waited::TimedOut(bound),
                Err(RecvTimeoutError::Disconnected) => {
                    return Waited::ProducerGone(
                        "the reader ended without reaching a verdict".to_owned(),
                    );
                }
            }
        }
    }
}

impl Drop for Producer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
