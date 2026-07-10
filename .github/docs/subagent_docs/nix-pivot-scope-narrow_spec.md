# DriveDock: Nix Pivot + Scope Narrowing — Phase 1 Specification

Status: DRAFT (Phase 1 — Research & Specification only, no code changes)
Author: Orchestrating Agent (Phase 1 subagent)
Date: 2026-07-09

---

## 1. Current State Analysis

### 1.1 What exists today

- **`src/main.rs`** — `libadwaita::Application` bootstrap, `tracing` init, builds `ui::window::Window`. No changes needed structurally.
- **`src/ui/window.rs`** — Single `AdwApplicationWindow` with three `AdwPreferencesGroup` sections (Local Drives, Network Drives, Status), all placeholder content. No real data binding yet.
- **`src/system/local.rs`** — `MountedDrive` struct + `list_mounted_drives()` **stub** (documented intent: parse `/proc/mounts`, `statvfs` for size). Has unit tests for pure helper functions (size formatting) only.
- **`src/system/unmount.rs`** — `UnmountError` (`thiserror`) + `unmount_drive()`/`force_unmount()` **stubs**. Contains critical-mount-detection logic (protects `/`, `/boot`, `/home`, etc.) — this logic is reusable and should be preserved.
- **`src/network/smb.rs`** — `SmbShare` struct + **stub** `scan_smb_shares()` (Avahi `avahi-browse` + `smbclient -L` + `smbtree` parsing, all as `std::process::Command` shell-outs) and **stub** `mount_smb_share()` (raw `pkexec mount -t cifs` + hand-rolled `/etc/fstab` append via a temp file piped through `pkexec sh -c "cat ... >> /etc/fstab"`).
- **`src/network/nfs.rs`** — Same pattern as `smb.rs` for NFS (`avahi-browse` + `showmount -e`, raw `pkexec mount -t nfs`/`nfs4`, same hand-rolled fstab append).
- **`Cargo.toml`** — `gtk4` 0.9, `libadwaita` 0.7, `glib` 0.20, `futures`, `libc`, optional `tokio`, `anyhow`, `thiserror`, `tracing`/`tracing-subscriber`, `serde`/`serde_json`. **No `gio` crate declared explicitly** (it's a transitive re-export available via `gtk::gio` / `libadwaita::gio` from the `gtk4`/`libadwaita` crates' `Cargo.toml` — gtk-rs crates depend on `gio` internally and typically re-export it, but it is not currently a direct dependency of this crate, so it must be added directly for a clean, explicit API surface). **No D-Bus crate** (`zbus`, `dbus`, etc.) exists at all.
- **Flatpak packaging**: `org.example.DriveDock.yml` (manifest, GNOME 47 runtime, D-Bus/filesystem permissions for UDisks2/PolicyKit1/Avahi), `build-flatpak.sh`, `setup-dev.sh`. `docs/BUILDING.md` and `README.md` both document the Flatpak workflow as primary/recommended. `docs/PROJECT_STATUS.md` documents the Flatpak-centric roadmap. `.github/copilot-instructions.md` states "App must be built and shipped as a Flatpak" as a primary purpose bullet.
- **`CLAUDE.md`** (this repo's own orchestration doc) — Resource Constraints section documents Flatpak forbidden commands and references `org.example.DriveDock.yml` in Repository Notes/Special Constraints. **Out of scope to edit in Phase 1/2** per task instructions, but it will need a follow-up update once the pivot lands (flagged as a risk in §7).

### 1.2 What becomes obsolete / must be removed

- `src/network/smb.rs` and `src/network/nfs.rs` — the **discovery** logic (Avahi `avahi-browse`, `nmblookup`, `smbtree`, `showmount -e` subprocess parsing) is fully superseded by GVfs/GIO discovery (§3.2) and must be deleted, not refactored. The **data structures** (`SmbShare`/`NfsExport`-equivalent) are replaced by a unified `NetworkShare` model backed by `gio::Mount`/`gio::File` (§3.2). The **mounting** logic (raw `pkexec mount -t cifs/nfs` + hand-rolled fstab append) is replaced per §3.3/§3.4.
- `org.example.DriveDock.yml`, `build-flatpak.sh`, `setup-dev.sh` — deleted outright (§6).
- Flatpak-specific sections of `README.md`, `docs/BUILDING.md`, `docs/PROJECT_STATUS.md`, `.github/copilot-instructions.md` — deleted/rewritten to describe the Nix flake workflow instead (§6). `CLAUDE.md` needs a corresponding update but is explicitly out of scope for this Phase 1/2 cycle — flagged for the user.
- Credential-manager-adjacent scope: per the narrowed feature list, no interactive username/password dialog is being (re)built. GVfs's own `gio::MountOperation` will surface GNOME's native credential prompt (Files/Nautilus-style) when needed — DriveDock does not need to implement its own credentials UI, consistent with "no credential-manager UI" in the pivot brief.

---

## 2. Problem Definition

Pivot goals (restated, final per user):

1. **Distribution**: Flatpak → Nix flake. Flake must expose a full GUI package (not just a dev shell) plus a NixOS module, consumable as a flake input by the user's separate `vexos-nix` NixOS-config repo.
2. **Scope**: Narrow to exactly — list local drives + network shares, mount ("dock"), unmount, and a per-item "permanently dock" toggle that persists the mount across reboots. Nothing else.
3. **Network discovery**: Stop reimplementing Avahi/smbclient/showmount. Piggyback on GNOME/GVfs's own discovery via `gio`.
4. **Persistence**: OS-aware — non-NixOS Linux writes `/etc/fstab`; NixOS needs a technically correct, NixOS-native approach (not naive raw fstab editing). Privileged mount/unmount operations continue to go through Polkit/UDisks2 D-Bus, which must be confirmed compatible with both non-NixOS and NixOS, and compatible with Nix packaging.

---

## 3. Proposed Solution Architecture

### 3.1 New/changed module structure

```
src/
├── main.rs                  # unchanged (entry point)
├── ui/
│   ├── mod.rs
│   └── window.rs            # REWRITE: two sections only — "Drives" (local+network
│                             # unified list, each row: name, kind badge, mount state,
│                             # Dock/Undock button, "Permanently dock" checkbox) and
│                             # "Status" (feedback log). Drop separate Local/Network
│                             # sections in favor of one list with a protocol/kind
│                             # badge per gio::Drive/Volume/Mount, OR keep two
│                             # sections but backed by the same discovery module —
│                             # implementation detail for Phase 2, not a Phase 1
│                             # architectural fork. Recommendation: keep the existing
│                             # two-section layout (matches copilot-instructions.md
│                             # UX spec, still valid) but source both from `discovery`.
├── system/
│   ├── mod.rs
│   ├── local.rs             # REWRITE: local drive enumeration now backed by
│                             # gio::VolumeMonitor (drives/volumes/mounts) instead of
│                             # /proc/mounts parsing. Keep MountedDrive-equivalent
│                             # struct as a thin projection over gio::Drive/Volume/Mount
│                             # for UI binding. Keep is_critical_mount() safety logic
│                             # from unmount.rs unchanged (still needed to block
│                             # unmounting "/", "/boot", "/home", etc.), adapted to
│                             # take a mount-point path extracted from gio::Mount.
│   └── unmount.rs            # REWRITE: unmount() now calls UDisks2 D-Bus
│                             # Filesystem.Unmount for local block devices (§3.3)
│                             # instead of a stub. Keep UnmountError + critical-mount
│                             # guard logic.
├── network/
│   ├── mod.rs
│   └── gvfs.rs               # NEW, replaces smb.rs + nfs.rs entirely.
│                             # - Enumerates gio::File::for_uri("network:///") children
│                             #   (GVfs-discovered SMB/NFS/etc. servers/shares) via
│                             #   enumerate_children_async.
│                             # - Also surfaces already-mounted network gio::Mounts from
│                             #   VolumeMonitor (a share the user mounted previously via
│                             #   Nautilus/Files already appears as a Mount, not just a
│                             #   network:// child).
│                             # - Unified NetworkShare struct: display_name, uri
│                             #   (smb://host/share, nfs://host/path), protocol label
│                             #   parsed from URI scheme, mounted: bool.
│                             # - mount()/unmount() via gio::File::mount_enclosing_volume
│                             #   / gio::Mount::unmount_with_operation (§3.3).
├── dock/                     # NEW module: OS detection + permanent-dock persistence
│   ├── mod.rs                # detect_os() -> Os::NixOs | Os::OtherLinux
│   ├── fstab.rs               # non-NixOS path: UDisks2 Block.AddConfigurationItem /
│                             # RemoveConfigurationItem for LOCAL block devices (§3.4).
│                             # For network shares on non-NixOS, still fstab-based but
│                             # via the same UDisks2-adjacent pattern is NOT available
│                             # (UDisks2 Block interface only applies to block devices) —
│                             # see §3.4 for the network-share persistence answer.
│   └── nixos.rs               # NixOS path: emits user-facing guidance/snippet rather
│                             # than silently writing files NixOS will discard (§3.4).
└── udisks.rs                 # NEW: thin wrapper around the `udisks2` crate (or hand-
                              # rolled zbus proxies) for Filesystem.Mount/Unmount and
                              # Block.AddConfigurationItem/RemoveConfigurationItem.
```

Deleted: `src/network/smb.rs`, `src/network/nfs.rs`.

### 3.2 GVfs/GIO integration for listing (research findings)

Sources consulted: gtk-rs `gio::VolumeMonitor` docs (https://gtk-rs.org/gtk-rs-core/git/docs/gio/struct.VolumeMonitor.html), GNOME C API docs.gtk.org `Gio.VolumeMonitor` (https://docs.gtk.org/gio/class.VolumeMonitor.html), docs.rs `gio::File` (https://docs.rs/gio/latest/gio/struct.File.html), docs.rs `gio::FileEnumerator` (https://docs.rs/gio/latest/gio/struct.FileEnumerator.html). Context7 was tried first for "gio" — it only indexes the unrelated Go "Gio UI" toolkit, not the GNOME/gtk-rs `gio` crate, so WebFetch/docs.rs was used instead per the Phase 1 fallback instruction.

Findings:
- `gio::VolumeMonitor::get()` returns the shared monitor singleton. It exposes `connected_drives()` / `volumes()` / `mounts()` (Rust-idiomatic accessors for the C `get_connected_drives`/`get_volumes`/`get_mounts`), returning `Vec<gio::Drive>` / `Vec<gio::Volume>` / `Vec<gio::Mount>` respectively, plus `*-added`/`*-removed`/`*-changed` signals for live updates. **Requires a running GLib main loop** to receive updates — already satisfied since this is a GTK4 app.
- `gio::Drive`/`gio::Volume`/`gio::Mount` expose `name()`, `icon()`, and for `Mount`: `root()` (a `gio::File` giving the mount path/URI) and `default_location()`.
- The C docs and Rust docs do **not explicitly confirm** whether GVfs network mounts (smb/nfs/dav) always populate as `gio::Mount` automatically — they do once actively mounted (by DriveDock itself or by another GVfs client like Nautilus), consistent with GVfs's shared-daemon architecture (gvfsd is a per-session daemon; all GIO clients in the session see the same mount table). This is standard GVfs behavior, not something Rust-specific.
- **Discovery of not-yet-mounted network shares** (the actual "piggyback on GNOME's discovery" requirement) is via GVfs's virtual `network:///` root, exactly like Nautilus's "Other Locations" → "Network" view: create `gio::File::for_uri("network:///")` and call `enumerate_children_async()` (or the `_future()` combinator for async/await) to iterate `gio::FileInfo` children — each child represents a discovered server/workgroup (populated by the `gvfsd-network` backend, which itself uses Avahi/WS-Discovery/SMB browsing internally — this is exactly the "let GNOME do the discovery" behavior requested). Enumerating one level deeper (a specific server's `File`) lists its shares.
- To mount a discovered-but-unmounted network location: `gio::File::mount_enclosing_volume_future(flags, Option<&MountOperation>)`. A `gio::MountOperation` supplies a `ask-password`/`ask-question` signal handler for credential prompts — DriveDock can either supply a minimal GTK dialog or (simpler, in scope for "no credential-manager UI") pass `None`/an anonymous operation and let the mount fail with a clear error asking the user to mount once via Nautilus first, then DriveDock just docks/undocks the resulting `gio::Mount`. **Recommendation for Phase 2 spec drafting**: implement a minimal `MountOperation` with password callback (a single GTK `PasswordEntryRow` in an Adw dialog) since fully punting credentials to Nautilus would defeat the "Dock" mount button's purpose for shares not already known to the system. This is a Phase 2 implementation decision, not a scope-creep — the toggle/mount flow requires *some* credential path or it can only "dock" already-mounted shares.
- Unmounting: `gio::Mount::unmount_with_operation_future(flags, Option<&MountOperation>)`.
- **Local (non-network) volumes**: `gio::VolumeMonitor` also lists local/removable volumes and their `gio::Drive` (physical device) parent, with `Volume::identifier("unix-device")` giving the `/dev/sdXN` path needed to correlate with UDisks2 objects for the privileged mount/unmount and fstab-config path (§3.3/§3.4). This is the bridge between the GIO listing layer and the UDisks2 D-Bus privileged-operation layer.

### 3.3 Mount/unmount approach

Two different backends for two different mount kinds — this is the key architectural decision, and it is a **deliberate split**, not an inconsistency:

- **Local block devices (internal/removable drives)**: UDisks2 D-Bus, `org.freedesktop.UDisks2.Filesystem.Mount` / `.Unmount` on the `Block` object matching the device (found by cross-referencing `gio::Volume::identifier("unix-device")` against UDisks2's `/org/freedesktop/UDisks2/block_devices/<name>` object paths, or preferably via UDisks2's own `Block.Device` byte-array property). UDisks2 itself performs the Polkit authorization prompt — no manual `pkexec` subprocess needed. This matches the existing project constraint (Polkit/UDisks2 D-Bus for privileged mount/unmount) unchanged by the pivot.
- **Network shares (SMB/NFS/etc.)**: **GVfs/GIO** (`gio::File::mount_enclosing_volume_future` / `gio::Mount::unmount_with_operation_future`), **not** UDisks2. Finding: UDisks2's `Filesystem` interface operates on `Block` objects, i.e. local block devices only (confirmed via https://storaged.org/udisks/docs/gdbus-org.freedesktop.UDisks2.Filesystem.html and the `Block` interface docs) — it has no generic CIFS/NFS network-mount capability. GVfs network mounts are unprivileged (run in the user's session via `gvfsd`/FUSE, no root, no Polkit prompt), which is actually *simpler* than the original stub design's `pkexec mount -t cifs`. This is a deliberate, technically-justified split: **local = UDisks2 (privileged, Polkit-gated)**, **network = GVfs (unprivileged, session-scoped)**. It does not conflict with the existing "prefer Polkit/UDisks2 for privileged operations" project constraint, because network-share mounting was never a privileged/root operation to begin with under this design — it only *appears* privileged in the current stub code because the stub shells out to raw `mount -t cifs` via `pkexec`, which is exactly the pattern being replaced.
- **D-Bus crate decision**: No D-Bus crate exists in `Cargo.toml` today. Two options researched via Context7 (`/z-galaxy/zbus`) and docs.rs:
  1. Raw `zbus` (`Connection::system().await?` + hand-written `#[proxy]` traits for `org.freedesktop.UDisks2.Filesystem`/`.Block`/`.Manager`) — full control, more boilerplate, verified current API pattern from zbus's own book/README via Context7 (macro is `#[proxy(interface = ..., default_service = ..., default_path = ...)]`, async-first, works with any async runtime including `glib`'s if a compatible executor is wired in, or `tokio` via the already-present optional `tokio-runtime` feature).
  2. The `udisks2` crate (crates.io, https://docs.rs/udisks2, MIT/LGPL-2.1, by FineFindus — https://github.com/FineFindus/udisks-rs, actively maintained, latest 0.3.x as of research) — a ready-made zbus-based client: `udisks2::Client::new().await?`, `client.object("/org/freedesktop/UDisks2/block_devices/sda")`, `.block()`/`.filesystem()` interface accessors. This already wraps the exact `Filesystem` and `Block` (incl. `AddConfigurationItem`/`RemoveConfigurationItem`) interfaces needed.
  - **Recommendation**: depend on the `udisks2` crate directly rather than hand-rolling zbus proxies. It is a thin, purpose-built wrapper (not a heavy abstraction), directly covers every UDisks2 interface this feature needs, and avoids maintaining hand-written D-Bus XML-derived proxy traits. It pulls in `zbus` transitively, so `zbus` does not need to be a *direct* dependency, but should be pinned/verified compatible in `Cargo.lock` regardless (Phase 2 must run `cargo tree` or check `Cargo.lock` to confirm no version conflicts with `glib`/`gio`'s own async expectations — `zbus` is runtime-agnostic and doesn't require tokio).

### 3.4 Permanent-mount (OS-aware) persistence — concrete recommendation

This is the most consequential research finding of this spec, and it **corrects an assumption in the task brief**: the brief speculates that manual `/etc/fstab` edits might coexist safely with NixOS's declarative `fileSystems` as long as the specific mount point isn't declared. **This is not correct as a general safety guarantee**, per direct research:

- Sources: NixOS/nixpkgs issue #499807 ("`fileSystems`: Populates fstab instead of mount units") and #49337, NixOS `nixos/modules/tasks/filesystems.nix` (https://github.com/NixOS/nixpkgs/blob/master/nixos/modules/tasks/filesystems.nix), NixOS manual (https://nixos.org/manual/nixos/stable/).
- **Finding**: On NixOS, `/etc/fstab` is a fully **generated** file, written from the `fileSystems`/`swapDevices` options as part of every `nixos-rebuild switch`/`boot`. It is not merged additively with hand-edits — NixOS treats the whole file as its own output. Any manually-appended line **will be silently deleted on the next `nixos-rebuild`**, because the generator writes the complete file contents from the Nix module system's evaluated config, not from the file's prior contents. There is no supported "leave unmanaged lines alone" escape hatch in `/etc/fstab` itself on NixOS.
- **Concrete recommendation for NixOS**:
  1. **Local block devices**: use **UDisks2's `Block.AddConfigurationItem`** (see below) — this writes into `/etc/fstab` via UDisks2's own privileged helper, which on NixOS **still gets wiped on the next rebuild** just like any other manual fstab write, because UDisks2 doesn't know about NixOS's config-generation model — so this alone is not sufficient on NixOS either. **The only durable-across-rebuild option on NixOS is a declarative `fileSystems."/mount/point" = { device = ...; fsType = ...; options = [...]; };` block added to the user's `configuration.nix` (or, since this app ships as a `nixosModules` flake output for `vexos-nix`, a NixOS **module option** DriveDock's own `nixosModules.default` could expose, e.g. `services.drivedock.persistentMounts`, which the module renders into `fileSystems` internally).** DriveDock (the GUI, running as an ordinary user session app) **cannot itself durably persist a mount across a NixOS rebuild** by writing any file at runtime — persistence on NixOS is fundamentally a config-time, not runtime, operation. Given the narrowed scope explicitly excludes anything beyond list/dock/undock/permanent-toggle, the technically honest behavior is:
     - On NixOS, the "Permanently dock" toggle **still performs a live UDisks2 `AddConfigurationItem` fstab write** (so the mount does survive a normal *reboot* that isn't preceded by a `nixos-rebuild`, since nothing regenerates `/etc/fstab` on a plain reboot — it's only rewritten during `nixos-rebuild switch/boot`), **and additionally surfaces a status-panel message/snippet** telling the user the exact `fileSystems."<mountpoint>" = { device = "/dev/disk/by-uuid/<uuid>"; fsType = "<fs>"; options = [ ... ]; };` block to add to their NixOS configuration for the mount to survive a future `nixos-rebuild`. This is the technically correct, non-silent behavior: DriveDock does what it can at the D-Bus/runtime layer and is explicit about the boundary of what it cannot do (Nix config is out of its runtime control).
     - This should be implemented as a distinct `Os::NixOs` branch in `src/dock/mod.rs` → `src/dock/nixos.rs`, which performs the same `AddConfigurationItem` call as `fstab.rs` (no need to skip the runtime persistence — a plain reboot without a rebuild in between is a normal, common case) but tags the Status section message differently to include the config-snippet guidance. **Do not silently pretend the toggle is equivalent to a real NixOS-durable mount** — that would violate the "surface assumptions, don't guess" engineering principle in `CLAUDE.md`.
  2. **Network shares** (SMB/NFS) on **both** NixOS and non-NixOS: since these are mounted via GVfs (§3.3), they are inherently **session-scoped, not systemwide `/etc/fstab` entries at all** (no `Block` object exists to call `AddConfigurationItem` on — GVfs mounts are FUSE-backed userspace mounts, invisible to `/etc/fstab`/UDisks2 entirely). "Permanently dock" for a network share therefore cannot mean a fstab entry on either OS. **Recommendation**: persist a lightweight DriveDock-local record (e.g. `~/.config/drivedock/persistent-shares.json`, using the already-present `serde`/`serde_json` deps) of URIs the user toggled "permanently dock" for, and re-issue the `gio::File::mount_enclosing_volume` call for each recorded URI on DriveDock startup (or via a `systemd --user` unit / XDG autostart entry launching a headless re-mount at login, if "persists across reboots" is interpreted to mean "without the user manually reopening DriveDock"). This must be flagged to the user as a genuine design choice/tradeoff (not a silent pick) — see §7.
- **Non-NixOS Linux**: direct `/etc/fstab` write is safe and conventional. Recommendation: implement it via **UDisks2 `Block.AddConfigurationItem`** (declarative, Polkit-gated, no raw file parsing/temp-file/`pkexec cat >>` needed) rather than the existing stub's hand-rolled read-modify-append-via-shell approach — confirmed via https://storaged.org/udisks/docs/gdbus-org.freedesktop.UDisks2.Block.html: the method takes a `(sa{sv})` configuration item of type `"fstab"` with a dict of `fsname`/`dir`/`type`/`opts`/`freq`/`passno` (fields may be omitted for UUID-based defaults), and is Polkit-authorized. `RemoveConfigurationItem` is the exact inverse for un-toggling "permanently dock." This eliminates the current duplicate-detection-by-string-matching logic entirely (UDisks2 handles it).
- **UDisks2 on NixOS**: confirmed compatible — UDisks2 is a standard systemd/D-Bus system service shipped in nixpkgs (`services.udisks2.enable`, on by default in most desktop NixOS configs via GNOME/Plasma modules), no NixOS-specific conflicts. This does not conflict with Nix packaging of DriveDock itself; DriveDock only needs to *talk to* the system's already-running `udisks2` daemon over D-Bus, it does not need to bundle or manage UDisks2.

### 3.5 Nix flake structure

Sources: gtk-rs official Rust bindings guidance (https://www.gtk.org/docs/language-bindings/rust/), example `buildRustPackage` GTK4 flake (https://github.com/SKyletoft/gtk4_test/blob/master/flake.nix), `nixos-hardware` as the canonical "flake shipping only `nixosModules.*` for downstream consumption" pattern (https://github.com/NixOS/nixos-hardware), NixOS Wiki Rust page (https://nixos.wiki/wiki/Rust), real-world GTK4/libadwaita NixOS-ecosystem apps for precedent (`nixos-conf-editor`, `nix-software-center`, `icicle` — all `snowfallorg` projects using `libadwaita`+`gtk4`+Rust, packaged for NixOS).

Recommended `flake.nix` outputs (minimum, matching conventional flake-input consumption patterns like `nixos-hardware`):

```
outputs = { self, nixpkgs, ... }:
  let
    forAllSystems = nixpkgs.lib.genAttrs [ "x86_64-linux" "aarch64-linux" ];
  in {
    packages = forAllSystems (system:
      let pkgs = nixpkgs.legacyPackages.${system}; in {
        default = pkgs.callPackage ./nix/package.nix { };
        drivedock = self.packages.${system}.default;  # aliased name for clarity when consumed as an input
      });

    nixosModules = {
      default = import ./nix/module.nix self;
      drivedock = self.nixosModules.default;
    };

    devShells = forAllSystems (system:
      let pkgs = nixpkgs.legacyPackages.${system}; in {
        default = pkgs.callPackage ./nix/shell.nix { };
      });
  };
```

- **Packaging approach**: `pkgs.rustPlatform.buildRustPackage` (not `crane`/`naersk`) — recommended because: (a) it's the batteries-included nixpkgs-native approach with zero extra flake inputs, matching the simplicity-first principle in `CLAUDE.md`; (b) `crane`/`naersk` mainly earn their keep for incremental-build caching in CI, which is not a stated requirement here (no CI configured per Resource Constraints); (c) every located real-world precedent (`gtk4_test`, and implicitly `nixos-conf-editor`/`nix-software-center` per the snowfall-lib-based ecosystem convention) uses `buildRustPackage` for GTK4/libadwaita Rust apps. Use `cargoLock.lockFile = ./Cargo.lock;` (not `cargoHash`) since the repo already commits `Cargo.lock`.
- **System deps via nixpkgs**: `nativeBuildInputs = [ pkg-config wrapGAppsHook4 ]` (GTK4-specific wrap hook — note this differs from the plain `wrapGAppsHook` used for GTK3; `wrapGAppsHook4` is the GTK4-correct variant and should be verified present in the target nixpkgs channel during Phase 2 implementation), `buildInputs = [ gtk4 libadwaita glib ]`. `wrapGAppsHook4` is important and was *absent* from the `gtk4_test` reference flake (flagged by that flake's own omission) — without it, GSettings schemas/icon themes/GVfs may not resolve correctly at runtime for the packaged binary; Phase 2 should include it deliberately rather than copy the minimal reference verbatim.
- **NixOS module** (`nix/module.nix`): standard `{ config, lib, pkgs, ... }: { options.programs.drivedock.enable = lib.mkEnableOption "DriveDock"; config = lib.mkIf cfg.enable { environment.systemPackages = [ package ]; services.udisks2.enable = true; services.dbus.packages = [ ... ]; /* polkit already enabled by udisks2 module */ }; }` pattern, taking `self` (the flake) as a parameter so it can reference `self.packages.${pkgs.system}.default` without re-fetching nixpkgs — mirrors the `nixos-hardware`-style "flake input ships a module referencing its own package" convention `vexos-nix` will consume.
- **devShell**: `mkShell { inputsFrom = [ package ]; packages = [ rust-analyzer clippy rustfmt ]; }` — standard, low-risk, matches "simplicity first."

---

## 4. Implementation Steps (ordered, for Phase 2)

1. `Cargo.toml`: add `gio = "0.20"` (matching the existing `glib`/`gtk`/`libadwaita` 0.20-series pin — verify exact compatible version against the already-pinned `gtk4`/`libadwaita` versions during Phase 2, since gtk-rs crates are released in lockstep) and `udisks2 = "0.3"`. Remove nothing yet (do this after new code compiles, to avoid a broken intermediate state — see step 6).
2. Delete `src/network/smb.rs`, `src/network/nfs.rs`. Create `src/network/gvfs.rs` implementing discovery (§3.2) and mount/unmount (§3.3).
3. Rewrite `src/system/local.rs` to source from `gio::VolumeMonitor` instead of `/proc/mounts`; keep the existing `MountedDrive`-shaped public struct where feasible to minimize UI churn.
4. Create `src/udisks.rs` wrapping `udisks2::Client` for `Filesystem.Mount`/`Unmount` and `Block.AddConfigurationItem`/`RemoveConfigurationItem`.
5. Rewrite `src/system/unmount.rs` to call `src/udisks.rs` instead of stubbing; preserve `is_critical_mount`/`UnmountError`.
6. Create `src/dock/mod.rs` (OS detection: check `/etc/NIXOS` marker file or `/run/current-system` symlink presence — standard, well-known NixOS-detection convention), `src/dock/fstab.rs` (non-NixOS + NixOS-reboot-only local persistence, both via `udisks.rs`), `src/dock/nixos.rs` (config-snippet guidance text + the persistent-shares JSON file for network shares, shared by both OS branches per §3.4 point 2).
7. Rewrite `src/ui/window.rs` to bind to the new unified discovery/dock modules per the narrowed 2-section UI (Drives, Status) — Phase 2 implementation detail.
8. `cargo check` / `cargo build` / `cargo test` iteratively (per CLAUDE.md Phase 3 validation commands) until green.
9. Delete `org.example.DriveDock.yml`, `build-flatpak.sh`, `setup-dev.sh`.
10. Update `README.md`, `docs/BUILDING.md`, `docs/PROJECT_STATUS.md`, `.github/copilot-instructions.md` to describe the Nix flake workflow and narrowed scope instead of Flatpak (content-only edits, no code). Flag `CLAUDE.md` itself as needing a follow-up (out of scope this cycle).
11. Add `flake.nix`, `flake.lock`, `nix/package.nix`, `nix/module.nix`, `nix/shell.nix` per §3.5.
12. Update `.gitignore` if needed (e.g. add `result`, `result-*` for `nix build` symlink outputs; keep existing Rust/editor ignores; the current Flatpak-specific ignore lines — `build-dir/`, `.flatpak-builder/`, `repo/`, `.flatpak/` — become dead entries and should be removed).

---

## 5. Dependencies

| Crate | Version | Purpose | Verified via |
|---|---|---|---|
| `gio` | `0.20` (match `glib`/`gtk4`/`libadwaita` series) | `VolumeMonitor`, `File`, `Mount`, `Volume`, `Drive`, `MountOperation` | docs.rs/gio, gtk-rs docs (Context7 had no coverage for this crate — used WebFetch fallback per policy) |
| `udisks2` | `0.3.x` | UDisks2 D-Bus client (`Filesystem.Mount/Unmount`, `Block.AddConfigurationItem/RemoveConfigurationItem`) | docs.rs/udisks2, github.com/FineFindus/udisks-rs |
| (`zbus`, transitive via `udisks2`) | whatever `udisks2` 0.3 pins | underlying D-Bus transport | Context7 `/z-galaxy/zbus` (client connection & `#[proxy]` macro pattern confirmed current) |

Removed: none of the current deps become unnecessary — `anyhow`/`thiserror`/`tracing`/`serde`/`serde_json`/`futures`/`libc` are all still used (`serde`/`serde_json` now additionally used for the persistent-shares JSON file in §3.4). `tokio`(optional `tokio-runtime` feature) may remain optional/unused unless Phase 2 finds `zbus`'s default async executor needs it — `zbus` is runtime-agnostic so this is not expected to be required.

Nix-side dependencies (flake inputs): `nixpkgs` (single input is sufficient — no `flake-utils`/`crane`/`naersk` needed per §3.5's simplicity rationale; if per-system boilerplate is disliked, `flake-utils` may be added later as a pure convenience, not a requirement).

---

## 6. What Gets Deleted

- `org.example.DriveDock.yml`
- `build-flatpak.sh`
- `setup-dev.sh`
- `src/network/smb.rs`, `src/network/nfs.rs` (replaced by `src/network/gvfs.rs`)
- Flatpak-specific content in `README.md` (Installation section's `./setup-dev.sh && ./build-flatpak.sh` instructions), `docs/BUILDING.md` (entire "Alternative: Use Flatpak Builder" / "Option 2/3" sections, GNOME Platform/SDK prerequisites framing), `docs/PROJECT_STATUS.md` (Flatpak Integration section, Flatpak references in the file tree diagram), `.github/copilot-instructions.md` ("App must be built and shipped as a Flatpak" bullet and the "FLATPAK REQUIREMENTS" section) — rewritten to describe `nix build`/`nix run`/flake-module consumption instead.
- `CLAUDE.md` itself is **not edited in this phase** (explicitly out of scope) but flagged: its Resource Constraints/FORBIDDEN COMMANDS/Repository Notes sections reference Flatpak-specific facts (`org.example.DriveDock.yml`, `flatpak install`/`flatpak-builder` forbidden commands, `.github/copilot-instructions.md` Flatpak framing) that will become stale once this pivot lands, and should be revisited by the user or a dedicated follow-up cycle.

---

## 7a. User Decisions (post-Phase-1, confirmed 2026-07-09)

Both open questions in §7 risks 2 and 3 were put to the user and resolved as follows — these are now final for Phase 2:

- **Network share "permanently dock" (§7 risk 2)**: **Re-mount on login.** DriveDock stores toggled network share URIs in `~/.config/drivedock/persistent-shares.json`, and installs a `systemd --user` unit (preferred over XDG autostart for reliability/ordering against `gvfsd`/network-online) that re-mounts each recorded URI at login, in addition to re-mounting on DriveDock's own startup if the unit hasn't run yet or failed.
- **NixOS mount durability (§7 risk 3)**: **Fstab write + show config snippet.** Toggling "Permanently dock" on a local drive always performs the UDisks2 `AddConfigurationItem` write (so it survives a plain reboot on any OS, NixOS included). On NixOS specifically, the Status panel additionally shows the exact `fileSystems."<mountpoint>" = { device = "/dev/disk/by-uuid/<uuid>"; fsType = "<fs>"; options = [ ... ]; };` snippet the user must add to their NixOS configuration for the mount to survive a future `nixos-rebuild`. This asymmetry vs. non-NixOS must be visible in the UI copy itself, not just code comments.

## 7. Risks and Mitigations

1. **`gio::VolumeMonitor`/GVfs requires a session D-Bus + a running `gvfsd`.** Inside a *Nix-built* binary this is not a packaging problem per se (GVfs is a runtime/session dependency, not a build-time link dependency) — but it means DriveDock will show an empty/degraded network-shares list on any system where `gvfs`/`gvfs-mtp`/`gvfs-smb`/`gvfs-nfs` GIO modules aren't installed (common on minimal/non-GNOME NixOS or non-GNOME distros installing DriveDock standalone). **Mitigation**: the NixOS module (`nix/module.nix`) should add `services.gvfs.enable = true;` (and pull in `pkgs.gvfs` for its GIO modules) as part of enabling `programs.drivedock`, and `README.md`/`docs/BUILDING.md` should document that non-NixOS users need `gvfs`, `gvfs-smb` (for CIFS), and `gvfs-nfs` installed for network-share discovery to work at all. This is a hard runtime dependency introduced by the pivot's own design choice (§3.2) and must be documented, not hidden.
2. **GVfs mounts are not real fstab/system mounts** — "permanently dock" for network shares cannot mean a literal `/etc/fstab` entry on any OS (§3.4). This is a genuine scope/expectation gap versus the phrase "persists the mount across reboots" in the pivot brief, which most naturally reads as fstab-style persistence. **This needs explicit user sign-off before Phase 2**: is a DriveDock-managed re-mount-on-login (via a JSON record + XDG autostart/`systemd --user` unit) an acceptable interpretation of "persist across reboots" for network shares, given GVfs mounts are inherently session-scoped? Flagging per "surface assumptions and tradeoffs" — not resolving silently.
3. **NixOS "permanently dock" cannot be equivalent to a true NixOS-durable fstab entry from a running GUI app** (§3.4) — DriveDock's NixOS behavior for local drives is "survives a plain reboot, does not survive a `nixos-rebuild`, and shows the user the `fileSystems` snippet to make it durable." This is a materially different guarantee than the non-NixOS behavior (true fstab persistence). **Mitigation**: make this asymmetry visible in the UI copy itself (e.g. the Status message after toggling "Permanently dock" on NixOS should differ from the non-NixOS message), not just in code comments — a Phase 2/3 review item.
4. **`wrapGAppsHook4` correctness** — the one real-world reference flake found (`gtk4_test`) omits it; omitting it risks a packaged binary that can't find its own GSettings schemas, icons, or (relevantly here) GVfs's GIO modules at runtime. **Mitigation**: explicitly include `wrapGAppsHook4` in `nix/package.nix` nativeBuildInputs per §3.5, and smoke-test `nix run .#default` (a safe, non-forbidden command — this is not `flatpak install`/`flatpak-builder --install`) once the flake is implemented, though this is a Phase 6-adjacent validation step for a future cycle since Nix itself may not be installed in the current dev/preflight environment (needs confirming in Phase 2/3, not assumed here).
5. **UDisks2 device correlation** — matching a `gio::Volume`'s `identifier("unix-device")` (e.g. `/dev/sdb1`) to the correct UDisks2 `/org/freedesktop/UDisks2/block_devices/<name>` D-Bus object path is a string-munge (`sdb1` from `/dev/sdb1`) that's usually reliable but not guaranteed stable (e.g. NVMe naming, device-mapper/LUKS-wrapped volumes). **Mitigation**: prefer matching on UUID (`gio::Volume::identifier("uuid")` vs. UDisks2 `Block.IdUUID` property) where available, falling back to device-path munging only when no UUID exists (e.g. some removable FAT media) — a Phase 2 implementation-level detail to get right, flagged here so it isn't missed.
6. **Version lockstep risk**: `gio` must match the `glib`/`gtk4`/`libadwaita` 0.20-series exactly (gtk-rs releases these in lockstep) — adding `gio = "0.20"` blindly could pull an incompatible patch version if gtk-rs's actual current release differs from `0.20.x`; Phase 2 must run `cargo add gio` and check the resolved `Cargo.lock` version against `glib`'s, not hardcode a guessed version number.
