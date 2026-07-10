# DriveDock — Architecture & Structure Analysis

Date: 2026-07-10. Scope: architecture and structure only (no functional testing; native build is
not possible on this Windows host). Priorities: **High** = actively misleads contributors or is a
structural defect with user-visible consequences; **Medium** = friction/inconsistency that will
cause bugs or wasted work; **Low** = cosmetic or minor hygiene.

---

## 1. Architectural anti-patterns / design problems

### 1.1 Blocking synchronous subprocess calls inside `async fn` on the GTK main thread — **High**
**Files:** `src/network/mount.rs:118-151` (`run_helper`), called from `mount_share` (:155),
`unmount_share` (:186), `set_persistent` (:217); invoked via `glib::spawn_future_local` in
`src/ui/window.rs:536`, `575`, `494`.

`run_helper` uses `std::process::Command` with a blocking `write_all` and `wait_with_output`.
The callers are declared `async fn` and are spawned with `glib::spawn_future_local`, which runs
them **on the GTK main thread**. Nothing in these functions ever awaits around the subprocess, so
the entire UI event loop freezes for the full duration of `pkexec` — which includes the time the
Polkit authentication dialog sits waiting for the user to type a password. This can be tens of
seconds of a completely unresponsive (and, under some compositors, "not responding"-flagged)
window. The `async` signatures are misleading: they promise non-blocking behavior the
implementation does not deliver. Fix direction: use `gio::Subprocess` with async wait, or
`tokio::process` (the dep is already declared, unused — see 5.1), or offload to a thread via
`gio::spawn_blocking`.

### 1.2 Checkbox revert in error path re-triggers the toggled handler — recursive operation firing — **High**
**Files:** `src/ui/window.rs:339` and `src/ui/window.rs:507` (`check_clone.set_active(!enable)`).

`connect_toggled` fires on *any* state change, including programmatic `set_active`. When a
permanent-dock toggle fails, the error path calls `set_active(!enable)` to revert the checkbox —
which re-enters the same `connect_toggled` closure and spawns a **second** helper/UDisks2 call
attempting the opposite operation (with a fresh pkexec prompt for network shares). There is no
guard flag or signal-blocking (`glib::SignalHandlerId` + `block_signal`, or a `Cell<bool>`
reentrancy guard). This is a structural flaw in how UI state and operations are coupled, not just
a bug: state changes and user intent are conflated in one signal.

### 1.3 "Permanently dock" checkbox state is never initialized from reality — **High**
**Files:** `src/ui/window.rs:290-294` (local), `src/ui/window.rs:476-479` (network).

Every refresh builds the checkbox unchecked, regardless of whether an fstab entry (UDisks2
`Configuration` for local drives, `# drivedock:<slug>` tag for shares) already exists. A user who
persisted a share, restarts the app, and sees an unchecked box will check it again (harmless but
confusing) or — worse — will *check-then-uncheck* believing they are toggling off, when the first
check was a no-op re-persist and the uncheck removes the entry. The write path is fully
implemented; the read-back path simply does not exist. This is the clearest half-implemented
feature in the codebase (see also 4.1). The data needed is available: `Block.Configuration` via
the existing `Udisks` wrapper, and `/etc/fstab` is world-readable for tag scanning.

### 1.4 Cancelling the credentials dialog silently proceeds with an unauthenticated mount — **Medium**
**Files:** `src/ui/window.rs:576-582` and `495-501`; `src/network/mount.rs:278-336`.

`collect_credentials` returns `None` both when the user pressed **Cancel** and when no
credentials are needed. The callers pass `None` straight into `mount_share`/`set_persistent`,
which then attempt a guest/anonymous CIFS mount (and for `set_persistent`, write an fstab entry
with no credentials file). "User aborted" and "no credentials" are different intents collapsed
into one value — the type should be something like `Cancelled | Anonymous | Creds(..)`, with
Cancel aborting the operation. As designed, Cancel does not cancel.

### 1.5 `Window` is a 600-line procedural god-module with no data model — **Medium**
**File:** `src/ui/window.rs` (entire file, esp. `create_local_drive_row` :233-441 and
`create_network_share_row` :457-600).

All UI construction, event wiring, async orchestration, and state management live in one struct
of static methods that thread `(group, status)` handles through every closure, each of which must
manually re-clone 4-6 captured values (the clone-dance repeats 7 times). The two row builders are
~150-line near-duplicates of the same structure (badge, permanent toggle with async closure,
dock/undock button with async closure, refresh-on-success). There is no model type
(`gio::ListStore` + `ListBox::bind_model`, or GObject subclassing — the standard gtk4-rs
pattern); rows are imperatively removed and rebuilt on every refresh (:172-182). At the current
size this is workable, but it is the shape that decays fastest: every new per-row action must be
copy-pasted into both builders.

