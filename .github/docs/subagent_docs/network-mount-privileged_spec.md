# DriveDock: Privileged System-Wide Network Share Mounting — Phase 1 Specification

Status: DRAFT (Phase 1 — Research & Specification only, no code changes)
Author: Orchestrating Agent (Phase 1 subagent)
Date: 2026-07-09
Supersedes (in part): `.github/docs/subagent_docs/nix-pivot-scope-narrow_spec.md` §3.2–§3.4 (network-share
mount/persist portions only — local-drive UDisks2 design, GVfs *discovery*, and the OS-detection/NixOS
config-snippet pattern for local drives are unchanged and reused).

---

## 1. Current State Analysis

### 1.1 What exists today (post nix-pivot)

- **`src/network/gvfs.rs`** (re-exported via `src/network/mod.rs`) does two jobs today that must be
  split:
  1. **Discovery** (keep, unchanged): `scan_network_shares()` merges (a) already-mounted network
     `gio::Mount`s from `gio::VolumeMonitor::get().mounts()` and (b) not-yet-mounted shares found by
     `enumerate_network_root()` walking `gio::File::for_uri("network:///")` one level deep via
     `enumerate_children_future`/`next_files_future`. This is GVfs's own server/share browsing
     (`gvfsd-network`, itself using Avahi/SMB-browsing/WS-Discovery) — confirmed working, zero
     subprocess shell-outs, and explicitly **not** the problem reported by the user.
  2. **Mounting** (replace): `mount_share()`/`unmount_share()` call
     `gio::File::mount_enclosing_volume_future` / `gio::Mount::unmount_with_operation_future`, backed
     by `interactive_mount_operation()` — a `gio::MountOperation` wired to an `AdwAlertDialog` with
     username/domain/password rows, answering GVfs's `ask-password` signal. This produces a
     **GVfs/FUSE mount at `/run/user/<uid>/gvfs/smb-share:server=...,share=...`**, which is
     session-scoped: invisible to root-run services, other users, containers, or anything before login.
     This is the confirmed defect.
- **`src/dock/shares.rs`**: "permanently dock" for network shares today means writing
  `~/.config/drivedock/persistent-shares.json` plus generating (not enabling) a
  `~/.config/systemd/user/drivedock-remount.service` unit that re-invokes
  `<drivedock-exe> --remount-shares` (handled in `main.rs`) to re-issue GVfs mounts at login. This
  entire mechanism exists **only** because GVfs mounts have no `/etc/fstab`/`Block` object to persist
  through — it is a workaround for GVfs's session-scoping, not a feature in its own right.
- **`src/dock/mod.rs`** / **`src/dock/nixos.rs`** / **`src/dock/fstab.rs`**: local-drive-only today.
  `Os::NixOs`/`Os::OtherLinux` detection (via `/etc/NIXOS` marker) and the NixOS
  "fstab write now (survives plain reboot) + show `fileSystems` config snippet (for `nixos-rebuild`
  durability)" pattern are proven, reviewed (Phase 3, grade A/96%), and reusable verbatim for network
  shares — this spec extends that pattern rather than inventing a new one.
- **`src/udisks.rs`**: thin `udisks2::Client` wrapper for `Filesystem.Mount`/`Unmount` and
  `Block.AddConfigurationItem`/`RemoveConfigurationItem`, used only for local block devices. Confirmed
  (this spec, §1.2) to remain inapplicable to network shares.
- **Cargo.toml**: `gtk4`, `libadwaita`, `glib`, `gio` 0.20-series, `udisks2` 0.3, `futures`, `libc`,
  `anyhow`, `thiserror`, `tracing`(-subscriber), `serde`/`serde_json`. **No D-Bus crate beyond what
  `udisks2` pulls in transitively (`zbus`)**. No second Cargo binary target exists yet (single
  `drivedock` bin).
- **`nix/package.nix`**: `rustPlatform.buildRustPackage`, `nativeBuildInputs = [ pkg-config
  wrapGAppsHook4 ]`, `buildInputs = [ gtk4 libadwaita glib ]`, `propagatedBuildInputs = [ gvfs ]`. No
  Polkit `.policy` installation exists. No `cifs-utils`/`nfs-utils`.
- **`nix/module.nix`**: `programs.drivedock.enable`, sets `services.udisks2.enable = true;` and
  `services.gvfs.enable = true;`. No `boot.supportedFilesystems` handling.

