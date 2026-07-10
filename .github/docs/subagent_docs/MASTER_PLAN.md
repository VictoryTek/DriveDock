# DriveDock — Master Plan

Consolidated from `ANALYSIS_ARCH.md`, `ANALYSIS_BUGS.md`, and `ANALYSIS_FEATURES.md`
(2026-07-10 analysis passes). Duplicate findings reported by more than one pass are merged into
a single line below, with all source references kept. Ordering: **High** priority first
(security/correctness before docs before packaging/features), then **Medium**, then **Low**.
Within a tier, order is roughly execution order, not strict severity.

Checkboxes are updated as work completes. This file is the single source of truth for
outstanding work going forward — the three `ANALYSIS_*.md` files remain as detailed reference
but should not be re-triaged independently.

---

## High Priority

- [ ] **fstab line injection via unvalidated `source`/`mount_point`/`fs_type`/`options` in the privileged helper.**
  Newline in any field writes an arbitrary attacker-controlled `/etc/fstab` line at next boot — a
  local-root persistence primitive gated only by one cached Polkit auth.
  *Source: BUGS 2.1.* `src/bin/drivedock-mount-helper.rs:390-417` (`build_fstab_line`), `:230-253`
  (`cmd_persist`), `:52-66` (request parsing).

- [ ] **"Permanently dock" on a share discards the credentials the user just typed.**
  `cmd_persist` never writes a credentials file from `request.username/password/domain` — it only
  reuses one left over from a prior `mount`. Persisting a share not currently mounted produces a
  boot-time fstab entry with no `credentials=`, so it fails at boot.
  *Source: BUGS 1.1.* `src/ui/window.rs:494-501` → `src/network/mount.rs:217-244` →
  `src/bin/drivedock-mount-helper.rs:230-253`.

- [ ] **Synchronous `pkexec`/mount subprocess blocks the GTK main loop.**
  `run_helper` blocks inside `async fn`s spawned via `glib::spawn_future_local` — the whole window
  freezes from click until the Polkit dialog is answered and the mount returns. Fix direction:
  `gio::Subprocess` (already an available dep via `gio`) rather than `std::process::Command`;
  resolve the never-used `tokio`/`tokio-runtime` dependency (see Medium) as part of this fix —
  either wire it up or delete it once `gio::Subprocess` is in place.
  *Source: ARCH 1.1 = BUGS 3.1; implementation direction also FEATURES 1.3.*
  `src/network/mount.rs:118-151`; call sites `src/ui/window.rs:494`, `:536`, `:575`.

- [ ] **Error-path checkbox revert re-fires `connect_toggled`, launching the opposite privileged operation.**
  `set_active(!enable)` in the failure path emits `toggled` again, spawning a second
  UDisks2/helper call in the opposite direction (a second Polkit prompt for shares) undoing
  something that never happened. Needs a reentrancy guard or `block_signal`/`unblock_signal`.
  *Source: ARCH 1.2 = BUGS 1.2.* `src/ui/window.rs:339`, `:507`.