### 1.6 Duplicated `/proc/mounts` parsing and network-fs-type knowledge between GUI and helper — **Medium**
**Files:** `src/network/discover.rs:167-185` (`is_network_mount_active`, `NETWORK_FS_TYPES`) vs
`src/bin/drivedock-mount-helper.rs:355-370` (`is_mounted`, `NETWORK_FS_TYPES`).

Two independent parsers of the same `/proc/mounts` format with the same fs-type list
(`cifs`/`cifs2`/`nfs`/`nfs4`), plus a third duplication of `slugify`
(`src/network/mount.rs:51-56` vs `src/bin/drivedock-mount-helper.rs:308-313`). The helper's
isolation from GTK is a deliberate and good decision (documented at `mount.rs:48-50`), but it does
not require duplication: both binaries are targets of the *same crate*, so shared pure logic can
live in `src/` behind a `lib.rs` (or a tiny internal module included via `#[path]`) with zero
extra dependencies. Duplicated security-relevant logic (what counts as "mounted", what a slug is)
is exactly the kind that drifts: if the GUI's slugify ever diverges from the helper's, credentials
files and fstab tags stop matching mount points.

Also duplicated: the fs-type list `cifs2` looks like a typo carried into both copies — there is no
kernel filesystem named `cifs2` (the SMB fs types are `cifs` and, on newer kernels, `smb3`).
Both parsers therefore also *miss* `smb3` mounts.

### 1.7 `unmount_share` silently defaults unknown protocols to `"cifs"` — **Medium**
**File:** `src/network/mount.rs:187`.

`mount_share` (:156) and `set_persistent` (:222) return a clear error for non-SMB/NFS protocols;
`unmount_share` instead does `fs_type_for_protocol(...).unwrap_or("cifs")` and proceeds. The three
sibling operations handle the identical invalid input three different ways (error, error,
silent-guess). In practice `mount_source_from_uri` will return `None` for those schemes anyway
(:188), so the fallback is dead-but-load-bearing-looking code that misstates the design.

### 1.8 `dock::fstab` is a pure pass-through layer — **Low**
**File:** `src/dock/fstab.rs:16-31`.

Both functions are one-line delegations to `Udisks::add_fstab_entry`/`remove_fstab_entry` with
identical signatures. The module adds a naming layer (`dock::fstab::add_entry`) over an
already-thin wrapper (`crate::udisks`) with no logic of its own — an abstraction for single-use
code. Either the UDisks methods should be called directly from `dock/mod.rs`, or the fstab logic
should actually live here.

---

## 2. Structural inconsistencies (naming, organization, module boundaries)

### 2.1 `src/udisks.rs` is a root-level orphan while every other domain gets a directory — **Medium**
**Files:** `src/udisks.rs` vs `src/system/`, `src/network/`, `src/dock/`, `src/ui/`.

The crate's stated architecture (CLAUDE.md "Repository Notes", PROJECT_STATUS.md) is "modular
binary crate split by domain directories." UDisks2 access — used by `system/unmount.rs`,
`dock/mod.rs`, `dock/fstab.rs`, and `ui/window.rs` — sits as a bare root module. Meanwhile
`system/` contains only local-drive concerns. Either `udisks.rs` belongs under `system/` (it only
ever touches local block devices, per its own doc comment :9-12), or the "one directory per
domain" convention isn't real. Minor on its own, but it blurs the `system` (local drives) vs
`dock` (persistence) boundary — see 2.2.

### 2.2 The `system` / `dock` / `udisks` boundary splits one concern across three modules — **Medium**
**Files:** `src/system/unmount.rs`, `src/dock/mod.rs`, `src/dock/fstab.rs`, `src/udisks.rs`,
`src/ui/window.rs:415-420`.

Local-drive *unmount* lives in `system::unmount` (which internally calls `udisks`). Local-drive
*mount* has no module at all — `window.rs:417-419` calls `Udisks::new()` + `find_block_object` +
`mount` inline in the UI layer. Local-drive *persistence* lives in `dock`. So the three verbs on
the same entity (mount / unmount / persist a local drive) live in the UI layer, `system`, and
`dock` respectively, at three different abstraction levels (raw wrapper calls vs typed-error API
vs message-producing orchestration). Contrast network shares, where all three verbs live together
in `network::mount`. The network side has the right shape; the local side should mirror it
(a `system::mount` or a unified `system::drives` API), and the UI should never construct
`Udisks` objects itself.