### 1.2 Confirmed research finding: UDisks2 still has no CIFS/NFS mount capability

Re-verified (not trusted blindly from the prior spec) against `storaged-project/udisks` upstream:
`udisksLinuxFilesystem` and the `org.freedesktop.UDisks2.Filesystem` D-Bus interface operate on
`Block` objects (local block devices) exclusively — there is no `Filesystem.Mount` variant, option, or
sibling interface for a CIFS/NFS network source. Open upstream issues (e.g. storaged-project/udisks#421,
#1323) describe UDisks2 *reacting to* a CIFS mount that already exists in `/proc/mounts` (added by
something else, e.g. `/etc/fstab` + the kernel) by polling/monitoring it — this is UDisks2 *observing* an
externally-created mount, not UDisks2 *creating* one. No 2025/2026 release adds a network-mount
`Filesystem`-equivalent interface. **Conclusion unchanged from the prior spec: UDisks2 cannot be the
privileged-mount mechanism for network shares.**

---

## 2. Problem Definition

GVfs mounts network shares per-user-session at `/run/user/<uid>/gvfs/...` (FUSE). This cannot satisfy
the user's now-final requirement: network shares marked "permanently dock" must land at a **stable,
system-wide path** (e.g. `/mnt/<share-name>`), mountable by the kernel itself, visible to root
services/other users/containers, and present before any desktop login — the same guarantee local drives
already get via UDisks2 + a real `/etc/fstab` entry.

**Decision needed**: how does an unprivileged GTK process (DriveDock) get a real `mount -t cifs`/
`mount -t nfs` executed as root, and a real `/etc/fstab` (or equivalent) entry written, safely,
idempotently, and consistently with this project's existing "prefer Polkit-proper patterns, not raw
`pkexec` shell-outs" stance — given no existing D-Bus service (like UDisks2 for local block devices)
performs this job for network filesystems?

Network share *discovery* (`gio::File::for_uri("network:///")`) is explicitly unchanged and out of
scope for this spec.

---

## 3. Proposed Solution Architecture — Decision

**Recommendation: Option 2 — a minimal privileged helper binary, installed with a Polkit `.policy`
action, invoked via `pkexec`.** Concrete rationale, and rejection of the alternatives, below.

### 3.1 Why not the alternatives

- **Option 1 (raw `pkexec mount -t cifs/nfs` + hand-rolled fstab `>>`/text edit)**: this is exactly the
  pattern the nix-pivot spec already rejected for local drives (race conditions, duplicate-entry risk,
  no idempotency guarantee). Reintroducing it for network shares — the very feature request that is
  explicitly about matching local drives' "same guarantee" — would be an inconsistent regression.
  Rejected.
- **Option 3 (systemd transient units via `org.freedesktop.systemd1` `StartTransientUnit` D-Bus, system
  bus)**: researched directly against `org.freedesktop.systemd1(5)` and systemd's own Polkit action
  table. Two disqualifying findings:
  1. `StartTransientUnit` units are **runtime-only** — they vanish on process exit/reboot. Durability
     across reboot still requires a **real `.mount` unit file written to `/etc/systemd/system/`**,
     which is a privileged file write of exactly the same class as an fstab write — systemd's D-Bus API
     has no method to durably persist a unit file for you. It does not eliminate the "write a
     privileged file safely" problem, it just relocates it from `/etc/fstab` to `/etc/systemd/system/`.
  2. The default Polkit action gating unit management, `org.freedesktop.systemd1.manage-units`, is
     `allow_active: auth_admin_keep` — i.e. it already requires a full admin-password prompt, the same
     weight as `pkexec`. There is no efficiency or UX win over a dedicated helper, only extra
     complexity (D-Bus proxy code, unit-name escaping, mount-vs-automount unit semantics, a new
     dependency surface). Rejected as the primary mechanism — noted as a possible *future* enhancement
     only for the already-mounted/no-persistence case, not adopted now (the user wants a decision, not
     a menu).
  3. `zbus` is available transitively via the `udisks2` crate, so it *could* be reused for a hand-rolled
     `systemd1` proxy without a new Cargo dependency — but per (1)/(2) above there is no benefit large
     enough to justify the added code, so this option is not pursued.
- **Option 4 (wait for UDisks2 to gain CIFS/NFS support)**: re-confirmed absent (§1.2). Rejected —
  not available today.

