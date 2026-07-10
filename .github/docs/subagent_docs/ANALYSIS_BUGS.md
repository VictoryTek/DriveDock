# DriveDock — Bug & Code Quality Analysis

Date: 2026-07-10. Scope: logic errors, security, performance, dead code, error handling.
(Architecture/structure findings live separately in `ANALYSIS_ARCH.md`; a few overlap where a
structural problem is also a concrete bug — those are restated here with the bug framing.)
Analysis is from code reading; a native build/test run is not possible on this Windows host
(GTK4 linkage, per CLAUDE.md Resource Constraints), so nothing below was runtime-verified.

Priorities: **High** = wrong behavior a user will hit, or a security issue in the privileged
path; **Medium** = real defect in edge cases or defense-in-depth gap; **Low** = hygiene.

---

## 1. Logic errors / likely bugs

### 1.1 Credentials collected for "permanently dock" are silently discarded by the helper — **High**
**Files:** `src/ui/window.rs:494-501` → `src/network/mount.rs:217-244` →
`src/bin/drivedock-mount-helper.rs:230-253` (`cmd_persist`).

When the user enables "permanently dock" on an SMB share, the GUI shows the credentials dialog
(`window.rs:495-499`) and sends username/password/domain in the persist request
(`mount.rs:239-241`). But `cmd_persist` never reads `request.username`/`password`/`domain` and
never calls `write_credentials_file` — it only checks whether a credentials file *already exists*
from a prior `mount` (`drivedock-mount-helper.rs:231-232`). Consequence: persisting a share that
was never mounted in this design (or whose cred file was cleaned up) writes an fstab line with
**no `credentials=` option** (`build_fstab_line` gets `cred_path: None`), so the boot-time mount
fails or hangs prompting on a non-existent console — after the user explicitly typed a password
that went nowhere. Fix: `cmd_persist` should write the credentials file when credentials are
present in the request, exactly like `cmd_mount` does.

### 1.2 Error-path checkbox revert re-fires `connect_toggled`, launching the opposite privileged operation — **High**
**Files:** `src/ui/window.rs:339` and `:507` (`check_clone.set_active(!enable)`).

`gtk::CheckButton::set_active` emits `toggled` even when called programmatically. The failure
path of both permanent-dock handlers reverts the checkbox, which re-enters the same closure and
spawns a *second* operation in the opposite direction — for network shares, a second `pkexec`
invocation (second Polkit prompt) attempting to undo something that never happened; for local
drives, a UDisks2 `RemoveConfigurationItem` for an entry that was never added. If that second
call also fails, its own error path calls `set_active` again — but with the now-current value the
signal may not re-fire, so it terminates, leaving the status line showing the error of the
*revert*, not the original failure. Needs a reentrancy guard (`Cell<bool>`) or
`block_signal`/`unblock_signal` around the revert.

### 1.3 Share names with spaces or any non-ASCII/URL-escaped characters break mount matching and mounting — **High**
**Files:** `src/network/discover.rs:154-165` (`mount_source_from_uri`),
`:169-185` (`is_network_mount_active`); `src/bin/drivedock-mount-helper.rs:355-370` (`is_mounted`).

GVfs URIs are percent-encoded: a share named `My Share` arrives as `smb://host/My%20Share`.
`mount_source_from_uri` does no percent-decoding, producing source `//host/My%20Share`, which is
then (a) passed literally to `mount.cifs` — which will request the share literally named
`My%20Share` from the server and fail, and (b) compared against `/proc/mounts`, where the kernel
octal-escapes spaces as `\040` (`//host/My\040Share`) — so even a correctly-mounted share never
matches and `mounted` stays `false`. Neither parser (`is_network_mount_active`, helper
`is_mounted`) unescapes `/proc/mounts` octal sequences. Consequences: any share with a space,
`#`, or non-ASCII character in its path cannot be mounted; and if mounted externally, the UI
shows a "Dock" button for an already-mounted share, and the helper's idempotency check fails
(source string mismatch) so it attempts an over-mount. Also related: URIs carrying an explicit
port (`smb://host:445/share`) produce `//host:445/share`, which never matches `/proc/mounts`
either.

### 1.4 Shares whose display name contains no ASCII alphanumerics cannot be docked at all — **Medium**
**Files:** `src/network/mount.rs:51-56` (`slugify`), `:70-72` (`mount_point_for`);
`src/bin/drivedock-mount-helper.rs:87-89`.

