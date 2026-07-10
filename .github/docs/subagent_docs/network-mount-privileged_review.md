# DriveDock: Privileged Network Share Mounting — Phase 3 Review

Status: REVIEW COMPLETE
Reviewer: Orchestrating Agent (Phase 3 subagent)
Date: 2026-07-09
Spec: `.github/docs/subagent_docs/network-mount-privileged_spec.md`
Scope: `src/bin/drivedock-mount-helper.rs`, `src/network/mount.rs`, `src/network/discover.rs`,
`data/polkit/org.example.DriveDock.mount-helper.policy.in`, `nix/package.nix`, `nix/module.nix`,
`src/dock/nixos.rs`, `src/ui/window.rs`, and dead-code removal (`src/dock/shares.rs`,
`--remount-shares`).

---

## Summary

The implementation matches the spec's architecture closely: a minimal, dependency-light
(`std`+`libc`+`serde`) privileged helper binary, invoked via `pkexec`, receiving all
inputs — including credentials — as a single JSON object on stdin (never argv/env), with
pure/unit-tested decision logic (slug sanitization, mount-point validation, `/proc/mounts`
idempotency, fstab line build/strip) separated from the privileged I/O. All `Command`
invocations use argv arrays; no shell interpolation exists anywhere in the helper.
`mount.cifs`/`mount.nfs`/`umount` are resolved from a fixed root-owned candidate list, not
`$PATH`. The Polkit `.policy` matches the spec byte-for-byte (`auth_admin_keep`,
`@HELPER_PATH@` annotation), and `nix/package.nix`'s `substitute`-based path injection is sound.
Dead-code removal (`dock::shares`, `--remount-shares`, `interactive_mount_operation`) is clean —
no orphaned references found anywhere in the tree.

However, two **CRITICAL** security defects were found by reading the actual privileged code,
both of which the task brief specifically asked to verify and neither of which is caught by the
existing unit tests (both are edge cases the current test fixtures don't exercise):

### CRITICAL 1 — Credentials file has a world-readable window before permissions are tightened
`src/bin/drivedock-mount-helper.rs:173-182` (`write_credentials_file`):
```rust
let mut file = OpenOptions::new().write(true).create(true).truncate(true).open(&path)?;
file.write_all(contents.as_bytes())?;      // password written here
file.set_permissions(fs::Permissions::from_mode(0o600))?;   // chmod happens AFTER
```
`OpenOptions::create(true)` without `.mode(0o600)` creates the file at the process umask default
(typically `0o644`, world-readable) — the plaintext password is written to that file *before*
`set_permissions` narrows it to `0600`. Any local user (or another process) that reads
`/etc/drivedock/credentials/<slug>.cred` in that window sees the password in the clear. The spec
(§3.4, and explicitly restated in the review task's point 4) called for exactly this to be
avoided: "the credentials file's permissions are set atomically/before any data is written where
possible." Fix: use `OpenOptionsExt::mode(0o600)` at `open()` time (combined with a restrictive
process umask or `O_EXCL` semantics) so the file never exists in a world-readable state with
content in it.

### CRITICAL 2 — fstab tag matching is an unanchored substring match, not exact
`src/bin/drivedock-mount-helper.rs:409-416` (`strip_tagged_line`):
```rust
let tag = format!("# drivedock:{slug}");
fstab_contents.lines().filter(|line| !line.contains(&tag)).collect()...
```
`str::contains` matches the tag as a substring anywhere in the line, not anchored to end-of-line.
Given two docked shares with slugs `"media"` and `"media2"` (both valid per `slugify`'s
alphanumeric/`-`/`_` charset), `cmd_persist`/`cmd_unpersist` for `"media"` would also match and
strip the *unrelated* `"media2"` line, because `"# drivedock:media2"` contains `"# drivedock:media"`
as a prefix. This is exactly the "loose substring match that could accidentally strip/duplicate
unrelated lines" risk the review task flagged, and it is a real bug: toggling one share's
persistence can silently delete another share's `/etc/fstab` entry (and, transitively since
`cmd_unpersist` doesn't touch credentials for the wrong slug, leave that other mount undockable at
boot while its credentials file lingers). The existing test
(`test_strip_tagged_line_removes_only_matching_slug`) uses `"media-fileserver"` vs
`"other-share"` — non-prefix slugs — so it does not catch this. Fix: match on
`line.trim_end() == ... ` the full expected line's tag suffix anchored at the end (e.g.
`line.trim_end().ends_with(&tag)`), or otherwise anchor the comparison so `slug` can't
prefix-match a different slug.