### 3.2 Why Option 2 fits this project

- Structurally identical, well-precedented pattern to how NetworkManager, gnome-disk-utility, and
  `timedatectl`'s underlying `systemd-timedated` perform privileged operations from a GUI: a small,
  single-purpose, root-executed helper + a declarative Polkit `.policy` action, invoked via `pkexec`.
  This *is* "Polkit-proper," not a step backward from raw `pkexec mount` — the raw-`pkexec`-of-`mount`
  pattern (Option 1) has no action-specific authorization message/scoping and cannot safely
  read-modify-write `/etc/fstab`; a dedicated helper does both correctly.
- Minimal code: one small `[[bin]]` crate target, no new external dependencies (std + already-present
  `serde`/`serde_json`/`anyhow`/`libc`), matching the Simplicity First principle.
- Gives DriveDock full control over: mount-point path validation (preventing arbitrary-path root
  writes), idempotent fstab read-modify-write (tagged entries, `flock`, atomic rename — the same
  guarantee UDisks2 gives locally, replicated by hand because no D-Bus service exists to delegate to),
  and credentials-file lifecycle (create with correct root:root 0600 ownership, delete on undock).

### 3.3 Module/file changes

```
src/
├── network/
│   ├── mod.rs               # re-exports updated: discover::scan_network_shares,
│   │                         # discover::NetworkShare, mount::mount_share, mount::unmount_share,
│   │                         # mount::set_persistent
│   ├── discover.rs           # RENAMED from gvfs.rs. Keeps scan_network_shares(),
│   │                         # enumerate_network_root(), enumerate_children(), is_network_uri(),
│   │                         # NetworkShare::protocol_from_uri() UNCHANGED.
│   │                         # CHANGED: NetworkShare.mounted is no longer sourced from
│   │                         # gio::VolumeMonitor::mounts() (that only reflects GVfs/FUSE mounts,
│   │                         # which we are no longer creating). Instead, `mounted` is computed by
│   │                         # parsing /proc/mounts for `cifs`/`cifs2`/`nfs`/`nfs4` fstype rows and
│   │                         # matching the share's `//host/share` (SMB) or `host:/export` (NFS)
│   │                         # source string. This mirrors how the pre-pivot codebase already
│   │                         # parsed /proc/mounts for local drives - now the technically correct
│   │                         # source of truth for *network* mount state specifically, since a real
│   │                         # kernel mount (not a GVfs Mount object) is what we create.
│   │                         # REMOVED: interactive_mount_operation() and any gio::MountOperation
│   │                         # plumbing (no longer applicable - GVfs is not doing the mount).
│   └── mount.rs               # NEW. Privileged mount/unmount/persist orchestration:
│                             # - collect_credentials(parent: Option<&gtk::Window>) -> Option<Creds>:
│                             #   a plain AdwAlertDialog (username/domain/password rows,
│                             #   PasswordEntryRow) reusing the existing dialog *layout* from the
│                             #   removed interactive_mount_operation(), but returning plain
│                             #   Rust strings instead of answering a gio signal.
│                             # - mount_share(share: &NetworkShare, creds: Option<Creds>,
│                             #   persist: bool) -> Result<()>: builds the JSON request (see §3.4),
│                             #   spawns `pkexec <libexec-path>/drivedock-mount-helper mount`,
│                             #   writes the request JSON to the child's stdin, waits for exit.
│                             # - unmount_share(share: &NetworkShare) -> Result<()>: same pattern,
│                             #   `... unmount`.
│                             # - set_persistent(share: &NetworkShare, creds: Option<Creds>,
│                             #   enable: bool) -> Result<DockResult>: `... persist`/`unpersist`
│                             #   subcommand; on NixOS, also calls
│                             #   crate::dock::nixos::network_config_snippet_message() (§3.6) for the
│                             #   Status-panel guidance text, mirroring dock::set_permanent_dock for
│                             #   local drives.
├── dock/
│   ├── mod.rs                # UNCHANGED (local-drive Os detection/logic untouched).
│   ├── fstab.rs               # UNCHANGED (local-drive UDisks2 fstab helper, untouched).
│   ├── nixos.rs               # EXTENDED: add network_config_snippet_message(mount_point, source,
│   │                         # fs_type, credentials_path) -> String, producing a
│   │                         # `fileSystems."<mp>" = { device = "..."; fsType = "cifs"|"nfs";
│   │                         # options = [ "credentials=<path>" "_netdev" ... ]; };` snippet -
│   │                         # reusing the existing credentials file the helper already wrote at
│   │                         # /etc/drivedock/credentials/<name>.cred (see §3.5 - that path is NOT
│   │                         # managed/regenerated by NixOS, so it survives `nixos-rebuild` even
│   │                         # though /etc/fstab does not; the snippet can safely reference it
│   │                         # as-is rather than asking the user to re-enter credentials).
│   └── shares.rs             # REWRITTEN (not deleted): the JSON-file + systemd --user re-mount-on-
│                             # login workaround is now OBSOLETE and removed - a real /etc/fstab
│                             # entry with `_netdev` mounts automatically at boot via the kernel/
│                             # systemd fstab generator, exactly like local drives, with no
│                             # DriveDock-side re-mount machinery needed at all. This file becomes a
│                             # thin re-export/compat shim during Phase 2 or is deleted outright if
│                             # nothing else references it - Phase 2 to confirm via grep for
│                             # `dock::shares::` call sites (currently: `main.rs`'s
│                             # `--remount-shares` flag and `ui/window.rs`). Both should be removed
│                             # in Phase 2 as dead code made obsolete by this design, not left as
│                             # orphaned stubs.
└── main.rs                   # `--remount-shares` headless flag REMOVED (no longer meaningful -
                              # nothing to re-mount at userspace login now that shares are real
                              # kernel fstab mounts brought up at boot).

