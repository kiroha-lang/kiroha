//! Race-resistant regular-file read and atomic replacement (spec §13.2).
//!
//! Every path component is opened relative to its parent directory handle
//! with `O_NOFOLLOW`, so a symlink substituted after validation is never
//! followed. Replacement writes an exclusive temporary file in the target
//! directory and renames it atomically with `renameat`.

use crate::host::{EffOutcome, EffValue};
use crate::value::{err, Value};
use rustix::fd::OwnedFd;
use rustix::fs::{fstat, openat, renameat, unlinkat, AtFlags, FileType, Mode, OFlags, CWD};
use rustix::io::Errno;
use std::io::{Read, Write};

/// Thread-safe result of blocking file work, converted on the runtime thread.
enum FileRes {
    Bytes(Vec<u8>),
    Done,
    Io(&'static str, &'static str, &'static str),
    Resource,
}

fn io(op: &'static str, reason: &'static str, outcome: &'static str) -> FileRes {
    FileRes::Io(op, reason, outcome)
}

fn convert(r: FileRes) -> EffOutcome {
    match r {
        FileRes::Bytes(b) => EffOutcome::Ok(EffValue::Bytes(b)),
        FileRes::Done => EffOutcome::Ok(EffValue::Null),
        FileRes::Resource => EffOutcome::Resource("max_file_bytes".into()),
        FileRes::Io(op, reason, outcome) => EffOutcome::Err(err::make(
            "io",
            format!("{op}: {reason}"),
            vec![("operation", Value::str(op)), ("reason", Value::str(reason)), ("outcome", Value::str(outcome))],
        )),
    }
}

fn reason_of(e: Errno) -> &'static str {
    match e {
        Errno::NOENT => "not_found",
        Errno::LOOP | Errno::MLINK => "symlink",
        Errno::NOTDIR => "not_directory",
        Errno::ACCESS | Errno::PERM => "permission_denied",
        Errno::ISDIR => "not_regular",
        _ => "os_error",
    }
}

/// Open the parent directory of `path` component by component without
/// following symlinks. Returns (dir fd, final component name).
fn open_parent(path: &str) -> Result<(OwnedFd, String), Errno> {
    let comps: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    let (last, dirs) = comps.split_last().ok_or(Errno::ISDIR)?;
    let mut fd = openat(CWD, "/", OFlags::DIRECTORY | OFlags::RDONLY | OFlags::CLOEXEC, Mode::empty())?;
    for d in dirs {
        fd = openat(&fd, *d, OFlags::DIRECTORY | OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty())?;
    }
    Ok((fd, last.to_string()))
}

pub async fn read_regular(path: String, max_bytes: u64) -> EffOutcome {
    convert(tokio::task::spawn_blocking(move || read_blocking(&path, max_bytes)).await.unwrap_or_else(|_| io("read", "os_error", "not_applied")))
}

fn read_blocking(path: &str, max_bytes: u64) -> FileRes {
    let (dir, name) = match open_parent(path) {
        Ok(x) => x,
        Err(e) => return io("read", reason_of(e), "not_applied"),
    };
    let fd = match openat(&dir, name.as_str(), OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK, Mode::empty()) {
        Ok(f) => f,
        Err(e) => return io("read", reason_of(e), "not_applied"),
    };
    let st = match fstat(&fd) {
        Ok(s) => s,
        Err(e) => return io("read", reason_of(e), "not_applied"),
    };
    if FileType::from_raw_mode(st.st_mode) != FileType::RegularFile {
        return io("read", "not_regular", "not_applied");
    }
    if st.st_size as u64 > max_bytes {
        return FileRes::Resource;
    }
    let mut f = std::fs::File::from(fd);
    let mut buf = Vec::new();
    let mut limited = (&mut f).take(max_bytes + 1);
    if limited.read_to_end(&mut buf).is_err() {
        return io("read", "os_error", "not_applied");
    }
    if buf.len() as u64 > max_bytes {
        return FileRes::Resource;
    }
    FileRes::Bytes(buf)
}

pub async fn replace_regular(path: String, bytes: Vec<u8>) -> EffOutcome {
    convert(
        tokio::task::spawn_blocking(move || replace_blocking(&path, &bytes))
            .await
            .unwrap_or_else(|_| io("write", "os_error", "unknown")),
    )
}

fn temp_name(name: &str) -> String {
    let mut r = [0u8; 8];
    let _ = getrandom::getrandom(&mut r);
    let hex: String = r.iter().map(|b| format!("{b:02x}")).collect();
    format!(".{name}.kiri-{hex}.tmp")
}

fn replace_blocking(path: &str, bytes: &[u8]) -> FileRes {
    let (dir, name) = match open_parent(path) {
        Ok(x) => x,
        Err(e) => return io("write", reason_of(e), "not_applied"),
    };
    // 1. inspect the existing target without following symlinks
    let mode = match rustix::fs::statat(&dir, name.as_str(), AtFlags::SYMLINK_NOFOLLOW) {
        Ok(st) => {
            if FileType::from_raw_mode(st.st_mode) != FileType::RegularFile {
                let reason = if FileType::from_raw_mode(st.st_mode) == FileType::Symlink { "symlink" } else { "not_regular" };
                return io("write", reason, "not_applied");
            }
            st.st_mode & 0o777
        }
        Err(Errno::NOENT) => 0o600,
        Err(e) => return io("write", reason_of(e), "not_applied"),
    };
    // 2. exclusive unpredictable temporary file
    let tmp = temp_name(&name);
    let fd = match openat(
        &dir,
        tmp.as_str(),
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    ) {
        Ok(f) => f,
        Err(e) => return io("write", reason_of(e), "not_applied"),
    };
    let cleanup = |dir: &OwnedFd| {
        let _ = unlinkat(dir, tmp.as_str(), AtFlags::empty());
    };
    // 3. write, set mode, close
    let mut f = std::fs::File::from(fd);
    if f.write_all(bytes).is_err() || f.flush().is_err() {
        cleanup(&dir);
        return io("write", "os_error", "not_applied");
    }
    if rustix::fs::fchmod(&f, Mode::from_raw_mode(mode)).is_err() {
        cleanup(&dir);
        return io("write", "os_error", "not_applied");
    }
    drop(f);
    // 4. atomic replacement
    match renameat(&dir, tmp.as_str(), &dir, name.as_str()) {
        Ok(()) => FileRes::Done,
        Err(e) => {
            cleanup(&dir);
            io("write", reason_of(e), "not_applied")
        }
    }
}
