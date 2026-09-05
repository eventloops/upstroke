//! Extended notes: `docs/internals/effects/tests/policy.md`

#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

pub(super) fn marker_before(source: &str, line: usize, inner: bool) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let start = if inner { 0 } else { line.saturating_sub(13) };
    lines[start..line.min(lines.len())].join("\n")
}

pub(super) const PACKET_PRIMITIVES: &[&str] = &[
    "std::fs::write",
    "std::fs::remove_file",
    "std::fs::remove_dir",
    "std::fs::remove_dir_all",
    "std::fs::rename",
    "std::fs::copy",
    "std::fs::hard_link",
    "std::fs::set_permissions",
    "std::fs::create_dir",
    "std::fs::create_dir_all",
    "std::fs::File::create",
    "std::fs::File::create_new",
    "std::fs::File::options",
    "std::fs::File::set_len",
    "std::fs::File::sync_data",
    "std::fs::File::sync_all",
    "std::io::Write::write_all",
    "std::io::Write::flush",
    "std::os::unix::fs::symlink",
    "std::os::windows::fs::symlink_file",
    "std::os::windows::fs::symlink_dir",
    "std::process::Command::spawn",
    "std::process::Command::output",
    "std::process::Command::status",
    "libc::fork",
    "libc::kill",
    "libc::setpgid",
    "libc::setsid",
    "libc::flock",
    "libc::fcntl",
    "libc::execv",
    "libc::execve",
    "libc::execvp",
    "windows_sys::Win32::Storage::FileSystem::LockFileEx",
    "windows_sys::Win32::Storage::FileSystem::UnlockFileEx",
    "upstroke::util::write_json",
];

pub(super) const PACKET_TYPES: &[&str] = &[
    "std::fs::DirBuilder",
    "std::fs::OpenOptions",
    "std::process::Command",
];

pub(super) fn host_conditional_paths() -> Vec<&'static str> {
    if cfg!(windows) {
        vec!["std::os::unix::fs::symlink"]
    } else if cfg!(target_os = "macos") {
        vec![
            "std::os::windows::fs::symlink_dir",
            "std::os::windows::fs::symlink_file",
            "libc::pipe2",
        ]
    } else {
        vec![
            "std::os::windows::fs::symlink_dir",
            "std::os::windows::fs::symlink_file",
        ]
    }
}