data/
└── polkit/
    └── org.example.DriveDock.mount-helper.policy.in   # NEW. Polkit action definition template
                                                          # (see §3.5); `@HELPER_PATH@` substituted
                                                          # at Nix build time.

src/bin/
└── drivedock-mount-helper.rs   # NEW second Cargo [[bin]] target - the privileged helper (§3.4).
```

### 3.4 The privileged helper: `drivedock-mount-helper`

A single small binary, root-executed only via `pkexec`, with **no GTK/gio/glib dependency** (keeps the
privileged attack surface minimal and auditable) — only `std`, `libc` (already a dependency, used for
`flock`), and `serde`/`serde_json` (already dependencies) for its request contract.

**Invocation contract** (all secrets travel over stdin as a single JSON object, never as argv or an
env var — argv is visible to any local user via `ps`/`/proc/<pid>/cmdline`, env vars are visible via
`/proc/<pid>/environ` to root and sometimes to the owning user; stdin is not):

```
pkexec /nix/store/.../libexec/drivedock/drivedock-mount-helper <mount|unmount|persist|unpersist>
```

stdin JSON (varies slightly by subcommand, but shares this shape):
```json
{
  "share_name": "media-fileserver",
  "fs_type": "cifs",
  "source": "//fileserver/media",
  "mount_point": "/mnt/media-fileserver",
  "options": "uid=1000,gid=1000,file_mode=0644,dir_mode=0755",
  "username": "alice",
  "password": "•••",
  "domain": ""
}
```

Helper responsibilities, per subcommand:

- **`mount`**: (1) validate `mount_point` is a slug-sanitized child of a hardcoded base directory
  (`/mnt/`) — reject any path containing `..`, not starting with `/mnt/`, or resolving outside it, to
  prevent an arbitrary-path root write via a compromised/buggy caller; (2) `mkdir -p` the mount point
  if absent; (3) if `username`/`password` present, write
  `/etc/drivedock/credentials/<slug>.cred` (mode `0600`, owner `root:root`, contents
  `username=...\npassword=...\ndomain=...\n`); (4) check `/proc/mounts` first — if already mounted at
  that point with a matching source, no-op success (idempotent); (5) otherwise exec
  `/sbin/mount.cifs <source> <mount_point> -o <options>,credentials=<cred-path>` (CIFS) or
  `/sbin/mount.nfs <source> <mount_point> -o <options>` (NFS) via `std::process::Command` with an
  argv array — never a shell string — so no injection surface even though `options`/`source` originate
  from the (untrusted) GUI process.
- **`unmount`**: `umount <mount_point>` (idempotent: no-op if already unmounted per `/proc/mounts`).
- **`persist`**: idempotent `/etc/fstab` read-modify-write: acquire `flock(LOCK_EX)` on the fstab file
  itself, read full contents, strip any pre-existing line tagged `# drivedock:<slug>`, append a new
  line `<source> <mount_point> <fs_type> <options>,credentials=<cred-path>,_netdev 0 0  # drivedock:<slug>`,
  write to a temp file in `/etc/` and `rename()` atomically over `/etc/fstab` (never a partial-write
  window). **`_netdev` is mandatory** — without it, systemd's fstab generator does not know to order
  the mount after network availability and can hang boot waiting on an unreachable share (confirmed
  standard practice in every mount.cifs/fstab guide consulted, §5 sources).
