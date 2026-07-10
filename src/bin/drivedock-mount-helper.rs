//! DriveDock's privileged mount helper.
//!
//! Invoked exclusively as `pkexec <libexec-path>/drivedock-mount-helper <subcommand>`
//! by the unprivileged GUI process (`crate::network::mount`), per the Phase 1 spec
//! (`.github/docs/subagent_docs/network-mount-privileged_spec.md` §3.4). Root-executed
//! only via Polkit - kept deliberately minimal (no GTK/gio/glib dependency at all) to
//! keep the privileged attack surface small and auditable.
//!
//! Request contract: a single JSON object on stdin (never argv/env - both are
//! visible to other local users via `/proc/<pid>/cmdline`/`environ`; stdin is not).
//! See `Request` below for the exact shape.
//!
//! Subcommands:
//! - `mount`: mkdir -p the mount point, write a 0600 root:root credentials file if
//!   credentials are present, then `mount.cifs`/`mount.nfs` (idempotent: no-op if
//!   already mounted per `/proc/mounts`).
//! - `unmount`: `umount` the mount point (idempotent: no-op if not mounted).
//! - `persist`: idempotent `/etc/fstab` read-modify-write (flock + atomic rename),
//!   appending a `# drivedock:<slug>`-tagged line.
//! - `unpersist`: inverse of `persist`; also deletes the credentials file.
//!
//! All decision logic (slug sanitization, fstab line generation/matching, the
//! `/proc/mounts` idempotency check) is implemented as pure functions, unit-tested
//! below. The privileged I/O itself (mkdir, file writes as root, `flock`, `mount(2)`
//! via `mount.cifs`/`mount.nfs`, atomic `rename()`) cannot be exercised via `cargo
//! test` (needs root + a real mountable target) - this is a documented testing
//! boundary, not a defect (see Phase 1 spec §7 risk 5).

use serde::Deserialize;
use std::ffi::OsStr;
use std::fs;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Hardcoded base directory for network-share mount points. `mount_point` in every
/// request must resolve to a direct child of this directory - never anything else -
/// to prevent an arbitrary-path root write via a compromised/buggy caller.
const MOUNT_BASE: &str = "/mnt";

/// Root-owned candidate directories searched (in order) for `mount.cifs`/`mount.nfs`/
/// `umount`, instead of the caller's inherited `$PATH` (which a local user could
/// influence) - see Phase 1 spec §3.4/§7 risk 2/6.
const BIN_CANDIDATE_DIRS: &[&str] = &["/run/current-system/sw/bin", "/usr/sbin", "/sbin"];

const CREDENTIALS_DIR: &str = "/etc/drivedock/credentials";
const FSTAB_PATH: &str = "/etc/fstab";

#[derive(Debug, Deserialize)]
struct Request {
    share_name: String,
    fs_type: String,
    source: String,
    mount_point: String,
    #[serde(default)]
    options: String,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    domain: Option<String>,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("drivedock-mount-helper: error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let subcommand = std::env::args()
        .nth(1)
        .ok_or_else(|| "usage: drivedock-mount-helper <mount|unmount|persist|unpersist>".to_string())?;

    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| format!("failed to read request from stdin: {e}"))?;
    let request: Request =
        serde_json::from_str(&input).map_err(|e| format!("invalid request JSON: {e}"))?;

    let slug = slugify(&request.share_name);
    if slug.is_empty() {
        return Err("share_name does not contain any valid slug characters".to_string());
    }
    validate_mount_point(&request.mount_point)
        .map_err(|e| format!("invalid mount_point: {e}"))?;

    match subcommand.as_str() {
        "mount" => cmd_mount(&request, &slug),
        "unmount" => cmd_unmount(&request),
        "persist" => cmd_persist(&request, &slug),
        "unpersist" => cmd_unpersist(&request, &slug),
        other => Err(format!(
            "unknown subcommand '{other}' (expected mount|unmount|persist|unpersist)"
        )),
    }
}

// ---------------------------------------------------------------------------
// mount
// ---------------------------------------------------------------------------