Both are fixable with small, surgical, well-scoped changes and do not require re-architecture.

## RECOMMENDED (non-blocking)

- `src/bin/drivedock-mount-helper.rs:112` (`cmd_mount`): `fs::create_dir_all(mount_point)` runs
  before the mount, following the string-validated (not canonicalized) path. Given `/mnt` is
  root-owned `755` by default, an unprivileged attacker cannot plant a symlink there themselves,
  so this is not exploitable under the stated threat model (compromised/buggy unprivileged GUI
  caller, not an arbitrary local user with write access to `/mnt`) — but worth a one-line comment
  noting the assumption, since it's the kind of thing that quietly stops being true if `/mnt`
  permissions are ever loosened.
- Consider also anchoring the `# drivedock:<slug>` tag check with a delimiter/end-of-line boundary
  in `build_fstab_line` output comparison generally (once Critical 2 is fixed) so future readers
  don't reintroduce the same substring bug elsewhere.

---

## Verification Performed

1. **Command injection** — grepped all `Command::new(...)` call sites in the helper and
   `src/network/mount.rs`; every one builds argv via `.arg()`, none uses a shell or
   `format!()`-interpolated command string. PASS.
2. **Path traversal** — `validate_mount_point` (helper.rs:306-338) uses `Path::strip_prefix` +
   `Path::components()`, correctly rejecting `Component::ParentDir` (`..`) and multi-segment paths
   via component-based matching, not naive string prefix checks. Confirmed by tracing
   `/mnt/../etc` through `strip_prefix("/mnt")` → yields `[ParentDir, Normal("etc")]` → first
   component is `ParentDir`, hits the `_ => Err(...)` arm. Sound. Credentials path and fstab tmp
   path are built from a sanitized slug / fixed constant, not attacker-controlled strings. PASS
   (see symlink caveat noted as RECOMMENDED above).
3. **Binary resolution** — `resolve_binary` (helper.rs:362-372) iterates
   `BIN_CANDIDATE_DIRS = ["/run/current-system/sw/bin", "/usr/sbin", "/sbin"]` only; no
   `Command::new("mount.cifs")` bare-name/PATH-lookup usage anywhere. PASS.
4. **Credential handling** — password never appears in `Command::arg()` or `env::set_var`; only
   written to the credentials file and referenced via `credentials=<path>` in mount options/fstab.
   `unpersist` deletes the file (helper.rs:246-250). **However** see CRITICAL 1 — the file-creation
   permission window is not handled correctly.
5. **fstab write safety** — `with_locked_fstab` (helper.rs:258-288) uses `flock(LOCK_EX)` +
   write-to-temp + `rename()` (atomic, same directory `/etc/`). Sound mechanism. **However** see
   CRITICAL 2 — the tag matching used to decide which line to strip is not properly anchored.
6. **Idempotency** — `cmd_mount`/`cmd_unmount` check `/proc/mounts` first and no-op if already in
   the target state; `cmd_persist`/`cmd_unpersist` are naturally idempotent (strip-then-append /
   strip-and-delete-if-exists). Confirmed safe to call twice. PASS.