- **`unpersist`**: same lock+atomic-rewrite, removing the tagged line; also deletes the credentials
  file at `/etc/drivedock/credentials/<slug>.cred` if present (no orphaned secrets).

Idempotency-relevant logic (line generation/matching, path-slug sanitization, tag-comment
parsing) is written as pure, unit-testable functions separate from the actual privileged syscalls —
`cargo test` can exercise all of it without root, mirroring how `is_critical_mount()` was kept
independently testable pre-pivot. The actual `mount(2)`/file-write paths are exercised manually
(flagged §7 — cannot run as root in the CI/preflight environment).

### 3.5 Polkit policy + Nix packaging integration

`data/polkit/org.example.DriveDock.mount-helper.policy.in`:
```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE policyconfig PUBLIC "-//freedesktop//DTD PolicyKit Policy Configuration 1.0//EN"
 "http://www.freedesktop.org/software/polkit/policyconfig.dtd">
<policyconfig>
  <action id="org.example.DriveDock.mount-network-share">
    <description>Mount or persist a network share system-wide</description>
    <message>Authentication is required to mount a network share for all users</message>
    <defaults>
      <allow_any>auth_admin_keep</allow_any>
      <allow_inactive>auth_admin_keep</allow_inactive>
      <allow_active>auth_admin_keep</allow_active>
    </defaults>
    <annotate key="org.freedesktop.policykit.exec.path">@HELPER_PATH@</annotate>
  </action>
</policyconfig>
```
`auth_admin_keep` (not `yes`) deliberately — this writes a systemwide, persistent `/etc/fstab` entry
and root-owned credentials file, the same admin-auth weight UDisks2 itself requires for *non-removable*
`Filesystem.Mount` (only removable media gets `allow_active: yes` in UDisks2's own default policy).
Consistent, not weaker than the existing local-drive precedent.

`@HELPER_PATH@` **must** be substituted at Nix build time with the helper's actual `$out/libexec/...`
store path (unknown until build) — pkexec resolves an action's target binary via the
`org.freedesktop.policykit.exec.path` annotation, not the argv path passed by the caller; if this
annotation is missing/mismatched, pkexec falls back to requiring the far heavier
`org.freedesktop.policykit.pkexec.run-program-as-root` action. `nix/package.nix` performs this via
`pkgs.replaceVars` (or `substituteAll`) in a `postPatch`/dedicated derivation step, standard practice
mirrored by how NetworkManager/udisks2/gparted install their own `.policy` files in nixpkgs.

`nix/package.nix` changes:
- Add `cifs-utils` and `nfs-utils` to `propagatedBuildInputs` (alongside existing `gvfs`) — they
  provide `/sbin/mount.cifs`/`/sbin/mount.nfs`, which the helper execs by absolute path (helper does
  **not** rely on `$PATH` for the actual mount call — avoids a `$PATH`-hijack privilege-escalation
  surface in a root-executed binary — but does need `mount.cifs`/`mount.nfs` present on the system at
  those absolute paths, which `cifs-utils`/`nfs-utils` install at `/sbin` or the Nix-store-relative
  equivalent depending on distro; on NixOS these land under
  `/run/current-system/sw/bin/mount.cifs` via `environment.systemPackages`, so the helper should locate
  them via `which`-equivalent `PATH` search restricted to a fixed, root-owned candidate list
  (`/run/current-system/sw/bin`, `/usr/sbin`, `/sbin`) rather than the caller's inherited `$PATH` —
  Phase 2 implementation detail, flagged here so it isn't missed).
- `postInstall`: move `$out/bin/drivedock-mount-helper` → `$out/libexec/drivedock/drivedock-mount-helper`
  (out of the user-facing `$out/bin`, since it must never be run directly/unprivileged-usefully) and
  install the substituted `.policy` file to `$out/share/polkit-1/actions/org.example.DriveDock.mount-helper.policy`.
- `wrapGAppsHook4` continues to wrap only the main `drivedock` GUI binary; the helper is intentionally
  **not** wrapped with GApps env (it has no GTK deps to wrap).

`nix/module.nix` changes:
- `boot.supportedFilesystems.cifs = lib.mkDefault true;` and `boot.supportedFilesystems.nfs =
  lib.mkDefault true;` when `programs.drivedock.enable` — confirmed via nixpkgs'
  `nixos/modules/tasks/filesystems/cifs.nix`: this is what pulls `pkgs.cifs-utils` into
  `environment.systemPackages` on NixOS specifically and enables the kernel-module list (`cifs`,
  `nls_utf8`, `hmac`, `md4`, `ecb`, `des_generic`, `sha256`) for use — NixOS does **not** enable CIFS/NFS
  kernel support by default in minimal configs, so this must be explicit, not assumed.
- `security.polkit.enable = lib.mkDefault true;` — already true on most desktop NixOS configs via
  `services.udisks2.enable` (which itself depends on polkit), but should be explicit here since this
  feature's authorization now depends on Polkit directly, not only transitively through UDisks2.

### 3.6 Credential security (confirmed convention)

Standard, widely-documented Linux practice (Samba/`mount.cifs`(8) ecosystem, multiple sources §5):
never place a password directly in `/etc/fstab` (world-readable) or on a command line (visible via
`ps`/`/proc/<pid>/cmdline`). Use `credentials=/path/to/file` pointing at a file containing
`username=`/`password=`/`domain=` lines, owned `root:root`, mode `0600`. This is exactly what the
helper implements (§3.4) — DriveDock's unprivileged GUI process never itself holds a credentials file
on disk; it only holds the password in memory long enough to serialize it into the one-shot stdin JSON
sent to the already-authorized helper process, which alone writes the file, as root.

---

## 4. Implementation Steps (ordered, for Phase 2)

1. `Cargo.toml`: add `[[bin]] name = "drivedock-mount-helper" path = "src/bin/drivedock-mount-helper.rs"`.
   No new dependency entries required (helper uses existing `serde`/`serde_json`/`anyhow`/`libc`/`std`).
2. Implement `src/bin/drivedock-mount-helper.rs`: argv subcommand dispatch, stdin JSON parsing, path/
   slug validation (pure + unit-tested), `/proc/mounts` idempotency checks (pure + unit-tested),
   credentials-file writer, `mount.cifs`/`mount.nfs` `Command` invocation, `/etc/fstab` flock+atomic-
   rewrite logic (line generation pure + unit-tested; the flock/rename I/O itself not unit-tested, see
   §7).
3. Rename `src/network/gvfs.rs` → `src/network/discover.rs`; remove `interactive_mount_operation`/
   mount/unmount functions from it; change `NetworkShare.mounted` computation to parse `/proc/mounts`
   for `cifs*`/`nfs*` rows instead of `gio::VolumeMonitor::mounts()` (§3.3).
4. Create `src/network/mount.rs`: credential-collection dialog (reusing existing `AdwAlertDialog` +
   `PasswordEntryRow` layout, decoupled from `gio::MountOperation`), `mount_share`/`unmount_share`/
   `set_persistent` spawning `pkexec <helper>` with JSON over stdin (§3.3).
5. Update `src/network/mod.rs` re-exports to point at `discover`/`mount` instead of `gvfs`.
6. Extend `src/dock/nixos.rs` with `network_config_snippet_message()` (§3.3).
7. Remove `src/dock/shares.rs`'s JSON+systemd-user-unit logic; remove `main.rs`'s `--remount-shares`
   flag and its call site; grep for and remove any now-dead call sites in `src/ui/window.rs`.
8. Add `data/polkit/org.example.DriveDock.mount-helper.policy.in`.
9. Update `nix/package.nix` (helper build/install, `.policy` substitution+install, `cifs-utils`/
   `nfs-utils` propagation) and `nix/module.nix` (`boot.supportedFilesystems.{cifs,nfs}`,
   `security.polkit.enable`) per §3.5.
10. Update `src/ui/window.rs` to call the new `network::mount::*` functions instead of the removed
    GVfs-based ones; surface the returned `DockResult`/Status message (including the NixOS snippet)
    exactly as the existing local-drive flow already does.
11. `cargo check` / `cargo build` / `cargo test` iteratively until green (per CLAUDE.md Phase 3
    validation commands — no FORBIDDEN COMMANDS).
12. Update `README.md`/`docs/BUILDING.md` to document: the new runtime deps (`cifs-utils`, `nfs-utils`,
    a running Polkit authentication agent), the `/mnt/<name>` mount-point convention, and the
    NixOS `boot.supportedFilesystems` requirement.

---

## 5. Dependencies

| Crate | Version | Purpose | Notes |
|---|---|---|---|
| *(none new)* | — | Helper uses only already-present `serde`/`serde_json`/`anyhow`/`libc`/`std` | No `zbus`/D-Bus dependency added — Option 3 (systemd D-Bus) was evaluated and rejected (§3.1), so no new D-Bus proxy code or crate is needed. |

Nix-side (runtime, not Cargo): `cifs-utils` (provides `/sbin/mount.cifs`), `nfs-utils` (provides
`/sbin/mount.nfs`) — both standard, widely-packaged, reasonable runtime deps, confirmed present in
nixpkgs and referenced directly by NixOS's own `cifs.nix` module. `polkit`/`security.polkit` — already
an implicit dependency via `services.udisks2.enable` in the existing module; made explicit here since
this feature depends on it directly for its own action, not only transitively.

Sources consulted (≥6, per CLAUDE.md Phase 1 policy):
1. `org.freedesktop.systemd1(5)` manual page / freedesktop.org systemd docs — `StartTransientUnit`
   semantics, transient-vs-persistent unit distinction.
2. systemd/systemd GitHub issue #17224 — `StartTransientUnit` Polkit authorization request behavior.
3. ArchWiki "Polkit" — `allow_active`/`auth_admin_keep` semantics, default `org.freedesktop.systemd1.
   manage-units` policy.
4. `storaged-project/udisks` GitHub issues #421, #1323, and `udiskslinuxfilesystem.c` (source read) —
   confirms `Filesystem` interface is `Block`-object-only, no CIFS/NFS mount capability.
5. jensd.be "Mount Windows (CIFS) shares on Linux with credentials in a secure way" — `credentials=`
   file convention, `0600` permissions.
6. `mount.cifs`(8) man page (samba.org / linux.die.net mirrors) — credentials file format
   (`username=`/`password=`/`domain=`), option semantics.
7. Ubuntu Community Help Wiki "MountCifsFstabSecurely" — corroborating `/etc/fstab` credentials-file
   best practice, independent source.
8. `NixOS/nixpkgs` `nixos/modules/tasks/filesystems/cifs.nix` (source read) — confirms
   `boot.supportedFilesystems.cifs` gates `cifs-utils` inclusion and kernel-module list; NixOS does not
   enable CIFS by default.
9. NixOS Wiki "Samba" — `fileSystems.<name>.options` with `credentials=` pattern for declarative CIFS
   mounts.
10. Prior in-repo spec `nix-pivot-scope-narrow_spec.md` §3.3/§3.4 (re-verified, not blindly trusted) —
    UDisks2/`Block`-only scope, NixOS `/etc/fstab` full-regeneration behavior on `nixos-rebuild`.

Context7 was not applicable here: no *new* external Cargo crate is being added (§5 table), so the
Context7 dependency-verification requirement ("before adding any new dependency") does not trigger. The
only new "dependencies" are Nix/system runtime packages (`cifs-utils`, `nfs-utils`, Polkit), which
Context7 does not index (it covers library/SDK documentation, not distro packages) — verified instead
via nixpkgs source and upstream man pages as listed above, consistent with the fallback approach the
prior nix-pivot spec used for `gio` (which Context7 also does not cover).

---

## 6. Configuration Changes

- New system paths introduced (all created/managed exclusively by the root-executed helper, never by
  the unprivileged GUI process): `/mnt/<share-slug>` (mount points), `/etc/drivedock/credentials/
  <share-slug>.cred` (root:root, 0600), and tagged lines in `/etc/fstab` (`# drivedock:<share-slug>`).
- New Polkit action: `org.example.DriveDock.mount-network-share`, `auth_admin_keep` for all subjects
  (§3.5).
- `nix/module.nix` gains `boot.supportedFilesystems.cifs`/`.nfs` and explicit `security.polkit.enable`.
- `main.rs` loses the `--remount-shares` headless flag (obsoleted, §3.3).

---

## 7. Risks and Mitigations

1. **pkexec requires a running Polkit authentication agent** (e.g. `polkit-gnome-authentication-agent-1`
   or the desktop shell's built-in one) to show a graphical password prompt; on a minimal/non-desktop
   NixOS install without one, `pkexec` falls back to a text prompt on the invoking terminal, which is
   broken for a GUI app launched from an app menu (no controlling terminal). **Not a new risk** — the
   original (pre-pivot) stub design already had this exact dependency for its raw `pkexec mount`
   calls, and UDisks2/Polkit already carries it implicitly for local-drive admin operations.
   **Mitigation**: document the requirement in `README.md`/`docs/BUILDING.md`.
2. **Root-executed helper is new privileged attack surface.** **Mitigation**: keep it minimal (no GTK/
   gio/glib deps), validate all paths against a hardcoded `/mnt/` base with slug sanitization, use
   `Command` argv arrays (never shell strings) for `mount.cifs`/`mount.nfs`, resolve those binaries via
   a fixed root-owned candidate-path list rather than the caller's `$PATH`, and keep the JSON contract
   as the only untrusted-input surface (never argv/env for secrets).
3. **`/etc/fstab` concurrent-write race.** **Mitigation**: `flock(LOCK_EX)` + atomic `rename()` over a
   same-directory temp file, replicated by hand since (unlike local drives) no UDisks2-equivalent
   D-Bus service exists to delegate this to for network shares — scoped tightly to a single tagged-line
   read-modify-write, not a general-purpose file editor.
4. **Credential-file lifecycle / orphaned secrets.** **Mitigation**: `unpersist` subcommand always
   deletes the corresponding `/etc/drivedock/credentials/<slug>.cred`; Phase 2/3 review should verify
   this path is actually reached on every un-toggle code path, including error/retry paths.
5. **Cannot exercise the actual privileged mount/fstab-write path via `cargo test`** (needs root + a
   real mountable target). **Mitigation**: keep all decision logic (slug sanitization, fstab line
   generation/matching, idempotency checks against a given `/proc/mounts` snapshot) as pure functions
   under unit test; the privileged I/O itself is a documented manual/Phase-3 environment limitation
   (consistent with how CLAUDE.md already handles the "GTK headers unavailable" case) — not a build
   defect, must be reported to the user as a testing boundary, not silently assumed correct.
6. **`mount.cifs`/`mount.nfs` binary resolution differs per distro** (`/sbin` vs `/usr/sbin` vs Nix's
   `/run/current-system/sw/bin` on NixOS). **Mitigation**: helper searches a fixed, ordered candidate
   list (not the inherited `$PATH`) covering all three; Phase 2 must verify this list against an actual
   NixOS `nix build`/`nix run` smoke test once flake-native validation is performed (flagged as an
   existing blind spot in the prior Phase 3 review, still open).
7. **Polkit `.policy` `exec.path` staleness across DriveDock upgrades** — the substituted path is a
   specific Nix store path; an upgrade produces a new store path and a new `.policy` file replacing the
   old one via the normal NixOS activation/generation-switch process (same mechanism as any other
   declaratively-installed file), so this is expected to self-heal on `nixos-rebuild switch`, but should
   be explicitly smoke-tested in Phase 2/3 rather than assumed.
8. **Scope discipline**: this spec deliberately does not touch local-drive UDisks2 logic, GVfs
   discovery, or introduce any secrets-management framework (e.g. `sops-nix`/`agenix`) — the NixOS
   snippet in §3.3 points at the already-root-written `/etc/drivedock/credentials/<slug>.cred` file
   as a pragmatic default, and explicitly does not attempt to integrate with declarative secrets
   tooling, per the task's own instruction to avoid scope creep into secrets management.

---

## 8. Open Question for the User (flag, not silent resolution)

None blocking — the destination and mechanism are both decided per this spec. One item worth the
user's awareness before Phase 2 starts: removing `src/dock/shares.rs`'s JSON+systemd-user-unit
mechanism and `main.rs`'s `--remount-shares` flag is a **behavior-visible deletion**, not just an
internal refactor — if the user (or `vexos-nix`) already relies on the generated
`~/.config/systemd/user/drivedock-remount.service` unit today, upgrading will silently stop generating
it. Recommend calling this out explicitly in the Phase 2 implementation summary/commit message, not
just in code comments.