fn cmd_mount(request: &Request, slug: &str) -> Result<(), String> {
    let mount_point = Path::new(&request.mount_point);

    // `mount_point` is string-validated by `validate_mount_point` (direct child of
    // `/mnt`, no `..`), not canonicalized before this `create_dir_all`. This is safe
    // under the current threat model - the caller is the unprivileged DriveDock GUI
    // (compromised/buggy, not an arbitrary local user), and `/mnt` is root-owned
    // `0755` by default, so no unprivileged process can plant a symlink there to
    // redirect this mkdir. This assumption would need revisiting if `/mnt` permissions
    // are ever loosened.
    fs::create_dir_all(mount_point)
        .map_err(|e| format!("failed to create mount point {}: {e}", mount_point.display()))?;

    let mut cred_path: Option<PathBuf> = None;
    if request.username.is_some() || request.password.is_some() {
        cred_path = Some(write_credentials_file(request, slug)?);
    }

    let mounts = fs::read_to_string("/proc/mounts")
        .map_err(|e| format!("failed to read /proc/mounts: {e}"))?;
    if is_mounted(&mounts, &request.mount_point, &request.source) {
        // Idempotent no-op.
        return Ok(());
    }

    let mut options = request.options.clone();
    if let Some(cred_path) = &cred_path {
        if !options.is_empty() {
            options.push(',');
        }
        options.push_str(&format!("credentials={}", cred_path.display()));
    }

    let binary = match request.fs_type.as_str() {
        "cifs" => resolve_binary("mount.cifs")?,
        "nfs" | "nfs4" => resolve_binary("mount.nfs")?,
        other => return Err(format!("unsupported fs_type '{other}' (expected cifs|nfs)")),
    };

    let mut cmd = Command::new(binary);
    cmd.arg(&request.source).arg(&request.mount_point);
    if !options.is_empty() {
        cmd.arg("-o").arg(&options);
    }

    let output = cmd
        .output()
        .map_err(|e| format!("failed to execute mount command: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "mount failed (exit {:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}

fn write_credentials_file(request: &Request, slug: &str) -> Result<PathBuf, String> {
    fs::create_dir_all(CREDENTIALS_DIR)
        .map_err(|e| format!("failed to create {CREDENTIALS_DIR}: {e}"))?;

    let path = PathBuf::from(CREDENTIALS_DIR).join(format!("{slug}.cred"));
    let contents = format!(
        "username={}\npassword={}\ndomain={}\n",
        request.username.as_deref().unwrap_or(""),
        request.password.as_deref().unwrap_or(""),
        request.domain.as_deref().unwrap_or(""),
    );

    // Create the file with mode 0600 atomically at open() time (via O_CREAT's mode
    // argument) so it never exists in a world-readable state - fixing the prior
    // create-then-chmod race where the plaintext password was briefly writable to a
    // file at the process umask default (typically 0644). This helper only ever runs
    // as root (invoked exclusively via pkexec), so the file is already root:root by
    // virtue of the creating process's uid/gid - no separate chown is needed.
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .map_err(|e| format!("failed to create {}: {e}", path.display()))?;
    file.write_all(contents.as_bytes())
        .map_err(|e| format!("failed to write {}: {e}", path.display()))?;

    Ok(path)
}

// ---------------------------------------------------------------------------
// unmount
// ---------------------------------------------------------------------------

fn cmd_unmount(request: &Request) -> Result<(), String> {
    let mounts = fs::read_to_string("/proc/mounts")
        .map_err(|e| format!("failed to read /proc/mounts: {e}"))?;
    if !is_mounted(&mounts, &request.mount_point, &request.source) {
        // Idempotent no-op.
        return Ok(());
    }

    let binary = resolve_binary("umount")?;
    let output = Command::new(binary)
        .arg(&request.mount_point)
        .output()
        .map_err(|e| format!("failed to execute umount: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "umount failed (exit {:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// persist / unpersist
// ---------------------------------------------------------------------------

fn cmd_persist(request: &Request, slug: &str) -> Result<(), String> {
    let cred_path = PathBuf::from(CREDENTIALS_DIR).join(format!("{slug}.cred"));
    let cred_path = cred_path.exists().then(|| cred_path.display().to_string());

    let new_line = build_fstab_line(
        &request.source,
        &request.mount_point,
        &request.fs_type,
        &request.options,
        cred_path.as_deref(),
        slug,
    );

    with_locked_fstab(|contents| {
        let stripped = strip_tagged_line(contents, slug);
        let mut updated = stripped;
        if !updated.is_empty() && !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str(&new_line);
        updated.push('\n');
        updated
    })
}

fn cmd_unpersist(_request: &Request, slug: &str) -> Result<(), String> {
    with_locked_fstab(|contents| strip_tagged_line(contents, slug))?;

    let cred_path = PathBuf::from(CREDENTIALS_DIR).join(format!("{slug}.cred"));
    if cred_path.exists() {
        fs::remove_file(&cred_path)
            .map_err(|e| format!("failed to delete {}: {e}", cred_path.display()))?;
    }
    Ok(())
}

/// Acquire an exclusive `flock` on `/etc/fstab`, read its contents, run `transform`
/// over them, and atomically replace the file (temp file in `/etc/` + `rename()`).
/// This I/O is not unit-tested (requires root + `/etc/fstab` access); `transform`
/// itself should be a pure function tested independently.
fn with_locked_fstab(transform: impl FnOnce(&str) -> String) -> Result<(), String> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(FSTAB_PATH)
        .map_err(|e| format!("failed to open {FSTAB_PATH}: {e}"))?;

    let fd = file.as_raw_fd();
    // SAFETY: `fd` is a valid, open file descriptor owned by `file` for the
    // duration of this call; `flock` operates only on that fd.
    let lock_result = unsafe { libc::flock(fd, libc::LOCK_EX) };
    if lock_result != 0 {
        return Err(format!(
            "failed to lock {FSTAB_PATH}: {}",
            std::io::Error::last_os_error()
        ));
    }

    let contents =
        fs::read_to_string(FSTAB_PATH).map_err(|e| format!("failed to read {FSTAB_PATH}: {e}"))?;
    let updated = transform(&contents);

    let tmp_path = format!("{FSTAB_PATH}.drivedock.tmp");
    fs::write(&tmp_path, &updated).map_err(|e| format!("failed to write {tmp_path}: {e}"))?;
    fs::rename(&tmp_path, FSTAB_PATH)
        .map_err(|e| format!("failed to replace {FSTAB_PATH}: {e}"))?;

    // Lock is released implicitly when `file` is dropped (closing the fd).
    Ok(())
}

// ---------------------------------------------------------------------------
// Pure, unit-tested logic
// ---------------------------------------------------------------------------

/// Sanitize an arbitrary share name into a filesystem/fstab-tag-safe slug:
/// lowercase ASCII alphanumerics, `-`, and `_` only; everything else is dropped.
fn slugify(name: &str) -> String {
    name.chars()
        .map(|c| c.to_ascii_lowercase())
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

/// Validate that `mount_point` is a direct child of `/mnt/`, with no `..`
/// components and no symlink-style tricks reachable via the path string itself.
/// Rejects anything not of the form `/mnt/<single-segment>`.
fn validate_mount_point(mount_point: &str) -> Result<(), String> {
    let path = Path::new(mount_point);
    if !path.is_absolute() {
        return Err("mount_point must be an absolute path".to_string());
    }

    let base = Path::new(MOUNT_BASE);
    let relative = path
        .strip_prefix(base)
        .map_err(|_| format!("mount_point must be a child of {MOUNT_BASE}"))?;

    let mut components = relative.components();
    let Some(first) = components.next() else {
        return Err(format!("mount_point must not be {MOUNT_BASE} itself"));
    };
    if components.next().is_some() {
        return Err(format!(
            "mount_point must be a direct child of {MOUNT_BASE} (no nested paths)"
        ));
    }

    use std::path::Component;
    match first {
        Component::Normal(segment) => {
            if segment == OsStr::new("..") || segment.to_string_lossy().contains("..") {
                return Err("mount_point must not contain '..'".to_string());
            }
        }
        _ => return Err("mount_point contains an invalid path component".to_string()),
    }

    Ok(())
}

/// Parse `/proc/mounts` contents and check whether `mount_point` is currently
/// mounted with a `cifs`/`cifs2`/`nfs`/`nfs4` filesystem whose source matches
/// `source`.
fn is_mounted(proc_mounts: &str, mount_point: &str, source: &str) -> bool {
    const NETWORK_FS_TYPES: &[&str] = &["cifs", "cifs2", "nfs", "nfs4"];
    proc_mounts.lines().any(|line| {
        let mut fields = line.split_whitespace();
        let Some(mount_source) = fields.next() else {
            return false;
        };
        let Some(mount_dir) = fields.next() else {
            return false;
        };
        let Some(fs_type) = fields.next() else {
            return false;
        };
        NETWORK_FS_TYPES.contains(&fs_type) && mount_dir == mount_point && mount_source == source
    })
}

/// Resolve a binary name (e.g. `mount.cifs`) against the fixed, root-owned
/// candidate directory list - never the caller's inherited `$PATH`.
fn resolve_binary(name: &str) -> Result<PathBuf, String> {
    for dir in BIN_CANDIDATE_DIRS {
        let candidate = Path::new(dir).join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "could not locate '{name}' in any of {BIN_CANDIDATE_DIRS:?}"
    ))
}

/// Build the `/etc/fstab` line for a network share mount, tagged with
/// `# drivedock:<slug>` so it can be found/replaced/removed idempotently.
/// `_netdev` is always included, mandatory per spec (without it, systemd's fstab
/// generator does not order the mount after network availability).
fn build_fstab_line(
    source: &str,
    mount_point: &str,
    fs_type: &str,
    options: &str,
    cred_path: Option<&str>,
    slug: &str,
) -> String {
    let mut opts: Vec<String> = options
        .split(',')
        .map(|o| o.trim())
        .filter(|o| !o.is_empty())
        .map(|o| o.to_string())
        .collect();
    if let Some(cred_path) = cred_path {
        opts.push(format!("credentials={cred_path}"));
    }
    if !opts.iter().any(|o| o == "_netdev") {
        opts.push("_netdev".to_string());
    }
    let opts_str = if opts.is_empty() {
        "defaults".to_string()
    } else {
        opts.join(",")
    };

    format!("{source} {mount_point} {fs_type} {opts_str} 0 0  # drivedock:{slug}")
}

/// Remove any pre-existing line tagged `# drivedock:<slug>` from `fstab_contents`,
/// leaving all other lines (including comments/blank lines) untouched and in order.
///
/// The tag match is anchored to the end of the (trimmed) line rather than a loose
/// substring check: `# drivedock:<slug>` is always appended as trailing content by
/// `build_fstab_line`, so `line.trim_end().ends_with(&tag)` matches only a line whose
/// tag is exactly `slug` - not a line tagged with a different slug that merely starts
/// with `slug` as a prefix (e.g. `"media"` vs `"media2"`), which an unanchored
/// `line.contains(&tag)` would incorrectly match and strip.
fn strip_tagged_line(fstab_contents: &str, slug: &str) -> String {
    let tag = format!("# drivedock:{slug}");
    fstab_contents
        .lines()
        .filter(|line| !line.trim_end().ends_with(&tag))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify_basic() {
        assert_eq!(slugify("media-fileserver"), "media-fileserver");
        assert_eq!(slugify("Media Fileserver!"), "mediafileserver");
        assert_eq!(slugify("../etc/passwd"), "etcpasswd");
    }

    #[test]
    fn test_validate_mount_point_accepts_direct_child() {
        assert!(validate_mount_point("/mnt/media-fileserver").is_ok());
    }

    #[test]
    fn test_validate_mount_point_rejects_outside_base() {
        assert!(validate_mount_point("/etc/passwd").is_err());
        assert!(validate_mount_point("/home/user/mnt/share").is_err());
    }

    #[test]
    fn test_validate_mount_point_rejects_dotdot() {
        assert!(validate_mount_point("/mnt/../etc").is_err());
    }

    #[test]
    fn test_validate_mount_point_rejects_nested_path() {
        assert!(validate_mount_point("/mnt/a/b").is_err());
    }

    #[test]
    fn test_validate_mount_point_rejects_relative_path() {
        assert!(validate_mount_point("mnt/share").is_err());
    }

    #[test]
    fn test_validate_mount_point_rejects_base_itself() {
        assert!(validate_mount_point("/mnt").is_err());
    }

    #[test]
    fn test_is_mounted_matches_network_fs_types() {
        let proc_mounts = "//fileserver/media /mnt/media-fileserver cifs rw,relatime 0 0\n\
             fileserver:/export/media /mnt/nfs-media nfs4 rw,relatime 0 0\n\
             /dev/sda1 / ext4 rw,relatime 0 0\n";
        assert!(is_mounted(
            proc_mounts,
            "/mnt/media-fileserver",
            "//fileserver/media"
        ));
        assert!(is_mounted(
            proc_mounts,
            "/mnt/nfs-media",
            "fileserver:/export/media"
        ));
        assert!(!is_mounted(proc_mounts, "/", "/dev/sda1"));
    }

    #[test]
    fn test_is_mounted_rejects_mismatched_source() {
        let proc_mounts = "//fileserver/media /mnt/media-fileserver cifs rw,relatime 0 0\n";
        assert!(!is_mounted(
            proc_mounts,
            "/mnt/media-fileserver",
            "//other/media"
        ));
    }

    #[test]
    fn test_is_mounted_not_present() {
        let proc_mounts = "/dev/sda1 / ext4 rw,relatime 0 0\n";
        assert!(!is_mounted(proc_mounts, "/mnt/media-fileserver", "//fileserver/media"));
    }

    #[test]
    fn test_build_fstab_line_includes_netdev_and_tag() {
        let line = build_fstab_line(
            "//fileserver/media",
            "/mnt/media-fileserver",
            "cifs",
            "uid=1000,gid=1000",
            Some("/etc/drivedock/credentials/media-fileserver.cred"),
            "media-fileserver",
        );
        assert!(line.starts_with("//fileserver/media /mnt/media-fileserver cifs "));
        assert!(line.contains("_netdev"));
        assert!(line.contains("credentials=/etc/drivedock/credentials/media-fileserver.cred"));
        assert!(line.ends_with("# drivedock:media-fileserver"));
    }

    #[test]
    fn test_build_fstab_line_does_not_duplicate_netdev() {
        let line = build_fstab_line(
            "host:/export",
            "/mnt/nfs-share",
            "nfs",
            "_netdev,ro",
            None,
            "nfs-share",
        );
        assert_eq!(line.matches("_netdev").count(), 1);
    }

    #[test]
    fn test_strip_tagged_line_removes_only_matching_slug() {
        let fstab = "UUID=1234 / ext4 defaults 0 1\n\
             //fileserver/media /mnt/media-fileserver cifs defaults,_netdev 0 0  # drivedock:media-fileserver\n\
             host:/export /mnt/other-share nfs defaults,_netdev 0 0  # drivedock:other-share\n";
        let stripped = strip_tagged_line(fstab, "media-fileserver");
        assert!(!stripped.contains("drivedock:media-fileserver"));
        assert!(stripped.contains("drivedock:other-share"));
        assert!(stripped.contains("UUID=1234"));
    }

    #[test]
    fn test_strip_tagged_line_does_not_match_prefix_slug() {
        // Regression test for the unanchored-substring bug: "media" must not strip a
        // line tagged "# drivedock:media2", even though "# drivedock:media" is a
        // string-prefix of "# drivedock:media2".
        let fstab = "UUID=1234 / ext4 defaults 0 1\n\
             //fileserver/media /mnt/media cifs defaults,_netdev 0 0  # drivedock:media\n\
             //fileserver/media2 /mnt/media2 cifs defaults,_netdev 0 0  # drivedock:media2\n";
        let stripped = strip_tagged_line(fstab, "media");
        assert!(!stripped.contains("drivedock:media\n"));
        assert!(!stripped.lines().any(|l| l.trim_end().ends_with("# drivedock:media")));
        assert!(stripped.contains("drivedock:media2"));
        assert!(stripped.contains("UUID=1234"));
    }

    #[test]
    fn test_persist_then_strip_round_trip() {
        let existing = "UUID=1234 / ext4 defaults 0 1\n";
        let line = build_fstab_line(
            "//fileserver/media",
            "/mnt/media-fileserver",
            "cifs",
            "",
            None,
            "media-fileserver",
        );
        let mut with_entry = existing.to_string();
        with_entry.push_str(&line);
        with_entry.push('\n');
        assert!(with_entry.contains("drivedock:media-fileserver"));

        let stripped = strip_tagged_line(&with_entry, "media-fileserver");
        assert!(!stripped.contains("drivedock:media-fileserver"));
        assert!(stripped.contains("UUID=1234"));
    }

    #[test]
    fn test_resolve_binary_missing_returns_err() {
        // A name that will not exist in any candidate directory in the test sandbox.
        assert!(resolve_binary("definitely-not-a-real-binary-xyz").is_err());
    }
}
