---
id: PR162-ASTRA-PAGE-SIZE
severity: P2
disposition: deferred
category: portability
pr: 162
reviewed_sha: a408608703fa34ea4e5de857bc20dd76626ac9b6
location: src/runner/host/tests.rs:4170
provenance: fix_regression
first_bad: b6a50741d3d9ef6809effe3a1f6e901cc7f2c01e
guard: marker_shims_do_not_leave_writers_in_another_threads_fork
---

# The inherited-writer fixture assumes 4 KiB Linux pages

Owner-authorized deferred under STACK_STOP_RULE.md. The lane steward explicitly classified this page-size-only fixture rejection as nonblocking in policy-only message 4b84ad6c-5a00-4ba3-92ee-2b43a0b00e40. No product regression or current required-CI failure was demonstrated.

## Failure sequence

Run the Linux helper on a kernel with 64 KiB pages. F_SETPIPE_SZ rounds the 4096-byte request up to the page size and returns 65536. The exact-equality assertion fails before the ownership witness runs. Simply removing the assertion would also leave the fixed 16 KiB script too small to guarantee a blocked writer.

This is a source-derived failure under the documented syscall contract, not a native 64 KiB-page reproduction. The review machine has 4096-byte pages and its complete baseline passed. Linux documents rounding below PAGE_SIZE and returning the actual capacity at [Linux F_SETPIPE_SZ documentation](https://man7.org/linux/man-pages/man2/F_GETPIPE_SZ.2const.html). The supported 64 KiB AArch64 Linux configuration is documented at [AArch64 Linux memory documentation](https://www.kernel.org/doc/html/latest/arch/arm64/memory.html).

## What the change that takes this up should do

Use the returned positive pipe capacity to size a bounded payload larger than the pipe, and retain the holder-alive and EOF assertions. Exercise the fixture on a Linux system with a larger page size.