7. **Polkit policy** — `data/polkit/org.example.DriveDock.mount-helper.policy.in` matches spec
   §3.5 exactly (`auth_admin_keep` for all three `<defaults>` subjects, `@HELPER_PATH@`
   annotation). `nix/package.nix`'s `postInstall` uses `substitute` with a concrete `$out` path
   (correctly reasoned in-file as to why `replaceVars`/`substituteAll` wouldn't work here) and
   moves the helper to `$out/libexec/drivedock/`, out of `$out/bin`. PASS.
8. **GUI side (`src/network/mount.rs`)** — `run_helper` pipes JSON via `child.stdin` (`Stdio::piped()`
   + `write_all`), never argv; non-zero exit is converted to `anyhow!` with the helper's stderr
   surfaced. `src/ui/window.rs` call sites (lines ~582-592, ~501-506, ~537-545) all match on
   `Ok`/`Err` and call `status.set_error(...)` plus `tracing::error!` on failure — errors are not
   swallowed. `collect_credentials` returns plain `Credentials` in memory only; no disk write or
   `tracing::info!`/log call includes the password anywhere in `mount.rs`. PASS.
9. **Spec compliance** — cross-checked against spec §3.3-§3.6; implementation matches file-by-file
   (rename `gvfs.rs`→`discover.rs`, new `mount.rs`, new helper binary, Polkit policy, `nix/package.nix`/
   `module.nix` additions, `dock::nixos::network_config_snippet_message`). No undocumented
   deviations found.
10. **Dead code** — `grep -rn "dock::shares|remount-shares|gvfs::mount_share|gvfs::unmount_share|interactive_mount_operation"`
    across `src/` returns only one hit: a doc-comment in `mount.rs:275` referencing the old
    function name for historical context (not a live call site). `src/dock/shares.rs` is deleted
    (`git status` confirms `D  src/dock/shares.rs`). No `--remount-shares` flag remains in
    `main.rs`. Clean.
11. **Build validation** (run directly, not trusted from prior report) — via
    `nix-shell -p pkg-config gtk4 libadwaita glib --run "..."` since native `pkg-config` is not on
    `$PATH` in this environment:
    - `cargo check --all-targets` → `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 0.05s` — exit 0.
    - `cargo build --all-targets` → `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 0.05s` — exit 0. Both `target/debug/drivedock` and `target/debug/drivedock-mount-helper` present on disk.
    - `cargo test` → **30/30 passed, 0 failed** (15 in `drivedock` unit test binary, 15 in
      `drivedock-mount-helper` unit test binary). Verbatim tail captured; both binaries report
      `test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`.
12. **CLAUDE.md integrity** — `git diff CLAUDE.md` returned empty output. Untouched. Confirmed via
    a safe, read-only `git diff` (no `add`/`commit`/`push` run).

---

## Score Table

| Category | Score | Grade |
|----------|-------|-------|
| Specification Compliance | 96% | A |
| Best Practices | 88% | B+ |
| Functionality | 90% | A- |
| Code Quality | 90% | A- |
| Security | 68% | D+ |
| Performance | 95% | A |
| Consistency | 94% | A |
| Build Success | 100% | A |

**Overall Grade: B- (89%)**

Security is weighted down specifically for the two CRITICAL findings above: a credentials-file
world-readable race window (real secret-exposure risk, however brief) and an unanchored fstab
tag match (real risk of silently corrupting/deleting an unrelated share's boot-mount
configuration on a system with two similarly-prefixed share names) — both are the exact classes
of defect this review was commissioned to catch in a root-executed, `pkexec`-invoked binary, and
both are unaddressed by the current test suite.

---

## Verdict: NEEDS_REFINEMENT

Build is green and the overall architecture/spec-compliance is strong, but Phase 3 cannot PASS
with two live security defects in the privileged helper. Per CLAUDE.md, this triggers Phase 4
(Refinement, cycle 1 of 2): fix `write_credentials_file` to create the credentials file with
`0600` permissions atomically at creation (e.g. `OpenOptionsExt::mode(0o600)`) rather than
chmod-after-write, and fix `strip_tagged_line` (and by extension the persist/unpersist path) to
anchor the `# drivedock:<slug>` tag match to end-of-line rather than a loose substring check —
then add a regression unit test for the prefix-collision case (e.g. slugs `"media"` and
`"media2"` both present, persisting/unpersisting one must not touch the other's line). Re-run
`cargo test` to confirm the new test passes and existing 30 tests remain green, then proceed to
Phase 5 re-review.