`slugify` keeps only ASCII `[a-z0-9_-]`. A share named `Média`, `общий`, or `共有` slugs to a
partial or empty string. Empty slug → mount point `/mnt/` → helper rejects with "mount_point must
not be /mnt itself" (or "share_name does not contain any valid slug characters"), surfaced to the
user as a raw error. Non-Latin share names are simply unsupported, with no graceful message.

### 1.5 Slug collisions: distinct shares silently share one mount point, credentials file, and fstab tag — **Medium**
**Files:** `src/network/mount.rs:51-56`, `src/bin/drivedock-mount-helper.rs:308-313`, `:416`, `:428-435`.

`slugify("Media Server")`, `slugify("MediaServer")`, and `slugify("media/server")` all yield
`mediaserver`. Two different shares from two different hosts then map to the same
`/mnt/mediaserver`, the same `/etc/drivedock/credentials/mediaserver.cred` (second mount
**overwrites the first share's stored password**), and the same `# drivedock:mediaserver` fstab
tag (persisting share B *replaces* share A's fstab line via `strip_tagged_line`). Mounting share B
while share A is mounted at the same point passes the `is_mounted` check (source differs →
"not mounted") and stacks an over-mount on top of A. Nothing detects or reports any of this. The
slug should incorporate the host, or collisions should be detected and refused.

### 1.6 "Permanently dock" on a local drive persists its `/run/media/...` mount point — **Medium**
**Files:** `src/ui/window.rs:322-329`; `src/udisks.rs:111-135`.

The fstab entry is written with the drive's *current* mount point. For removable media
auto-mounted by udisks, that is `/run/media/<user>/<label>` — a tmpfs path that does not exist at
boot. The resulting fstab line either fails at boot (systemd cannot order it; the dir is created
but `/run/media` semantics conflict with udisks' dynamic ownership) or fights with udisks'
own auto-mount of the same device to the same path. "Permanently dock" for a udisks-auto-mounted
drive should choose a stable mount point (e.g. `/mnt/<label>`), not blindly reuse the session
path. At minimum this deserves a warning, since it's the *primary* use case (removable drives are
exactly what users want to permanently dock).

### 1.7 Cancelling the credentials dialog proceeds with an anonymous mount — **Medium**
**Files:** `src/network/mount.rs:313-327`, `:335`; callers `src/ui/window.rs:576-582`, `:495-501`.

`collect_credentials` returns `None` for both "user pressed Cancel" and "dialog couldn't
determine a response"; callers treat `None` as "mount without credentials" and proceed to spawn
`pkexec`. A user who hits Cancel to abort still gets a Polkit password prompt and a guest-auth
CIFS mount attempt against the server. Cancel must abort the operation (distinguish
`Cancelled` from `NoCredentials` in the return type).

### 1.8 Mounted-but-undiscovered shares are invisible — can't be undocked from the UI — **Medium**
**File:** `src/network/discover.rs:53-73`.

The share list is seeded *only* from GVfs `network:///` enumeration; `/proc/mounts` is used only
to set the `mounted` flag on discovered shares. If a share is mounted (by DriveDock, persisted in
fstab, or manually) but its server is currently not advertising/browsable (server reboot, Avahi
off, WSD disabled, `gvfsd-network` not running — which the code itself anticipates at :57-60),
the share simply doesn't appear, and the user has no way to undock it. Active `cifs`/`nfs` rows
in `/proc/mounts` under `/mnt/` should be merged into the list as mounted entries regardless of
discovery.

### 1.9 `unmount_share` silently guesses `cifs` for unknown protocols — **Low**
**File:** `src/network/mount.rs:187`.

`fs_type_for_protocol(...).unwrap_or("cifs")` — the siblings `mount_share` (:156) and
`set_persistent` (:222) error out for non-SMB/NFS protocols. Unreachable in practice (the UI only
shows Undock for `mounted == true`, which requires a cifs/nfs `/proc/mounts` row), but the
fallback misleads and would send a wrong `fs_type` to the helper if the invariant ever breaks.

### 1.10 `validate_mount_point`'s `..` checks are partly dead, partly overreaching — **Low**
**File:** `src/bin/drivedock-mount-helper.rs:339-347`.

Within the `Component::Normal(segment)` arm, `segment == OsStr::new("..")` can never be true —
`std::path` parses a literal `..` as `Component::ParentDir`, which the `_ =>` arm already rejects.
And `to_string_lossy().contains("..")` rejects legitimate names like `my..share` (harmless but
arbitrary). The dead check suggests the parser semantics weren't verified; the wildcard arm is
the one doing the actual work.

### 1.11 `used space` is computed from `free`, not `available` — **Low**
**File:** `src/system/local.rs:128-152`.

`used = size - free` uses `FILESYSTEM_FREE` (includes root-reserved blocks). The label then says
"X available of Y" (`window.rs:273-278`) — but the value shown is derived from *free*, not
*available* (`filesystem::free` ≠ what an unprivileged user can actually use on ext4 with the 5%
reserve). The label overstates available space by the reserve amount. Cosmetic accuracy issue.

### 1.12 `is_critical_mount` does not protect `/nix` on the project's own primary target OS — **Medium**
**File:** `src/system/local.rs:159-163`.

The critical list is `/`, `/boot`, `/boot/efi`, `/usr`, `/var`, `/home`. DriveDock is
Nix-flake-packaged and NixOS-aware everywhere else, yet `/nix` (and `/nix/store`, a separate
bind mount on many setups) is absent — unmounting it kills the running system as thoroughly as
unmounting `/usr`. Also missing: `/srv`, `/var/log` (if a separate volume), `/tmp`, and swap is
unhandled (though `VolumeMonitor` likely won't surface these, the unmount path takes an arbitrary
mount point and re-checks this same list at `unmount.rs:49`).

---

## 2. Security

### 2.1 fstab line injection through unvalidated `source`, `mount_point` content, and `options` — **High**
**Files:** `src/bin/drivedock-mount-helper.rs:390-417` (`build_fstab_line`), `:230-253`
(`cmd_persist`); input parsing `:52-66`.

The helper is the privilege boundary (root via `pkexec`; Polkit `auth_admin_keep` means that
after one admin authentication, *any process of the same user session* can invoke it without a
prompt for the keep-window). Yet `cmd_persist` writes `request.source`, `request.mount_point`,
`request.fs_type`, and `request.options` into `/etc/fstab` with **no character validation**:

- A `source` containing whitespace shifts every subsequent fstab field (options become the
  fs-type field, etc.).
- A `source` or `options` containing a **newline** injects an arbitrary, attacker-controlled
  additional fstab line — e.g. one that mounts a `suid`-capable filesystem, or overrides `/etc`
  via a bind mount, at next boot. That is a local-root persistence primitive gated only by one
  cached admin auth.
- `fs_type` is not restricted to `cifs`/`nfs` in `cmd_persist` (only `cmd_mount` checks it,
  :142-145), so persist accepts any fs_type string.
- `options` is passed to `mount.cifs -o` in `cmd_mount` unvalidated; CIFS options like
  `setuid`/`dev` (absence of `nosuid,nodev,noexec` defaults — see 2.2) are accepted.

`validate_mount_point` correctly constrains the mount-point *shape* but nothing constrains its
bytes beyond path components (a segment can contain spaces/newline? — a newline inside a single
path component passes `validate_mount_point` and lands in the fstab line). The helper must
reject whitespace/newlines in every field it writes to fstab and whitelist `fs_type`
in all subcommands, not just `mount`.

### 2.2 Network mounts are made without `nosuid,nodev,noexec` — **Medium**
**Files:** `src/bin/drivedock-mount-helper.rs:134-152` (mount options), `:390-417` (fstab options);
GUI always sends `options: ""` (`src/network/mount.rs:174`, `:199`, `:238`).

A root-performed CIFS/NFS mount defaults to honoring suid bits and device nodes from the remote
server (CIFS default is `nosuid,nodev`? — no: `mount.cifs` defaults *do* include `nosuid,nodev`
only when the user mounts via setuid mount.cifs, not when root mounts). A malicious or
compromised SMB/NFS server can serve a setuid-root binary; any local user then executes it for
instant root. Standard practice for GUI-initiated network mounts (GVfs, udisks) is to force
`nosuid,nodev` (and usually `noexec` for CIFS). The helper should append these unconditionally to
both the live mount options and the generated fstab line.

### 2.3 Plaintext credentials file outlives the mount and is orphaned on failure — **Medium**
**Files:** `src/bin/drivedock-mount-helper.rs:122-125`, `:203-224` (`cmd_unmount`), `:255-264`.

`cmd_mount` writes `/etc/drivedock/credentials/<slug>.cred` (plaintext password, 0600 root)
*before* attempting the mount; if the mount fails, the file stays. `cmd_unmount` never deletes
it. Only `cmd_unpersist` cleans it up. So a one-off temporary mount (never persisted) leaves the
user's SMB password on disk in plaintext indefinitely — root-only readable, but backed up by any
root-run backup tool, readable in offline disk access, and surviving long after the credential
had any reason to exist. Either delete on unmount when the share isn't persisted, or use a
runtime-only path (`/run/drivedock/...`) for non-persisted mounts.

### 2.4 Credential file injection via newlines in username/domain — **Low**
**File:** `src/bin/drivedock-mount-helper.rs:172-178`.

`write_credentials_file` formats `username={}\npassword={}\ndomain={}` with no escaping. A
username containing `\npassword=x` injects/overrides keys in the mount.cifs credentials format.
The values come from the user's own dialog (self-attack only, given the request already carries
the real password), so impact is minimal — but rejecting `\n`/`\r` in all three fields is one
line of code at the privilege boundary.

### 2.5 `/etc/fstab` replacement is not crash-safe and defeats concurrent `flock` users — **Medium**
**File:** `src/bin/drivedock-mount-helper.rs:270-300` (`with_locked_fstab`).

Two issues: (a) `fs::write(tmp)` + `rename` with **no `fsync`** on the temp file or the `/etc`
directory — on power loss at the wrong moment, `/etc/fstab` can be left pointing at a zero-length
or partially-written inode on some filesystems. Corrupting fstab can make the system unbootable;
this is the one file in the project worth an `File::sync_all()` before `rename` (and ideally a
dir fsync after). (b) The `flock` is taken on the *old* inode; after the rename, a second helper
instance blocked on that lock proceeds while a third instance opens and locks the *new* inode —
the classic lock-then-replace race allowing lost updates between concurrent persist/unpersist
calls. Low practical likelihood (two concurrent DriveDock persists), but the lock as written
doesn't provide the guarantee the comment claims. A separate lock file (`/run/lock/drivedock-fstab.lock`)
that is never renamed fixes it.

### 2.6 Helper does not verify the mount-point/slug/share_name relationship — **Low**
**File:** `src/bin/drivedock-mount-helper.rs:87-98`.

`slug` derives from `share_name` while `mount_point` is validated independently; a request can
pair slug `a` (thus credentials file `a.cred`, fstab tag `drivedock:a`) with mount point
`/mnt/b`. Nothing exploits this directly today, but it means the tag/credfile/mountpoint triple
that the design treats as one identity can be desynchronized by a buggy caller, producing
un-unpersistable fstab lines (tag `a` but the GUI later recomputes slug `b`). Cheap fix: require
`mount_point == format!("{MOUNT_BASE}/{slug}")`.

---

## 3. Performance

### 3.1 Synchronous `pkexec` subprocess blocks the GTK main loop for the whole auth+mount duration — **High**
**Files:** `src/network/mount.rs:118-151`; called on the main thread via
`glib::spawn_future_local` at `src/ui/window.rs:494`, `:536`, `:575`.

`Command::spawn` + blocking `write_all` + `wait_with_output` run inside futures scheduled on the
GTK main context. The UI is completely frozen — no repaint, no input — from click until the
Polkit dialog is answered and `mount.cifs` returns (network mounts to an unresponsive server can
block for tens of seconds in kernel retries). The window will be flagged unresponsive by the
compositor. Use `gio::Subprocess` (async, already available via the `gio` dep) or
`gio::spawn_blocking`.

### 3.2 Sequential awaits in refresh; full rebuild of all rows on every refresh — **Low**
**File:** `src/ui/window.rs:172-221`; `src/system/local.rs:97-108`.

`refresh_drives` awaits local enumeration fully before starting network discovery (which itself
does a serial two-level GVfs walk with `next_files_future(10, ...)` batches at
`discover.rs:114-134`); each volume's fs-type and fs-stats are two more sequential D-Bus/GIO
round trips per drive (`local.rs:100-101`). Then every row widget is destroyed and rebuilt.
On a machine with many volumes/servers, refresh takes the *sum* of all round trips. All of these
are independent and could be `futures::join!`-ed. Impact is small at typical scale — noting
because the refresh runs at startup on the main loop and after every operation.

### 3.3 `helper_path()` and `Udisks::new()` re-resolved per operation — **Low**
**Files:** `src/network/mount.rs:87-114`; `src/ui/window.rs:318`, `:417`; `src/system/unmount.rs:57`.

Every dock/undock/toggle creates a new D-Bus connection to UDisks2 (`Client::new`) and re-stats
the helper path. Harmless at click frequency; flagged only because the connection setup is
itself an async D-Bus handshake done on the main context. Not worth fixing unless 3.1 is fixed
first.

---

## 4. Dead / redundant / useless code

### 4.1 `tokio` + the `tokio-runtime` feature: declared, never referenced — **Medium**
**File:** `Cargo.toml:41`, `:55-60`. No `#[cfg(feature = "tokio-runtime")]` or `tokio::` anywhere
in `src/`. Pure weight in `Cargo.lock` and the Nix vendored-deps closure.

### 4.2 `is_network_uri` — `#[allow(dead_code)]` with a speculative-future comment — **Low**
**File:** `src/network/discover.rs:136-147`. No callers outside its own test (:199-204). The
project's stated principle is "no speculative code"; delete it and its test.

### 4.3 `MountRequest.options` / `Request.options` is always `""` from the GUI — **Low**
**Files:** `src/network/mount.rs:174`, `:199`, `:238`; `src/bin/drivedock-mount-helper.rs:59`.
All three call sites hardcode `options: ""`. The entire options-merging logic in the helper
(`:134-140`, `:398-403`) is exercised only by hypothetical non-GUI callers — which, per 2.1, is
exactly the caller class the field shouldn't trust. Either the GUI grows an options UI or the
field should go (and with it, one injection surface).

### 4.4 Duplicated `test_is_critical_mount` — **Low**
**Files:** `src/system/local.rs:179-187` = `src/system/unmount.rs:85-92`, identical tests of the
same function. Keep the copy next to the definition.

### 4.5 Commented-out re-export; `#[allow(unused_imports)]` re-export — **Low**
**Files:** `src/ui/mod.rs:3-4`; `src/network/mod.rs:6-9`. The `Credentials` re-export is unused
because callers never name the type — which is itself a symptom of 1.7 (the type is too
anemic to name). Delete or use.

### 4.6 `bytes_to_str`/`str_to_bytes` asymmetry note — **Low**
**File:** `src/udisks.rs:176-185`. Not dead, but `str_to_bytes` NUL-terminates while UDisks2's
`ay` fstab fields conventionally require it — fine; `bytes_to_str` is only used for `Device`
comparison. No action; documented here because the pair looks like dead helpers but isn't.

---

## 5. Error handling

### 5.1 UUID-based block lookup silently swallows lookup errors — **Medium**
**File:** `src/udisks.rs:43-51`.

`self.client.block_for_uuid(uuid).await.into_iter().next()` — if `block_for_uuid` returns a
`Result`, `.into_iter().next()` converts `Err` into `None` *silently* (Result implements
IntoIterator over the Ok value). A transient D-Bus error on the fast path is indistinguishable
from "UUID not found", and the code falls through to the full object-manager scan (:54-75) —
usually masking the problem, but if the scan also can't match (e.g. device renamed), the final
error says "No UDisks2 block object found for device X", hiding the real cause. Match the Result
explicitly and log the error before falling back.

### 5.2 D-Bus error classification by substring sniffing — **Medium**
**File:** `src/system/unmount.rs:66-75`.

`msg.contains("busy")`, `contains("NotAuthorized")`, `contains("permission")` against a formatted
error string. D-Bus errors carry structured names
(`org.freedesktop.UDisks2.Error.DeviceBusy`, `org.freedesktop.PolicyKit1.Error.NotAuthorized`);
string matching breaks under localization or message rewording, and `contains("busy")` matches
unrelated text. Since no caller matches on `UnmountError` variants anyway (window.rs just
displays it), either match on the zbus error name properly or drop the classification.

### 5.3 Both discovery failures write to one status line; second overwrites first — **Low**
**File:** `src/ui/window.rs:186-210`.

If `list_local_drives` and `scan_network_shares` both fail, only the network error remains
visible. And when both fail, `any == false` additionally renders the "No drives or shares found /
Connect a drive…" placeholder (:212-220) — telling the user everything is fine and empty while
the status line says scanning failed. Contradictory UI on the same screen.

### 5.4 `collect_credentials` can hang forever when the dialog has no parent — **Low**
**File:** `src/network/mount.rs:329-335`.

`dialog.present(gtk::Window::NONE)` on an `adw::AlertDialog` (which must be presented within a
widget hierarchy in libadwaita 1.5+) may fail to map the dialog. If no response is ever emitted,
`rx.await` never resolves and the spawned future leaks — with the triggering `CheckButton`/`Button`
left permanently insensitive (`set_sensitive(false)` at `window.rs:491`, `:565` with re-enable
only after the future completes). The `parent_window` lookup (`window.rs:492`, `:570-573`) should
be reliable in practice, but the `None` branch is an unrecoverable-hang path; better to return
`None` immediately (or make the parent mandatory).

### 5.5 Helper's stdout is captured and discarded — **Low**
**File:** `src/network/mount.rs:126`, `:142-147`.

`stdout(Stdio::piped())` but only stderr is included in the error message; on failure any stdout
diagnostics from `mount.cifs`/`mount.nfs` (which often print to stdout) are dropped. Include both
streams in the error.

### 5.6 `/proc/mounts` read failure treats every share as unmounted, with only a log — **Low**
**File:** `src/network/discover.rs:63-69`.

Documented, and arguably fine — but the consequence is that the UI then shows "Dock" for mounted
shares, and clicking it invokes `pkexec` and the helper's own is_mounted check (which reads
`/proc/mounts` again and no-ops). Self-healing but confusing; a status-line notice would match
how the local-drive failure is surfaced (`window.rs:193-196` vs nothing here).

