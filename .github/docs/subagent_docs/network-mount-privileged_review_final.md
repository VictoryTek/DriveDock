# DriveDock: Privileged Network Share Mounting — Phase 5 Re-Review (Final)

Status: RE-REVIEW COMPLETE
Reviewer: Orchestrating Agent (Phase 5 subagent)
Date: 2026-07-09
Spec: `.github/docs/subagent_docs/network-mount-privileged_spec.md`
Prior review: `.github/docs/subagent_docs/network-mount-privileged_review.md` (NEEDS_REFINEMENT, B-/89%)
Refinement cycle verified: 1 of 2

---

## CRITICAL 1 — Credentials file permission race — VERIFIED FIXED

`src/bin/drivedock-mount-helper.rs:186-192` (`write_credentials_file`):

```rust
let mut file = OpenOptions::new()
    .write(true)
    .create(true)
    .truncate(true)
    .mode(0o600)
    .open(&path)
    .map_err(|e| format!("failed to create {}: {e}", path.display()))?;
file.write_all(contents.as_bytes())
    .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
```

`.mode(0o600)` is now supplied to `OpenOptions` before `.open()`, via `OpenOptionsExt` (imported at
line 34, `use std::os::unix::fs::OpenOptionsExt;`). The kernel applies this mode at `O_CREAT` time
(subject to umask AND-masking, which only narrows permissions further, never widens them past
`0600`), so the file cannot exist in a world-readable state at any point — `write_all` only ever
writes to an already-`0600` file descriptor. Grepped for any remaining `set_permissions`/`chmod`
call anywhere in the file: **none found** — the old create-then-chmod pattern is fully removed, not
just supplemented. No new bug (wrong mode value, wrong path) — mode is `0o600`, applied to the same
`path` that is written and returned. **FIXED.**

## CRITICAL 2 — Unanchored fstab tag substring match — VERIFIED FIXED

`src/bin/drivedock-mount-helper.rs:428-435` (`strip_tagged_line`):

```rust
fn strip_tagged_line(fstab_contents: &str, slug: &str) -> String {
    let tag = format!("# drivedock:{slug}");
    fstab_contents
        .lines()
        .filter(|line| !line.trim_end().ends_with(&tag))
        .collect::<Vec<_>>()
        .join("\n")
}
```

Traced the `"media"` vs `"media2"` scenario by hand against this exact code: for slug `"media"`,
`tag = "# drivedock:media"`. Line `"...# drivedock:media2"` trimmed does **not** `ends_with("#
drivedock:media")` (it ends with `"2"`), so it is retained. Line `"...# drivedock:media"` does
`ends_with`, so it is stripped. Collision resolved by anchoring to end-of-line rather than
substring containment. Grepped every `.contains(` call site in the file (line 342, 368, plus
test-only assertions at 524-586): none of the non-test call sites do tag/slug substring matching —
line 342 is an unrelated `".."` check, line 368 is a fixed-set membership check
(`NETWORK_FS_TYPES.contains(&fs_type)`), not attacker-influenced string prefix matching. No sibling
instance of the same bug class exists elsewhere in the file. **FIXED.**

## Regression test — VERIFIED

`test_strip_tagged_line_does_not_match_prefix_slug` (lines 553-566) builds an fstab with both a
`"media"`- and `"media2"`-tagged line, strips `"media"`, and asserts: the `"media"` line is gone
(via both a raw substring check and an explicit `ends_with` line-scan), the `"media2"` line
survives, and an unrelated `UUID=1234` line survives. Manually confirmed this test would fail
against the old `line.contains(&tag)` implementation (a `"media"` `.contains` check on the
`"# drivedock:media2"` line returns `true`, incorrectly stripping it) and passes against the
current `ends_with`-anchored code. Well-formed, genuinely exercises the collision. **CONFIRMED.**

## Other Phase 3 findings — re-checked, unaffected

Full file re-read start to finish (not just the diffed regions): command injection safety (argv-only
`Command::new(...).arg(...)`, no shell), path traversal (`validate_mount_point` via
`Path::components()`, unchanged at lines 318-350), fixed binary-resolution candidate list
(`BIN_CANDIDATE_DIRS`, unchanged at line 47), flock+atomic-rename fstab writes
(`with_locked_fstab`, unchanged at lines 270-300), and mount/unmount/persist/unpersist idempotency
(unchanged) are all still intact — the Phase 4 diff was surgically scoped to the two flagged
functions plus a doc-comment addition and the new test. `CLAUDE.md` untouched: `git diff CLAUDE.md`
returned empty output.

## Build Validation (run directly via `nix-shell -p pkg-config gtk4 libadwaita glib`)

```
cargo check --all-targets  → Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.06s   (exit 0)
cargo build --all-targets  → Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.05s   (exit 0)
cargo test:
  drivedock unittests:              15 passed; 0 failed; 0 ignored
  drivedock-mount-helper unittests: 16 passed; 0 failed; 0 ignored  (was 15 — new regression test added)
```

Total: **31/31 passed, 0 failed** (up from 30/30 in Phase 3, reflecting the one new test).

---

## Score Table

| Category | Score | Grade |
|----------|-------|-------|
| Specification Compliance | 96% | A |
| Best Practices | 95% | A |
| Functionality | 96% | A |
| Code Quality | 94% | A |
| Security | 96% | A |
| Performance | 95% | A |
| Consistency | 95% | A |
| Build Success | 100% | A |

**Overall Grade: A (96%)**

Security raised from 68% (D+) to 96% (A): both CRITICAL findings — the credentials-file
world-readable race window and the unanchored fstab tag substring match — are independently
verified fixed by direct code reading and manual trace, not merely by trusting the refinement
report. No new CRITICAL or security-relevant issue was introduced by the fix. Not a full 100%
because the RECOMMENDED symlink-assumption note (now addressed via comment at lines 112-118) still
rests on an documented-but-unenforced assumption about `/mnt` permissions, which is acceptable
under the stated threat model but keeps this at A rather than A+.

---

## Verdict: APPROVED

Both CRITICAL issues from Phase 3 are genuinely resolved with correct, minimal, surgical fixes. No
regressions in previously-passing areas. Build is green, all 31 tests pass. Proceed to Phase 6
(Preflight Validation).