### 2.3 Error-handling strategy differs per module with no rule — **Medium**
**Files:** `src/system/unmount.rs:8-24` (typed `thiserror` enum, string-matching on messages
:67-75) vs everything else (`anyhow::Result`): `src/udisks.rs`, `src/network/mount.rs`,
`src/dock/mod.rs`, `src/system/local.rs`; and the helper binary uses `Result<(), String>`
throughout (`src/bin/drivedock-mount-helper.rs:75` etc.).

`UnmountError` is the only typed error in the crate — and no caller ever matches on its variants
(`window.rs:374-384` just formats it), so the classification work (including fragile substring
sniffing of D-Bus error text: `msg.contains("busy")`, `msg.contains("NotAuthorized")` at
`unmount.rs:67-71`) buys nothing. Three conventions for one small crate: pick `anyhow` everywhere
in the GUI (drop `thiserror`, see 5.2), keep `String` in the dependency-free helper — that's a
defensible two-rule split; the current three-way split is not.

### 2.4 `BUILDING.md` exists twice, at root and in `docs/`, already diverged — **High**
**Files:** `BUILDING.md` vs `docs/BUILDING.md`.

The two files are near-copies that have *already* drifted: `docs/BUILDING.md` has the full
"mounting requires cifs-utils/nfs-utils + Polkit agent" prerequisites section (:18-29) and the
current `src/network/discover.rs`+`mount.rs` file listing; root `BUILDING.md` lacks the
prerequisites section and still lists the deleted `src/network/gvfs.rs` in its project-layout
diagram. README.md links only to `docs/BUILDING.md`. The root copy is a stale duplicate and
should be deleted or reduced to a one-line pointer.

### 2.5 Application ID is a placeholder, inconsistent with the published repo identity — **Medium**
**Files:** `src/main.rs:10` (`org.example.DriveDock`),
`data/polkit/org.example.DriveDock.mount-helper.policy.in:5`
(`org.example.DriveDock.mount-network-share`), `nix/package.nix`, `nix/module.nix` comments.

`org.example.*` is the reverse-DNS equivalent of `TODO`. The README installs from
`github:victorytek/drivedock`, so the correct ID (e.g. `io.github.victorytek.DriveDock`) is
derivable today. This matters structurally because the ID is load-bearing in three places that
must change in lockstep (GTK app ID, Polkit action ID + policy filename, Nix install paths) —
the longer it ships, the more places the rename touches (desktop files, icon names, dconf paths).

### 2.6 Inconsistent async-fn honesty: sync functions marked `async` — **Low**
**Files:** `src/network/mount.rs:155`, `:186`, `:217`.

`mount_share`, `unmount_share`, and `set_persistent` contain no `.await` on any I/O (only the
plain-sync `run_helper`); `set_persistent` awaits nothing at all before returning. Compare
`system::unmount::unmount_drive` and `dock::set_permanent_dock`, which are genuinely async
(D-Bus futures). The `async` keyword currently signals "called from the UI" rather than
"non-blocking", which hides problem 1.1. (The compiler doesn't even warn, since the functions
are `await`ed.)

---

## 3. Inconsistent patterns

### 3.1 Local drives and network shares report mounted-state from two different sources of truth — **Medium**
**Files:** `src/system/local.rs:97-108` (gio `VolumeMonitor`/`Mount`) vs
`src/network/discover.rs:63-69` (raw `/proc/mounts` parse).

Local drives trust GIO's view of mounts; network shares deliberately bypass GIO and parse
`/proc/mounts` (necessary, since kernel mounts made by the helper are invisible to GVfs volume
monitoring... except they aren't — GIO's unix mount monitor does surface kernel mounts). The
asymmetry is documented (`discover.rs:8-13`) but has a real consequence: a share mounted by the
helper at `/mnt/<slug>` can *also* appear in `list_local_drives()`' underlying monitor as a unix
mount on some systems, and the two lists are never reconciled or deduplicated before both being
appended to the same `PreferencesGroup` (`window.rs:186-210`). At minimum the refresh path should
filter cifs/nfs fstypes out of the local list explicitly rather than relying on
"no `unix-device` identifier" (:73-77) to exclude them.

### 3.2 Status reporting: errors overwrite each other; partial-failure refresh looks like success — **Low**
**File:** `src/ui/window.rs:186-210`.

