# `src/agent/proc/pipe_io.rs`

Extended notes for [`src/agent/proc/pipe_io.rs`](../../../../src/agent/proc/pipe_io.rs).

## Module

Owned, nonblocking parent endpoints inside the Process funnel.

Each endpoint has exactly one worker owner. Polling never waits for the
peer to produce bytes, free pipe capacity or close a handle. WouldBlock
returns control to the worker's cancellation protocol. These traits are
private to proc; an arbitrary blocking Read/Write does not implement them.
Test adapters must obey the same nonblocking operation and teardown rules.

Prepared owns parent endpoints on Windows, while Command owns the opposite
ends. The caller drops Command immediately after successful spawn, before
taking endpoints. Otherwise those parent copies suppress EOF and refusal.
The governed lint allowance is in the funnel section of effects/allowlist.toml.

## `pub(super) trait PollRead: Send + 'static {`

One nonblocking read. A live empty pipe returns WouldBlock; only EOF is zero.

## `try_read` › `pub(super) trait PollWrite: Send + 'static {`

One nonblocking write. A full live pipe returns WouldBlock, never WriteZero.

## `pub(super) struct Endpoints {`

The parent's three endpoints, transferred directly to their workers.

## `pub(super) struct Prepared {`

Configuration retained until a successfully spawned child is registered.

## `pub(super) fn configure(command: &mut Command) -> io::Result<Self> {`

Configure all three streams. Drop Command after spawn, then call take.

### Errors
Native pipe creation or mode-setting failed. Partial pairs close here.

## `configure` › `command`

All pairs exist before Command changes. Each opposite endpoint
moves into Stdio; no extra writer/reader copy is retained here.

## `pub(super) fn take(self, child: &mut Child) -> io::Result<Endpoints> {`

Transfer parent endpoints after successful spawn and Command drop.

### Errors
A configured Unix child pipe is absent or cannot become nonblocking.
The caller settles the registered child on this supervision failure.

## `fn pair(parent_reads: bool) -> (ReaderOrWriter, std::os::fd::OwnedFd) {`

The peer endpoint stays in this isolated helper process. Killing that
process closes every fixture handle, even if a mode-setting mutation
leaves a worker blocked inside native I/O. There is no child peer or
detached timeout thread to clean up.

## `native_pipe_cancellation_helper` › `let closed = Arc::new(AtomicBool::new(false));`

The fixture retains the close observer across worker ownership.

## `native_pipe_cancellation_helper` › `let mut full = false;`

Fill the actual native pipe before starting the
feeder. The live peer never consumes these bytes.

## `native_pipe_workers_join_while_the_peer_stays_open_and_inactive` › `if !matches!(&result, Ok(Some(_))) {`

Only this test owns and waits for this child. A refused poll or
deadline always reaches kill plus wait before any assertion fails.