- [ ] **"Permanently dock" checkbox never reflects real fstab/UDisks2 state — implement read-back.**
  The write path is fully built (UDisks2 `Block.Configuration` for local drives, tagged
  `# drivedock:<slug>` fstab lines for shares); the checkbox always renders unchecked regardless
  of actual state. Fix: during `refresh_drives`, query `Block.Configuration` via the existing
  `Udisks` wrapper for local drives, and scan `/etc/fstab` for the tag for shares (mirror
  `strip_tagged_line`'s anchor logic into an `is_tagged()` sibling); initialize the checkbox with
  its signal blocked (this also depends on the reentrancy-guard fix above being in place first).
  *Source: ARCH 1.3/4.1; constructive spec in FEATURES 1.1.*
  `src/ui/window.rs:290-294`, `:476-479`; `src/udisks.rs:145-149`;
  `src/bin/drivedock-mount-helper.rs` (`strip_tagged_line`).

- [ ] **No percent-decoding / `/proc/mounts` octal-unescaping — shares with spaces or non-ASCII paths can't mount or be matched.**
  GVfs URIs are percent-encoded (`My%20Share`); `/proc/mounts` octal-escapes spaces as `\040`.
  Neither is decoded anywhere, so `mount.cifs` gets the literal encoded name and fails, and a
  correctly mounted share never matches so `mounted` stays `false` (stuck showing "Dock" forever,
  and re-mount attempts stack on top of the real one).
  *Source: BUGS 1.3.* `src/network/discover.rs:154-165` (`mount_source_from_uri`), `:169-185`
  (`is_network_mount_active`); `src/bin/drivedock-mount-helper.rs:355-370` (`is_mounted`).

- [ ] **Root `BUILDING.md` and `docs/BUILDING.md` are near-duplicates that have already diverged.**
  Root copy lacks the mounting-prerequisites section and still lists the deleted
  `src/network/gvfs.rs`. README links only to `docs/BUILDING.md`. Delete the root copy or reduce
  it to a one-line pointer.
  *Source: ARCH 2.4.* `BUILDING.md` vs `docs/BUILDING.md`.

- [ ] **`docs/PROJECT_STATUS.md` documents a `--remount-shares` headless mode that was never built.**
  Leftover from the abandoned GVfs-era re-mount-on-login design, superseded by the fstab/`_netdev`
  approach. Delete the doc reference rather than build the feature (explicitly not recommended in
  FEATURES).
  *Source: ARCH 4.2.* `docs/PROJECT_STATUS.md:29-30`.

- [ ] **`CLAUDE.md` describes the pre-pivot (Flatpak-era) codebase.**
  Says the project is Flatpak-packaged, references a deleted `org.example.DriveDock.yml` manifest
  and nonexistent `src/network/smb.rs`/`nfs.rs`, describes `system/local.rs` as parsing
  `/proc/mounts` (it explicitly doesn't — uses `gio::VolumeMonitor`), and calls mount/persist
  "async-ready stubs" (both are fully implemented). As the standing instruction file for AI
  tooling on this repo, its staleness actively misdirects future work.
  *Source: ARCH 4.3.* `CLAUDE.md` ("Project Context", "Repository Notes" sections);
  `.github/copilot-instructions.md` where it overlaps.

- [ ] **App identity: no desktop file, no installed icon, placeholder `org.example.*` app/Polkit IDs.**
  `assets/drivedock.png` is committed but never installed; no `.desktop` file exists anywhere;
  the GTK app ID and Polkit action ID are both `org.example.DriveDock`. As packaged, the app has
  no menu entry and no icon. Settle on a real ID (e.g. `io.github.victorytek.DriveDock`) across
  `main.rs`, the Polkit policy filename/action ID, and `nix/package.nix`; add and install a
  `.desktop` file; install the icon to the hicolor theme path.
  *Source: FEATURES 1.4 (absorbs ARCH 2.5, the ID-inconsistency finding).*
  `src/main.rs:10`; `data/polkit/org.example.DriveDock.mount-helper.policy.in`;
  `nix/package.nix` (postInstall); `assets/drivedock.png`.

- [ ] **Live auto-refresh from `VolumeMonitor` signals.**
  `list_local_drives` already holds the `gio::VolumeMonitor` that emits `volume-added`/
  `mount-removed`/etc.; only polled on manual refresh today. Keep one long-lived monitor, connect
  the signals, debounce ~500ms, and call the existing `refresh_drives` future. Plugging in a USB
  drive should appear without a click.
  *Source: FEATURES 2.1.* `src/system/local.rs:69`; `src/ui/window.rs` (`refresh_drives`,
  `build_content`).

- [ ] **"Open in Files" action on mounted rows.**
  Every mounted row already knows its path (`mount_point: PathBuf` for local drives,
  deterministic `/mnt/<slug>` for shares). Add a folder-open suffix button using
  `gtk::FileLauncher` (available under the already-declared `v4_12` feature).
  *Source: FEATURES 2.2.* `src/system/local.rs:20`; `src/network/mount.rs:70-72`;
  `src/ui/window.rs` (`create_local_drive_row`, `create_network_share_row`).

- [ ] **Connect to a network share by address (manual URI entry).**
  Discovery is passive GVfs browsing only and can be empty (server off-subnet, Avahi disabled,
  gvfsd not running). The mount path itself takes any `NetworkShare`; add a "+" header-bar button
  opening a small dialog (reuse the `collect_credentials` dialog shape) for a URI like
  `smb://nas.local/media`, construct a `NetworkShare` via the existing `protocol_from_uri`, and
  run the existing dock flow.
  *Source: FEATURES 2.3.* `src/network/discover.rs:53-73`, `:154-165`; `src/network/mount.rs:278-336`.

---

## Medium Priority

- [ ] Cancelling the credentials dialog proceeds with an anonymous/guest mount instead of aborting
  (`Cancelled` and `NoCredentials` are collapsed into one `None` value).
  *Source: ARCH 1.4 = BUGS 1.7.* `src/network/mount.rs:278-336`; `src/ui/window.rs:495-501`, `:576-582`.

- [ ] `window.rs` is a 600-line procedural god-module with no list/data model; the two row builders
  are ~150-line near-duplicates that must be hand-copied for every new per-row action.
  *Source: ARCH 1.5.* `src/ui/window.rs` (whole file, esp. `:233-441`, `:457-600`).

- [ ] Duplicated `/proc/mounts` parsing, fs-type list, and `slugify` between the GUI and the
  helper binary (two independent copies each); both copies also carry a `cifs2` typo (not a real
  kernel fstype) and both miss `smb3`.
  *Source: ARCH 1.6.* `src/network/discover.rs:167-185`; `src/network/mount.rs:51-56`;
  `src/bin/drivedock-mount-helper.rs:308-313`, `:355-370`.

- [ ] `unmount_share` silently defaults an unknown protocol to `"cifs"` while `mount_share` and
  `set_persistent` correctly error — three inconsistent handlers for the same invalid input.
  *Source: ARCH 1.7 = BUGS 1.9 (priority reconciled to Medium).* `src/network/mount.rs:187`.

- [ ] `src/udisks.rs` sits as a root-level orphan while every other domain gets its own directory,
  blurring the `system`/`dock` boundary.
  *Source: ARCH 2.1.* `src/udisks.rs` vs `src/system/`, `src/network/`, `src/dock/`.

- [ ] Local-drive mount/unmount/persist are split three ways (raw calls inline in the UI layer,
  `system::unmount`, `dock::mod`) at three different abstraction levels, unlike network shares
  where all three verbs live together in one module.
  *Source: ARCH 2.2.* `src/ui/window.rs:415-420`; `src/system/unmount.rs`; `src/dock/mod.rs`.

- [ ] Three different error-handling conventions in one small crate (`thiserror` enum nobody
  matches on, `anyhow` everywhere else, `Result<(), String>` in the helper) with no stated rule.
  *Source: ARCH 2.3.* `src/system/unmount.rs:8-24`; `src/udisks.rs`; `src/network/mount.rs`;
  `src/bin/drivedock-mount-helper.rs`.

- [ ] Local drives (via `gio::VolumeMonitor`) and network shares (via raw `/proc/mounts` parse)
  report mounted-state from two different sources of truth with no reconciliation — a
  helper-mounted share can also surface in the local list on some systems, appearing twice.
  *Source: ARCH 3.1.* `src/system/local.rs:97-108`; `src/network/discover.rs:63-69`.

- [ ] Share display names with no ASCII alphanumerics (e.g. non-Latin names) slug to empty string,
  producing a hard, ungraceful error instead of a fallback or clear message.
  *Source: BUGS 1.4.* `src/network/mount.rs:51-56`, `:70-72`; `src/bin/drivedock-mount-helper.rs:87-89`.

- [ ] Slug collisions: distinct shares (different hosts, names that normalize the same way) can
  silently share one mount point, one credentials file (second overwrites first share's stored
  password), and one fstab tag.
  *Source: BUGS 1.5.* `src/network/mount.rs:51-56`; `src/bin/drivedock-mount-helper.rs:308-313`,
  `:416`, `:428-435`.

- [ ] Local "permanently dock" persists the drive's *current* session mount point
  (`/run/media/<user>/<label>`), which doesn't exist at boot — the primary removable-media use
  case for this feature is the one most likely to fail.
  *Source: BUGS 1.6.* `src/ui/window.rs:322-329`; `src/udisks.rs:111-135`.

- [ ] Mounted-but-currently-undiscovered shares are invisible and cannot be undocked from the UI
  (discovery-only list, no merge of active `/proc/mounts` rows under `/mnt/` that discovery
  missed). Fix as a feature: synthesize a `mounted: true` entry for any such row.
  *Source: BUGS 1.8, constructive spec in FEATURES 2.4.* `src/network/discover.rs:53-73`, `:63`.

- [ ] `is_critical_mount` protects `/`, `/boot`, `/usr`, `/var`, `/home` but not `/nix` (or
  `/nix/store`) — unmounting it is as destructive as unmounting `/usr`, on the project's own
  primary target OS.
  *Source: BUGS 1.12.* `src/system/local.rs:159-163`.

- [ ] Network mounts are made without forcing `nosuid,nodev,noexec` — a malicious/compromised
  SMB/NFS server can serve a setuid-root binary that any local user can then execute for root.
  *Source: BUGS 2.2.* `src/bin/drivedock-mount-helper.rs:134-152`, `:390-417`;
  `src/network/mount.rs:174`, `:199`, `:238`.

- [ ] Plaintext credentials file (`/etc/drivedock/credentials/<slug>.cred`) is written before the
  mount attempt and never cleaned up on mount failure or on plain unmount — only `unpersist`
  deletes it, so a one-off (never persisted) mount leaves a password on disk indefinitely.
  *Source: BUGS 2.3.* `src/bin/drivedock-mount-helper.rs:122-125`, `:203-224`, `:255-264`.

- [ ] `/etc/fstab` rewrite has no `fsync` before rename (crash can corrupt fstab → unbootable
  system) and the `flock` is taken on the pre-rename inode, so a second helper instance can race
  past a third and lose an update.
  *Source: BUGS 2.5.* `src/bin/drivedock-mount-helper.rs:270-300` (`with_locked_fstab`).

- [ ] `tokio`/`tokio-runtime` is declared but never referenced anywhere in `src/` — decide its
  fate as part of the async-subprocess fix above (wire it up via `tokio::process`, or delete it
  if `gio::Subprocess` is used instead).
  *Source: ARCH 5.1 = BUGS 4.1; see also FEATURES 1.3.* `Cargo.toml:41`, `:55-60`.

- [ ] UUID-based UDisks2 block lookup silently coerces a D-Bus error into "not found"
  (`.into_iter().next()` on a `Result` swallows `Err`), masking a transient failure behind a
  fallback full-object-manager scan whose own failure message hides the real cause.
  *Source: BUGS 5.1.* `src/udisks.rs:43-51`.

- [ ] D-Bus/UDisks2 error classification is done by substring-matching a formatted error string
  (`msg.contains("busy")`, `contains("NotAuthorized")`) instead of matching the structured D-Bus
  error name — fragile, and currently unused anyway since no caller matches on the resulting enum.
  *Source: BUGS 5.2.* `src/system/unmount.rs:66-75`.

- [ ] Add a minimal, safe options UI (Read-only → `ro`, "Mount as my user" → `uid=/gid=`) to use
  the `options` field that is already fully plumbed GUI→helper→mount→fstab but always sent empty.
  No free-text entry (that would reopen the injection surface fixed above).
  *Source: FEATURES 1.2; related dead-plumbing note BUGS 4.3.* `src/network/mount.rs:36`, `:174`,
  `:199`, `:238`; `src/bin/drivedock-mount-helper.rs:134-140`, `:398-403`.

- [ ] Copy button for the NixOS `fileSystems` config snippet shown in the status area — currently
  requires manually selecting multi-line wrapped text. Split the message functions into
  `(prose, snippet)` and add a clipboard button.
  *Source: FEATURES 2.5.* `src/dock/nixos.rs`; `src/ui/window.rs:160` (`StatusHandle`).

- [ ] Remember SMB credentials via the Secret Service keyring (`oo7` crate) so users aren't
  re-prompted on every mount of the same share — session convenience only, not a replacement for
  the helper's own root-owned credentials file.
  *Source: FEATURES 2.6.* `src/network/mount.rs:278-336`.

- [ ] Eject/power-off a removable drive after a successful undock, via UDisks2's `Drive` interface
  (`can-power-off`/`PowerOff`) — "undock" currently leaves the device spun up.
  *Source: FEATURES 3.1.* `src/udisks.rs:95-104`; `src/ui/window.rs:444-454` (`icon_for_drive`).

- [ ] Busy/in-flight feedback (spinner + message) during dock/undock/persist operations — depends
  on the async-subprocess fix above to be meaningful, since operations are currently blocking.
  *Source: FEATURES 3.2.* `src/ui/window.rs:27-42` (`StatusHandle`).

- [ ] NixOS module option `programs.drivedock.shares` to declaratively pre-configure shares,
  giving the in-app "add this to your config" guidance a real typed target instead of raw text to
  hand-copy.
  *Source: FEATURES 4.1.* `nix/module.nix`; `src/dock/nixos.rs`.

---

## Low Priority

- [ ] `dock::fstab` is a pure one-line pass-through over `crate::udisks` with no logic of its own.
  *Source: ARCH 1.8.* `src/dock/fstab.rs:16-31`.

- [ ] `mount_share`/`unmount_share`/`set_persistent` are marked `async` but contain no real
  `.await` on I/O — the keyword currently just signals "called from the UI", masking that they
  block synchronously (see High: blocking subprocess fix).
  *Source: ARCH 2.6.* `src/network/mount.rs:155`, `:186`, `:217`.

- [ ] Single mutable status line produces contradictory copy on partial/total discovery failure
  (e.g. "No drives or shares found" shown at the same time as a scan-failed error).
  *Source: ARCH 3.2 = BUGS 5.3.* `src/ui/window.rs:186-221`.

- [ ] NixOS/OtherLinux guidance message strings are duplicated verbatim between `dock::mod` and
  `network::mount` instead of living in one shared function.
  *Source: ARCH 3.3.* `src/dock/mod.rs:55-66`; `src/network/mount.rs:255-265`.

- [ ] `test_is_critical_mount` is copy-pasted identically into two modules' test blocks.
  *Source: ARCH 3.4 = BUGS 4.4.* `src/system/local.rs:179-187`; `src/system/unmount.rs:85-92`.

- [ ] `is_network_uri` is dead code kept behind `#[allow(dead_code)]` with a speculative comment,
  against the project's own "no speculative code" principle.
  *Source: ARCH 4.4 = BUGS 4.2.* `src/network/discover.rs:136-147`.

- [ ] Commented-out and `#[allow(unused_imports)]`-suppressed re-exports instead of being deleted
  or used.
  *Source: ARCH 4.5 = BUGS 4.5.* `src/ui/mod.rs:3-4`; `src/network/mod.rs:6-9`.

- [ ] `thiserror` is used for exactly one enum whose variants no caller ever matches on — folds
  away if the error-handling convention (Medium, above) is unified to `anyhow`.
  *Source: ARCH 5.2.* `Cargo.toml:45`; `src/system/unmount.rs:8-24`.

- [ ] Full `futures` crate pulled in for a single `oneshot` channel use.
  *Source: ARCH 5.3.* `Cargo.toml:35`; `src/network/mount.rs:280`.

- [ ] `libc` dependency exists solely for one `flock` call in the helper binary, but links into
  the GUI binary too since deps are per-crate not per-target.
  *Source: ARCH 5.4.* `Cargo.toml:38`; `src/bin/drivedock-mount-helper.rs:281`.

- [ ] `serde`/`serde_json` Cargo.toml comment says "Serialization for configuration" — actual use
  is the GUI↔helper request protocol; there is no configuration system.
  *Source: ARCH 5.5.* `Cargo.toml:51-53`.

- [ ] Dead `..` check in `validate_mount_point`'s `Component::Normal` arm
  (`segment == OsStr::new("..")` can never be true; `std::path` parses that as `ParentDir`,
  already rejected by the wildcard arm).
  *Source: BUGS 1.10.* `src/bin/drivedock-mount-helper.rs:339-347`.

- [ ] "X available of Y" label is computed from `filesystem::free`, not actual available space —
  overstates available space by the filesystem's reserved-block margin.
  *Source: BUGS 1.11.* `src/system/local.rs:128-152`; `src/ui/window.rs:273-278`.

- [ ] No newline/control-character rejection on username/domain in the credentials file writer —
  low impact (self-attack only, given the request already carries the real password) but one
  line to close.
  *Source: BUGS 2.4.* `src/bin/drivedock-mount-helper.rs:172-178`.

- [ ] Helper doesn't cross-check that `mount_point == /mnt/<slug-of-share_name>` — the
  slug/credentials-file/fstab-tag/mount-point identity can be desynchronized by a buggy caller.
  *Source: BUGS 2.6.* `src/bin/drivedock-mount-helper.rs:87-98`.

- [ ] `refresh_drives` awaits local and network discovery sequentially (each itself doing
  sequential per-item round trips) and destroys/rebuilds every row on every refresh.
  *Source: BUGS 3.2.* `src/ui/window.rs:172-221`; `src/system/local.rs:97-108`.

- [ ] Every operation reconnects to UDisks2 and re-resolves the helper binary path from scratch
  instead of caching either.
  *Source: BUGS 3.3.* `src/network/mount.rs:87-114`; `src/ui/window.rs:318`, `:417`;
  `src/system/unmount.rs:57`.

- [ ] `collect_credentials` can hang forever (leaking the spawned future, leaving the triggering
  button permanently disabled) if the dialog is ever presented with no parent window.
  *Source: BUGS 5.4.* `src/network/mount.rs:329-335`.

- [ ] Helper's stdout is captured but discarded from error messages — only stderr is included,
  even though `mount.cifs`/`mount.nfs` diagnostics often go to stdout.
  *Source: BUGS 5.5.* `src/network/mount.rs:126`, `:142-147`.

- [ ] `/proc/mounts` read failure during share discovery silently marks every share unmounted,
  with only a log line — no status-line notice, unlike the equivalent local-drive failure path.
  *Source: BUGS 5.6.* `src/network/discover.rs:63-69`.

- [ ] LUKS-encrypted partitions currently fail to dock with a raw "Not a mountable filesystem"
  error instead of prompting to unlock. Defer until the `window.rs` model cleanup (Medium, above)
  lands.
  *Source: FEATURES 3.3.* `src/udisks.rs` (`find_block_object`).

- [ ] `UnmountError::DeviceBusy` is a dead-end error string today — offer a retry/force-unmount
  dialog instead.
  *Source: FEATURES 3.4.* `src/system/unmount.rs:10-11`, `:66-75`.

- [ ] Desktop notifications for operations that complete while the window is unfocused — only
  useful once mounts are properly async (High, above).
  *Source: FEATURES 4.2.* `src/main.rs:20-22`.

- [ ] WebDAV/FTP/SFTP shares are discovered with correct protocol badges but hard-error on Dock
  (kernel-mount design only covers cifs/nfs) — replace Dock with "Open in Files" (GVfs/Nautilus
  hand-off) for those protocols instead.
  *Source: FEATURES 4.3.* `src/network/discover.rs:36-47`; `src/network/mount.rs:156-162`.

---

## Progress

**High:** 0 / 13 complete
**Medium:** 0 / 25 complete
**Low:** 0 / 24 complete
