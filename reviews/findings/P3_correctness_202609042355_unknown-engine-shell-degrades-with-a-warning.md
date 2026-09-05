---
id: SWEEP-CONFIG-PARSE-012
severity: P3
disposition: accepted-risk     # a recorded product choice: the parent's suite pins the soft reading, the warning names the key and value, and pre-flight checks the shell that will run exists
category: correctness
pr: 150
reviewed_sha: 425ad55b9703ed58542ee322e6a266d7501bdd93
location: src/config/parse.rs:395
provenance: pre_existing
first_bad:
guard: the owner, on a `design/17` ruling; a later pass that labels this P1 or P2 escalates to the owner rather than re-deferring, since flipping it is a configuration behaviour change and a one-line edit in `parse_engine`
---

## Failure sequence

`[engine]` contains `shell = "powershel"` on a Windows operator's machine -> `parse_engine`
warns `unknown [engine] shell \`powershel\` ... (using the platform default; known: cmd, sh,
bash, powershell, pwsh)` and takes `ShellKind::native()`, which is `cmd` -> a gate written
in PowerShell syntax fails under `cmd` with a syntax error -> the failure is reported as the
gate's, the worker is told the gate failed, and the ladder spends attempts on code the gate
would have passed.

This is the one value in `src/config/parse.rs` that degrades rather than refuses. Every other
enumerated value there (`kind`, `on_task_failure`, `mode`, an effort) errors on a
misspelling. The module's rule distinguishes a *silent* deletion (error) from a degradation
that is *named* (warning), and this one is named -- the operator sees the warning at
`validate` and at run start -- and `gates::shell_available` verifies at pre-flight that the
shell actually chosen exists. The parent's suite pins the soft reading as deliberate in two
tests (`blank_gate_fields_and_unknown_shell_are_handled`,
`the_new_engine_limits_sit_beside_the_keys_that_already_worked`: "the shell warning must
still be the soft one while `on_task_failure` stays hard").

## What the change that takes this up should do

Nothing, unless the owner rules the other way. If they do: make the arm an error in
`parse_engine` naming the value and the known shells, update the two tests above, and add
the sentence to `design/17`'s validation paragraph (this pull request added the paragraph,
and it currently records the warning).
