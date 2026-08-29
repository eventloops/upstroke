# 2026-08-27 — non-UTF-8 paths are supported, and already are

**Verdict.** Repository, worktree and store paths are byte strings on Unix and are
**not** required to be UTF-8. This is a supported case, it is already implemented
deliberately, and no engine change follows from this record. `Workspace::git`'s
UTF-8 decode is not a limit on that support: it is the correct reader for the
formats it is used on, every one of which Git emits as ASCII or quotes to ASCII.

The report that prompted this record — "the engine cannot describe a linked
worktree whose path contains non-UTF-8 bytes" — is wrong in its premise. The
engine never lists worktrees. A **test** did, through the one Git format that
prints a raw path.

## What was actually found

Writing hostile-ancestor coverage for [#36], a gate snapshot store was placed
under an ancestor directory named with a `0xff` byte. The snapshot materialised
and reclaimed correctly. `git worktree list --porcelain`, read through
`Workspace::git`, failed with `returned output that is not valid UTF-8`, so the
registration assertions were dropped from
`a_store_under_a_non_utf8_ancestor_still_announces_and_is_reclaimed` and the
inconsistency was filed for this record to settle.

## Measured

Git 2.43.0, Linux, `core.quotePath` unset (default `true`), on a repository
containing a tracked file named `odd-\xff-name.txt` and a linked worktree under a
directory named `anc-\xff-dir`:

| Format | Output | Valid UTF-8 |
|---|---|---|
| `git status --porcelain` | `?? "odd-\377-name.txt"` | **yes** — quoted |
| `git diff --binary --no-ext-diff --no-textconv --no-color` | `diff --git "a/odd-\377-name.txt" …` | **yes** — quoted |
| `git worktree list --porcelain` | `worktree /tmp/…/anc-<0xff>-dir/tree` | **no** — raw |
| `git worktree list --porcelain -z` | raw path, NUL-delimited | **no** — raw |

Also measured: `git worktree add` succeeds under such an ancestor, and the
snapshot store materialises, registers and reclaims correctly there.

Every call site was enumerated at this head rather than sampled:

- **`git worktree list --porcelain` has no production caller.** All seven
  occurrences are in `src/workspace.rs`'s test module. The one raw-path read in
  `src/engine.rs` (`rev-parse --git-common-dir`) is inside its `#[cfg(test)]`
  fake adapter.
- The engine has **three** readers, and the split is deliberate:
  - `git_path` → `PathBuf`, byte-exact on Unix via `OsString::from_vec`. Its
    doc comment already states the commitment in as many words: *decode one path
    printed by Git without requiring Unix path bytes to be UTF-8*.
  - `git_output` → `Vec<u8>`, for listings parsed as bytes: `ls-tree -r -z`,
    `ls-files -t -z`, `status --porcelain=v1`.
  - `git` → `String`, used only for `rev-parse`, `cat-file -t`,
    `check-ref-format`, `log -1 --format=%s`, `status --porcelain`, `diff`, and
    the mutating commands. Object ids, refs and object types are ASCII by
    construction; the two path-bearing formats are quoted by `core.quotePath`,
    as measured above.

So `gate_snapshot_accepts_non_utf8_tmpdir_on_linux` is not inconsistent with the
rest of the engine. It is consistent with `git_path`, and both express the same
supported case.

## Assumed, not measured

- That `core.quotePath` is not turned off in a user's environment. If it were,
  `status --porcelain` and `diff` would emit raw path bytes and the two String
  call sites would fail on a non-UTF-8 path. This is not defended against today.
  It is recorded here as a known edge rather than fixed, because the failure is a
  loud typed error naming the command, not silent corruption, and no reported
  case exists. Revisit if one does.
- That Windows paths reaching `git_path` are convertible to UTF-8. `git_path`
  requires that on Windows and errors otherwise; an unpaired surrogate in a
  Windows path would fail there. Out of scope for this record, which is about
  Unix path bytes.

## Options rejected

**(a) Read Git output as bytes and use `--porcelain -z` where a listing is
parsed.** Rejected on two counts. The `-z` half is factually wrong: `-z` changes
the *delimiter*, not the *encoding*, and `git worktree list --porcelain -z` still
emits raw non-UTF-8 bytes (measured above). Reading as bytes is the real fix — and
the engine already does exactly that everywhere it parses a path listing. There is
no production code left for this option to change.

**(b) Declare non-UTF-8 repository paths unsupported and make
`gate_snapshot_accepts_non_utf8_tmpdir_on_linux` consistent with that.** Rejected:
it would withdraw a capability that is implemented, tested and working, on the
strength of a test-only oracle failing. `git_path` exists for this case and says
so; deleting the test would make the code and the documented intent disagree in
the more damaging direction.

**(c) Accept the split as-is and document it.** Rejected as framed, because there
is no split in the engine to accept. The documentation half is adopted — the
contract was real but unwritten, which is why a plausible-sounding inconsistency
survived long enough to need a record.

## Consequences

1. `DESIGN.md` §6 gains the path-encoding contract, citing this record. It was
   previously true but unstated, which is the whole reason this took a record to
   settle.
2. No production code changes.
3. `Workspace::git` is for output Git guarantees to be ASCII. A future caller that
   needs a path must use `git_path`, or `git_output` and parse bytes.
   `git worktree list --porcelain` is the one format that prints a raw path, in
   its `-z` form too, and must never be read through `Workspace::git`.
4. **Follow-up, after [#36] merges:** restore the registration assertions to
   `a_store_under_a_non_utf8_ancestor_still_announces_and_is_reclaimed` by reading
   the worktree listing through `git_output` and comparing bytes. The test
   currently stops short of them and says why; that comment should then be
   replaced rather than left to rot. Not done here, because #36 is scoped to test
   synchronisation and the test does not exist on `master` yet.

[#36]: https://github.com/eventloops/upstroke/pull/36