---

## Summary table

| # | Finding | Priority |
|---|---------|----------|
| 1.1 | Persist discards user-entered credentials → broken boot mounts | High |
| 1.2 | Checkbox revert re-fires toggled → opposite privileged op runs | High |
| 1.3 | No percent-decoding / octal-unescaping → shares with spaces can't mount or match | High |
| 2.1 | fstab injection via unvalidated source/options/mount_point bytes in privileged helper | High |
| 3.1 | Blocking pkexec/mount subprocess freezes the GTK main loop | High |
| 1.4 | Non-ASCII share names → empty slug → hard error | Medium |
| 1.5 | Slug collisions merge distinct shares' mount point, cred file, fstab tag | Medium |
| 1.6 | Local permanent-dock persists volatile `/run/media/...` path into fstab | Medium |
| 1.7 | Credentials-dialog Cancel proceeds with anonymous mount | Medium |
| 1.8 | Mounted-but-undiscovered shares invisible → cannot undock | Medium |
| 1.12 | `is_critical_mount` misses `/nix` on the primary target OS | Medium |
| 2.2 | Network mounts lack forced `nosuid,nodev,noexec` | Medium |
| 2.3 | Plaintext credential file survives mount failure/unmount | Medium |
| 2.5 | fstab rewrite: no fsync (crash-unsafe) + flock-on-renamed-inode race | Medium |
| 4.1 | Unused `tokio` dependency/feature | Medium |
| 5.1 | UUID lookup errors silently coerced to None | Medium |
| 5.2 | D-Bus error classification via substring matching | Medium |
| 1.9 | `unmount_share` defaults unknown protocol to cifs | Low |
| 1.10 | Dead `..` check in validate_mount_point | Low |
| 1.11 | "Available" label computed from free, not available | Low |
| 2.4 | No newline rejection in credentials file fields | Low |
| 2.6 | slug/mount_point identity not cross-checked | Low |
| 3.2 | Sequential refresh awaits; full row rebuild | Low |
| 3.3 | Per-operation D-Bus reconnect / helper re-resolution | Low |
| 4.2 | Dead `is_network_uri` | Low |
| 4.3 | `options` field always empty from GUI | Low |
| 4.4 | Duplicated critical-mount test | Low |
| 4.5 | Commented-out / allow-suppressed re-exports | Low |
| 5.3 | Second discovery error overwrites first; contradictory empty-state | Low |
| 5.4 | Parentless credentials dialog can hang the operation forever | Low |
| 5.5 | Helper stdout discarded from error messages | Low |
| 5.6 | /proc/mounts read failure silently marks all shares unmounted | Low |