`refresh_drives` runs two discovery calls sequentially; if local enumeration fails and network
succeeds, the error is written to the single `StatusHandle` and stays (good), but if *both* fail,
only the second error survives; and a success from a subsequent user action overwrites any
standing error. A single mutable status line is a fine minimal design, but the two branches
handle "some rows added before the error" inconsistently with the `any` placeholder logic
(a failed local scan + empty network scan shows "No drives or shares found" *and* an error —
contradictory copy).

### 3.3 NixOS-guidance message strings duplicated verbatim across modules — **Low**
**Files:** `src/dock/mod.rs:55-60` vs `src/network/mount.rs:255-259` (identical `(Os::NixOs,
false)` paragraph); the `(OtherLinux, _)` strings are also duplicated (:61-66 vs :260-265).

The local-drive and network-share persist paths each own a private copy of the same four
user-facing messages; only the NixOS-enable arm genuinely differs. Copy drift here means the two
halves of the UI explain the same concept differently. The match belongs in one place
(`dock::persistence_message(os, enable, snippet)`), which `network::set_persistent` already half
acknowledges by reaching into `dock::nixos` for the snippet.

### 3.4 Test duplication instead of shared coverage — **Low**
**Files:** `src/system/local.rs:179-187` and `src/system/unmount.rs:85-92`.

`test_is_critical_mount` is copy-pasted identically into both modules' test blocks, testing the
same single function (`local::is_critical_mount`). One copy should exist, where the function is
defined.

---

## 4. Half-implemented or abandoned

### 4.1 Persistence state read-back missing (see 1.3) — **High**
The single biggest gap between what the UI presents (a stateful toggle) and what is implemented
(a write-only fire-and-forget).

### 4.2 `docs/PROJECT_STATUS.md` describes a `--remount-shares` headless mode that does not exist — **High**
**File:** `docs/PROJECT_STATUS.md:29-30` vs `src/main.rs` (no argument parsing at all).

The status doc claims main.rs has a "Headless `--remount-shares` mode for the generated systemd
--user unit." No such flag, no systemd unit generation, and no argv handling exists anywhere in
the tree (`grep remount` matches only this doc line). This is a leftover from the abandoned
GVfs-era re-mount-on-login design that the privileged-mount pivot explicitly removed
(`window.rs:473-475` references "the old GVfs-era re-mount-on-login workaround"). A contributor
reading PROJECT_STATUS.md will look for code that was never written or was deleted.

### 4.3 `CLAUDE.md` project context describes the pre-pivot codebase — **High**
**File:** `CLAUDE.md` ("Project Context" and "Repository Notes" sections).

CLAUDE.md still says the project is "Flatpak-packaged," lists `org.example.DriveDock.yml` (a
Flatpak manifest deleted in the Nix pivot, commit 78feebd) as a key file, lists
`src/network/smb.rs` / `src/network/nfs.rs` (never-existed or renamed; actual files are
`discover.rs`/`mount.rs`), describes `src/system/local.rs` as parsing `/proc/mounts` (it uses
`gio::VolumeMonitor`, and its own doc comment :58-59 says "rather than parsing /proc/mounts"),
and says mounting is "async-ready stub implementations" routing through Polkit/UDisks2 (both are
now fully implemented). The FORBIDDEN COMMANDS section forbids `setup-dev.sh`/`build-flatpak.sh`,
neither of which exists in the tree. Since CLAUDE.md is the standing instruction set for AI
tooling on this repo, its staleness actively steers automated contributors toward files and
constraints that no longer exist. Same applies to `.github/copilot-instructions.md` where it
overlaps.

### 4.4 Dead code kept "for interface stability" with no consumer — **Low**
**File:** `src/network/discover.rs:136-147` (`is_network_uri`, `#[allow(dead_code)]`).

Explicitly retained-but-unused, with a comment speculating about future need. Also
`src/network/mod.rs:6-9` carries an `#[allow(unused_imports)]` on the re-export of `Credentials`
with a justification comment longer than the code. Both are small, but they establish a pattern
of suppressing the compiler's dead-code detection instead of deleting (the stated project
principle is "no speculative code").

### 4.5 Commented-out re-export — **Low**
**File:** `src/ui/mod.rs:3-4` (`// pub use window::Window;`).

Two-line module with a commented-out line and a comment describing it. Callers use
`ui::window::Window` directly; delete the comment or use the re-export.

---

## 5. Dependency findings

### 5.1 `tokio` — declared, feature-gated, and never used anywhere — **Medium**
**File:** `Cargo.toml:41`, `:55-60`.

