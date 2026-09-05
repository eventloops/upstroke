# `src/error.rs`

Extended notes for [`src/error.rs`](../../src/error.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## `pub struct ValidationErrors(pub Vec<String>);`

Structural problems found in a parsed plan, collected so a single run
surfaces every issue at once instead of failing on the first.

## `pub enum UpstrokeError` › `Filesystem {`

A filesystem operation on a path the engine owns failed. Named for
the operation, because a removal, a write or a rename that fails did
not fail to read (§7's operation-context rule); `Io` stays the
variant for reads.

## `pub enum UpstrokeError` › `Resume { run_id: String, message: String },`

A resume precondition failed (§15). Always carries what to do about it:
refusing to continue is only useful if the operator can tell which of
the four things moved — the run, the plan, the config, or the branch.

## `pub enum UpstrokeError` › `Refused { message: String },`

A request we could not act on — an id that matches nothing or too many
things, a question already answered, an option that does not exist.
Carries its own whole sentence, because prefixing these with a
command's name (`cannot resume …` on a `status` lookup) misdescribes
what the operator was actually doing.
