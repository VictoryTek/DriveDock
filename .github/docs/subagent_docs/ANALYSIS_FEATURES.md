# DriveDock — Feature Recommendations

Date: 2026-07-10. Based on a full read of `src/`, the privileged helper, the Nix packaging, and
the docs (see `ANALYSIS_ARCH.md` / `ANALYSIS_BUGS.md` for the companion defect reports — several
recommendations below convert a reported defect into its natural feature). Scope respects the
project's stated boundary ("dock, undock, permanently dock — nothing else"): everything here
extends the existing flows rather than adding new domains, and nothing requires rearchitecture.

Priority = value relative to effort, given what's already built.

---

## 1. Partially built / clearly intended but unfinished

### 1.1 Read back and display persistence state (finish the "permanently dock" toggle) — **High**
**What exists:** The entire *write* path is done: UDisks2 `Block.AddConfigurationItem` /
`RemoveConfigurationItem` for local drives (`src/udisks.rs:111-166`), tagged
`# drivedock:<slug>` fstab lines for shares (`src/bin/drivedock-mount-helper.rs:230-264`), and
the checkbox UI in both row builders (`src/ui/window.rs:290`, `:476`). What's missing is the
*read* half: the checkbox always renders unchecked (`ANALYSIS_BUGS.md` 1.2/arch 1.3).
**The feature:** During `refresh_drives`, query `Block.Configuration` via the existing `Udisks`
wrapper (one new method, ~15 lines mirroring `remove_fstab_entry`'s configuration read at
`udisks.rs:145-149`) and set `check.set_active(true)` for drives that have an fstab item. For
shares, read `/etc/fstab` (world-readable, no privilege needed) and check for the
`# drivedock:<slug>` tag — the tag-matching logic already exists as a pure function
(`strip_tagged_line`'s anchor logic) and just needs an `is_tagged(contents, slug) -> bool`
sibling. Initialize the checkbox with the signal blocked (which also forces fixing the
revert-retrigger bug). This is the single highest value/effort ratio in the codebase: the toggle
is the app's headline feature and currently lies about state on every launch.

### 1.2 Use the `options` field that is plumbed end-to-end but always empty — **Medium**
**What exists:** `MountRequest.options` travels GUI → JSON → helper → `mount -o` → fstab line
(`src/network/mount.rs:36`, `drivedock-mount-helper.rs:134-140`, `:398-403` with merge/dedup
logic and tests), and `dock::set_permanent_dock` takes an `options` parameter that
`window.rs:327` hardcodes to `"defaults"`. Every call site sends `""` — the merging code is
tested but unreachable.
**The feature:** A minimal per-row options affordance: a "Read-only" toggle (maps to `ro`) and,
for SMB, "Mount as my user" (maps to `uid=$UID,gid=$GID` — without which a root-performed CIFS
mount is root-owned and the user can't write to it, something every user hits immediately).
No free-text option entry (that's the injection surface flagged in `ANALYSIS_BUGS.md` 2.1) —
two checkboxes that append known-safe literals. The helper-side merge logic requires zero
changes.

### 1.3 Decide the `tokio-runtime` feature's fate by using it for async subprocess — **Medium**
**What exists:** `Cargo.toml:41` declares optional tokio with `process` enabled "for future
advanced async needs"; no code references it. Meanwhile the app's worst UX defect is the
synchronous `pkexec` call blocking the GTK main loop (`ANALYSIS_BUGS.md` 3.1).
**The feature:** Not user-visible per se, but it *is* the intended-but-unfinished async story:
make `run_helper` genuinely async. Recommended shape: skip tokio entirely, use
`gio::Subprocess` with `communicate_utf8_future` (gio is already a dependency and integrates
with the glib main context the app already runs on), then delete the tokio dependency and
feature. Either way the placeholder stops being a placeholder.

### 1.4 App identity: desktop file, icon, and a real application ID — **High**
**What exists:** `assets/drivedock.png` is committed but *never installed or referenced* —
`nix/package.nix`'s postInstall handles only the helper and the Polkit policy; there is no
`.desktop` file anywhere in the tree; the app ID is the placeholder `org.example.DriveDock`
(`src/main.rs:10`). As shipped, `nix profile add` gives users a binary that appears in no app
menu, with no icon, and a generic window class.
**The feature:** (a) settle the ID (`io.github.victorytek.DriveDock` matches the README's
install source) across `main.rs`, the Polkit policy filename/action ID, and package.nix;
(b) add `data/io.github.victorytek.DriveDock.desktop` (Name/Exec/Icon/Categories=System;Utility)
and install it to `$out/share/applications`; (c) install the PNG to
`$out/share/icons/hicolor/…` (or better, an SVG). Pure packaging work, no code logic, and it's
the difference between "a binary" and "an app" for every GUI user.

---

## 2. Natural complements to existing code and data models

### 2.1 Live auto-refresh from `VolumeMonitor` signals — **High**
**What exists:** `list_local_drives` already holds a `gio::VolumeMonitor` (`src/system/local.rs:69`)
— the object that *emits* `volume-added`, `volume-removed`, `mount-added`, `mount-removed`
signals — but only polls it once per manual refresh. The refresh plumbing
(`refresh_drives(&group, &status)`) is already a reusable async fn, and the app already re-calls
it after its own operations.
**The feature:** In `build_content`, keep one long-lived `VolumeMonitor`, connect the four
signals, and have each fire the existing `refresh_drives` future (debounced ~500ms, since
plugging a drive emits several signals). Plugging in a USB stick then makes it appear in the
list within a second, with zero clicks — the baseline behavior of every file manager and
exactly what a drive-manager app is judged on. Estimated at well under 50 lines against
existing code.

### 2.2 "Open in Files" action on mounted rows — **High**
**What exists:** Every mounted local row knows its `mount_point: PathBuf`
(`src/system/local.rs:20`), every mounted share row has a deterministic `/mnt/<slug>` path
(`src/network/mount.rs:70-72`), and the rows already carry per-row suffix buttons with async
click handlers — the exact pattern to copy.
**The feature:** A folder-open button on mounted rows calling
`gtk::FileLauncher::new(Some(&gio::File::for_path(path))).launch(...)` (gtk 4.10+, within the
declared `v4_12` feature). The #1 thing a user does after mounting something is open it; right
now they must know the path from the subtitle and open a file manager themselves. ~20 lines per
row builder.

### 2.3 Connect to a share by address (manual entry) — **High**
**What exists:** Discovery is purely passive GVfs browsing (`src/network/discover.rs:53-73`),
which the code itself acknowledges can be empty (gvfsd not running, `:57-60`) — and mDNS/SMB
browsing routinely misses servers on other subnets, WireGuard peers, or NAS boxes with
broadcast discovery disabled. But the mount path doesn't need discovery at all:
`mount_share` takes any `NetworkShare`, and `mount_source_from_uri` (`discover.rs:154-165`)
already converts an arbitrary `smb://` / `nfs://` URI into a mount source.
**The feature:** A "+" button in the header bar (next to the existing refresh button) opening a
small `adw::AlertDialog` (the `collect_credentials` dialog at `mount.rs:278-336` is the exact
template) with one entry: a URI like `smb://nas.local/media`. Construct a `NetworkShare` from
it (`protocol_from_uri` already exists) and run the existing dock flow. This turns discovery
from a hard gate into a convenience, using only functions that already exist. Highest-value
new *capability* on this list.

### 2.4 Undockable entries for mounted-but-undiscovered shares — **Medium**
**What exists:** `scan_network_shares` already reads `/proc/mounts` (`discover.rs:63`) and both
codebases can parse its rows; the fstab tag and `/mnt/<slug>` convention make DriveDock's own
mounts identifiable.
**The feature:** After GVfs enumeration, add a synthetic `NetworkShare { mounted: true, .. }`
for every `cifs`/`nfs` `/proc/mounts` row under `/mnt/` that no discovered share matched
(reverse of `mount_source_from_uri` — `//host/share` → `smb://host/share`, a ~10-line inverse
of an existing pure function). Without this, a share mounted at boot via the app's *own*
persistence feature is invisible in the app whenever the server isn't currently browsable —
the user's most likely daily state. Fixes `ANALYSIS_BUGS.md` 1.8 as a feature.

### 2.5 Copy button for the NixOS config snippet — **Medium (trivial effort)**
**What exists:** `dock::nixos::config_snippet_message` / `network_config_snippet_message`
generate a paste-ready `fileSystems` block, displayed in a selectable status label
(`window.rs:160` `.selectable(true)`) — the design explicitly says the user should paste it
into their NixOS config.
**The feature:** When the status message contains a snippet, show a small "Copy snippet" button
beside the status icon that puts *just the code block* (not the prose) on the clipboard via
`gdk::Display::clipboard()`. To support that cleanly, have the snippet functions return
`(prose, snippet)` instead of one concatenated string — a 5-line refactor. Manually selecting a
multi-line snippet inside a wrapped label is genuinely painful; this is an afternoon of work
that completes the app's most distinctive feature (NixOS awareness).

### 2.6 Remember SMB credentials in the Secret Service keyring — **Medium**
**What exists:** A credentials dialog (`mount.rs:278-336`) that re-prompts on *every* mount of
the same share, and a root-owned plaintext file that only exists for fstab's benefit. The
GNOME platform the app already targets (libadwaita, GVfs) ships the Secret Service D-Bus API
for exactly this.
**The feature:** Add a "Remember password" check row to the existing dialog; on success, store
`{username, domain, password}` keyed by share URI via the `oo7` crate (pure-Rust Secret
Service/keyring client, no new C dependencies); on the next mount, pre-fill or skip the dialog.
Explicitly *not* replacing the helper's credentials file (fstab needs a root-readable path at
boot) — this is session convenience only. Medium effort (one new dependency, ~100 lines), but
it's the difference between the mount flow feeling native versus nagging.

---

## 3. Gaps users will expect

### 3.1 Eject/power-off for removable drives — **Medium**
**What exists:** The undock flow already unmounts via UDisks2 (`src/udisks.rs:95-104`), and the
`udisks2` crate exposes the `Drive` interface (`Eject`, `PowerOff`) on the same `Object` type
the code already resolves via `find_block_object`. The UI already distinguishes
removable-friendly icons (`icon_for_drive`, `window.rs:444-454`).
**The feature:** After a successful unmount of a drive whose UDisks2 `Drive` reports
`can-power-off`, offer (or automatically perform, matching GNOME Files' eject behavior) a
`Drive.PowerOff` so the USB disk is safe to unplug and its LED stops. "Undock" that leaves the
device spun up and busy-able isn't what users mean by eject. ~40 lines: one `Udisks` method +
one call in the undock success path.

### 3.2 Busy feedback during operations — **Medium**
**What exists:** Buttons are disabled during operations (`set_sensitive(false)` everywhere) and
re-enabled after, but nothing indicates *progress* — with the current synchronous pkexec call
the whole window freezes instead (bug 3.1). Once operations are properly async (1.3 above),
the app will need visible in-flight state, and `StatusHandle` (`window.rs:27-42`) is the
natural place.
**The feature:** Add `set_busy(&self, message)` to `StatusHandle` swapping the icon for a
`gtk::Spinner`, called at the start of every dock/undock/persist closure ("Docking
//nas/media…"). Also swap the clicked button's icon for a small spinner. This is the standard
adw pattern and makes the pkexec wait legible ("waiting for authorization…") instead of
mysterious.

### 3.3 Unlock LUKS-encrypted partitions — **Low**
**What exists:** `find_block_object` resolves any block device; the `udisks2` crate exposes the
`Encrypted` interface (`Unlock`/`Lock`) with UDisks2 handling the Polkit prompt, and the
credentials dialog provides the password-entry UI pattern. Encrypted USB drives currently
appear (VolumeMonitor surfaces them) but "Dock" fails with a raw "Not a mountable filesystem"
error.
**The feature:** When `object.filesystem()` fails but `object.encrypted()` succeeds, prompt for
the passphrase (reuse the dialog minus username/domain rows), call `Unlock`, resolve the
cleartext device object, then run the existing mount path. Low priority only because of
effort — it touches the row-building logic's assumptions — but it converts a hard failure on a
common device class into the expected flow. Defer until the window model cleanup
(`ANALYSIS_ARCH.md` 1.5) lands.

### 3.4 Confirmation before undocking a drive with open files — **Low**
**What exists:** The busy case is already classified: `UnmountError::DeviceBusy`
(`src/system/unmount.rs:10-11`, populated by the substring match at `:67-70`). It's currently
rendered as a dead-end error string.
**The feature:** On `DeviceBusy`, show an `adw::AlertDialog` ("Drive is in use — retry / force
unmount?") instead of only a status line; retry is a loop, force maps to UDisks2's
`force: true` mount option flag in `unmount(options)`. Small, but converts the most common
unmount failure from a cul-de-sac into an action.

---

## 4. Integrations/automations the structure is already set up for

### 4.1 NixOS module option to pre-declare shares (`programs.drivedock.shares`) — **Medium**
**What exists:** A real NixOS module (`nix/module.nix`) already shipping config
(`boot.supportedFilesystems`, polkit, udisks2/gvfs services), and the app already *generates*
`fileSystems` snippets as untyped text for the user to paste (`src/dock/nixos.rs`).
**The feature:** Extend the module with a typed option, e.g.
`programs.drivedock.shares = [ { mountPoint = "/mnt/media"; source = "//nas/media"; fsType = "cifs"; credentials = "/etc/drivedock/credentials/media.cred"; } ]`,
that the module lowers to `fileSystems` entries (with `_netdev` and the same option set
`build_fstab_line` uses). Then the in-app NixOS guidance can say "add this to
`programs.drivedock.shares`" — a one-line paste instead of a five-line block, and the module
becomes the declarative home the snippet currently gestures at. Nix-only work; no Rust changes
beyond the message text.

### 4.2 Desktop notifications for background-completed operations — **Low**
**What exists:** `adw::Application` (`src/main.rs:20-22`) *is* a `gio::Application`, which has
`send_notification` built in; operations already produce exactly one success/failure message
each (`StatusHandle` call sites).
**The feature:** Mirror `status.set_ok/set_error` to a `gio::Notification` only when the window
is not active (`window.is_active()` check) — relevant once mounts are async (1.3) and a slow
NFS mount finishes while the user has switched away. Deliberately gated on the async work;
pointless before it.

### 4.3 GNOME Files / Nautilus hand-off for non-mountable protocols — **Low**
**What exists:** Discovery already surfaces WebDAV/FTP/SFTP shares with correct protocol badges
(`discover.rs:36-47`), but docking them is a hard error (`mount.rs:156-162`) because the
kernel-mount design intentionally covers only cifs/nfs.
**The feature:** For those protocols, replace the Dock button with "Open in Files" — launch the
share's URI via `gtk::UriLauncher`, letting GVfs mount it session-scoped in Nautilus, which is
where those protocols belong. Turns "discovered but errors on click" into a coherent story
("DriveDock system-mounts SMB/NFS; everything else opens in your file manager") without
expanding the helper's scope by one line.

---

## Explicitly not recommended

- **`--remount-shares` headless mode + systemd user unit** (documented in
  `docs/PROJECT_STATUS.md:29-30`, never built): intentionally superseded by the fstab/`_netdev`
  design — boot-time mounting is now the init system's job. Delete the doc reference
  (see `ANALYSIS_BUGS.md`/arch 4.2) rather than build it.
- **Reimplementing discovery (Avahi/smbclient scanning)**: explicitly prohibited by
  `.github/copilot-instructions.md`; the manual-entry feature (2.3) covers the gap more cheaply.
- **Partitioning/formatting tools**: out of the stated scope; GNOME Disks exists. Everything
  above stays inside dock/undock/persist.

---

## Priority summary

| # | Feature | Priority |
|---|---------|----------|
| 1.1 | Read back persistence state into the toggle | High |
| 1.4 | Desktop file, icon install, real app ID | High |
| 2.1 | Auto-refresh from VolumeMonitor signals | High |
| 2.2 | "Open in Files" on mounted rows | High |
| 2.3 | Connect to share by address (manual URI entry) | High |
| 1.2 | Read-only / mount-as-user options (use the empty `options` plumbing) | Medium |
| 1.3 | Real async subprocess (resolve the tokio placeholder via gio::Subprocess) | Medium |
| 2.4 | Synthetic entries for mounted-but-undiscovered shares | Medium |
| 2.5 | Copy button for the NixOS snippet | Medium |
| 2.6 | Secret Service credential memory | Medium |
| 3.1 | Eject/power-off after undock | Medium |
| 3.2 | Busy spinners during operations | Medium |
| 4.1 | `programs.drivedock.shares` NixOS module option | Medium |
| 3.3 | LUKS unlock flow | Low |
| 3.4 | Busy-drive retry/force dialog | Low |
| 4.2 | Desktop notifications when unfocused | Low |
| 4.3 | Open WebDAV/FTP/SFTP in Files instead of erroring | Low |