The `tokio-runtime` feature is never enabled by default, never referenced by any `#[cfg]` in
`src/` (`grep tokio src/` matches nothing), and never enabled by `nix/package.nix`. It exists
purely "for future advanced async needs" — speculative, per the project's own Simplicity First
rule. Ironically, an async process API is *exactly* what 1.1 needs, so either put it to use or
remove it (removal also shrinks `Cargo.lock` and the Nix closure's vendored-deps hash surface).

### 5.2 `thiserror` — used for one enum whose variants no caller inspects — **Low**
**File:** `Cargo.toml:45`; sole use `src/system/unmount.rs:8-24`.

See 2.3. If `UnmountError` becomes `anyhow`, the dependency goes away.

### 5.3 `futures` — full crate pulled in for one `oneshot` channel — **Low**
**File:** `Cargo.toml:35`; sole use `src/network/mount.rs:280` (`futures::channel::oneshot`).

`futures` is a facade crate re-exporting many subcrates. For a single oneshot channel,
`futures-channel` alone suffices — or zero extra deps via `glib`'s own async primitives, which
are already in-tree. Not wrong, just heavier than the use justifies.

### 5.4 `libc` — used for a single `flock` call in the helper — **Low**
**File:** `Cargo.toml:38`; sole use `src/bin/drivedock-mount-helper.rs:281`.

Defensible (the helper wants minimal deps and `flock` has no std wrapper), but note the *GUI*
binary also links `libc` for nothing, since dependencies are per-crate not per-bin-target.
`rustix` or `std::os::fd` + `nix` are alternatives; keeping `libc` is fine — flagged only for
awareness that it exists solely for one syscall.

### 5.5 `serde`/`serde_json` comment misdescribes their purpose — **Low**
**File:** `Cargo.toml:51-53` ("Serialization for configuration").

Actual use is the GUI↔helper request protocol (`MountRequest`/`Request`), not configuration.
There is no configuration system. Cosmetic, but Cargo.toml comments are the first thing a
packager reads.

### 5.6 No outdated dependencies found — informational
`gtk4` 0.9 / `libadwaita` 0.7 / `glib`+`gio` 0.20 are a mutually consistent gtk-rs release set;
`udisks2` 0.3 is current on crates.io as of the knowledge cutoff. No version conflicts evident
from `Cargo.toml`. (Not verified against live crates.io from this offline analysis.)

---

## Summary of priorities

| # | Finding | Priority |
|---|---------|----------|
| 1.1 | Blocking `pkexec` subprocess on GTK main thread inside fake-async fns | High |
| 1.2 | Error-path checkbox revert re-triggers `connect_toggled`, firing the opposite operation | High |
| 1.3 / 4.1 | Permanent-dock toggle never initialized from actual fstab/UDisks state | High |
| 2.4 | Duplicate, already-diverged `BUILDING.md` at root vs `docs/` | High |
| 4.2 | PROJECT_STATUS.md documents nonexistent `--remount-shares` mode | High |
| 4.3 | CLAUDE.md describes the deleted Flatpak-era codebase (files, stubs, forbidden scripts) | High |
| 1.4 | Credentials-dialog Cancel proceeds with anonymous mount | Medium |
| 1.5 | `window.rs` god-module, duplicated row builders, no list model | Medium |
| 1.6 | Duplicated `/proc/mounts`/slug/fs-type logic between GUI and helper (incl. `cifs2` typo, missing `smb3`) | Medium |
| 1.7 | `unmount_share` silently defaults unknown protocol to `cifs` | Medium |
| 2.1 | `udisks.rs` breaks the one-directory-per-domain convention | Medium |
| 2.2 | Local-drive mount/unmount/persist split across UI, `system`, `dock` | Medium |
| 2.3 | Three error-handling conventions in one small crate | Medium |
| 2.5 | `org.example.*` placeholder app/Polkit IDs | Medium |
| 3.1 | Two sources of truth for mounted-state; no local/network dedup | Medium |
| 5.1 | `tokio` declared and never used | Medium |
| 1.8 | `dock::fstab` pure pass-through layer | Low |
| 2.6 | Sync functions marked `async` | Low |
| 3.2 | Single status line: contradictory partial-failure copy | Low |
| 3.3 | Duplicated NixOS/OtherLinux message strings | Low |
| 3.4 | Copy-pasted `is_critical_mount` tests | Low |
| 4.4 | `#[allow(dead_code)]`-suppressed speculative code | Low |
| 4.5 | Commented-out re-export in `ui/mod.rs` | Low |
| 5.2 | `thiserror` for one un-matched enum | Low |
| 5.3 | Full `futures` crate for one oneshot channel | Low |
| 5.4 | `libc` for one `flock` call (links into GUI too) | Low |
| 5.5 | Misleading serde comment in Cargo.toml | Low |
