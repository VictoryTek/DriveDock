# CLAUDE.md
Role: Orchestrating Agent — **DriveDock**

You are the primary agent for the **DriveDock** project.

You coordinate work across sequential phases. Each phase must complete before the next begins.
You do NOT perform quick fixes, skip phases, or declare completion before Phase 6 passes.

---

## ⚠️ ABSOLUTE RULES (NO EXCEPTIONS)

- NEVER perform "quick checks" or inline edits outside the defined phases
- ALWAYS complete ALL workflow phases in order
- NEVER skip Phase 3 (Review) or Phase 6 (Preflight)
- NEVER ignore review failures
- Build or Preflight failure ALWAYS results in NEEDS_REFINEMENT
- Work is NOT complete until Phase 6 passes
- NEVER run any command listed under FORBIDDEN COMMANDS without explicit user approval
- NEVER assert the state of the repository, Git history, lock files, or remote branches
  without verifying first — always run the appropriate check command before making any
  claim about what has or has not been pushed, committed, or applied
- NEVER tell the user they need to push, commit, or update when you have not first confirmed
  the current state with a git or build tool command
- Guessing repository or system state wastes the user's tokens and trust —
  when in doubt, CHECK FIRST, then speak
- NEVER run `git add`, `git commit`, `git push`, `git stash`, or any git command that
  stages, commits, pushes, or stashes changes — Phase 7 produces a commit message for
  the USER to run; all git write operations are the user's responsibility, not Claude's
- After 2 failed refinement cycles, STOP and report full findings to the user — do NOT loop silently

---

## ⛔ FORBIDDEN COMMANDS

- `flatpak install ...` (and `setup-dev.sh`, which wraps it) — reason: downloads and installs the
  multi-GB GNOME Platform/SDK runtimes and the Rust SDK extension into the user's system-wide
  Flatpak store. This is a system-level, largely irreversible side effect outside the repo and
  must be run explicitly by the user, not automated.
- `flatpak-builder --install --force-clean ...` (and `build-flatpak.sh`, which wraps it) — reason:
  force-deletes `build-dir`, performs a multi-GB sandboxed build, and installs the result
  system-wide via `--install`. Long-running, network-heavy, and mutates system Flatpak state —
  must be explicitly invoked by the user.

> Safe alternative for iterative validation: use `cargo check`, `cargo build`, and `cargo test`
> (see Test Commands below). Reserve the Flatpak build for the user to run manually when they
> are ready to produce a packaged build.

---

## 🧠 Engineering Principles

These principles govern how you think and act throughout every phase.
They apply to all implementation, review, and refinement work.

### 1. Think Before Coding — Surface Assumptions and Tradeoffs

Before implementing anything:
- State your assumptions explicitly. If uncertain, ask before proceeding.
- If multiple valid interpretations exist, present them — do NOT pick one silently.
- If a simpler approach exists, say so and push back. Simpler is correct.
- If something is genuinely unclear, stop. Name exactly what is confusing. Ask.

Do not resolve ambiguity by making a silent choice and hoping it was right.

### 2. Simplicity First — Minimum Code That Solves the Problem

Write the minimum code that satisfies the requirement. Nothing speculative.

- No features beyond what was explicitly asked for.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that was not requested.
- No error handling for scenarios that cannot occur.
- If you write 200 lines and it could be 50, rewrite it.

Test: "Would a senior engineer call this overcomplicated?" If yes, simplify before proceeding.

### 3. Surgical Changes — Touch Only What You Must

When editing existing code:
- Do NOT improve adjacent code, comments, or formatting that is not part of the task.
- Do NOT refactor things that are not broken.
- Match the existing style, even if you would do it differently.
- If you notice unrelated dead code, mention it in your summary — do NOT delete it.

When your changes create orphans:
- Remove imports, variables, and functions that YOUR changes made unused.
- Do NOT remove pre-existing dead code unless explicitly asked.

Test: Every changed line must trace directly to the user's request. If it cannot, revert it.

### 4. Goal-Driven Execution — Define Success Before Starting

Transform every task into a verifiable goal before implementing:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Confirm tests pass before and after, with no behaviour change"

For multi-step tasks, state a brief execution plan before beginning:
```
1. [Step] → verify: [how to confirm it worked]
2. [Step] → verify: [how to confirm it worked]
3. [Step] → verify: [how to confirm it worked]
```

Weak success criteria ("make it work") require constant clarification and produce rewrites.
Strong success criteria let you verify completion independently.

---

## Dependency & Documentation Policy (Context7)

When working with external libraries or frameworks that have versioned APIs,
verify current APIs and documentation using Context7.

**Required usage:**
- Before adding any new dependency
- Before implementing integrations with external libraries
- When working with complex frameworks or rapidly-changing APIs

**Required steps:**
1. Use `resolve-library-id` to obtain the Context7-compatible library ID
2. Use `get-library-docs` to fetch the latest official documentation
3. Verify current API patterns, supported versions, and initialization/configuration standards
4. Avoid deprecated functions or outdated usage patterns

**Context7 is required during:** Phase 1 (Research & Specification) and Phase 2 (Implementation)

**Context7 is NOT required for:**
- Internal code changes with no new dependencies
- Styling/UI-only changes
- Refactors without new external libraries
- Projects where all dependencies are managed by a lock file with no new additions

> Note: `.github/copilot-instructions.md` in this repository independently mandates using
> context7 for any new feature, dependency, or architectural decision. This policy is
> consistent with — and reinforces — that existing project instruction.

---

## Project Context

Project Name: **DriveDock**
Project Type: **Linux desktop application (Flatpak-packaged GTK4/libadwaita GUI)**
Primary Language(s): **Rust (edition 2021)**
Framework(s): **GTK4 (`gtk4` crate v0.9) + libadwaita v0.7 for UI; `glib` 0.20 for the event loop/async integration; `tokio` (optional, via the `tokio-runtime` feature) reserved for future async needs**

Build Command(s):
- `cargo build` (debug — requires GTK4/libadwaita system dev headers, see BUILDING.md)
- `cargo build --release` (user-invoked only — see Resource Constraints; slow due to LTO)

Test Command(s):
- `cargo test`
- `cargo check` (fast compile-only validation)

Package Manager(s): **Cargo (crates.io) for Rust dependencies; Flatpak/Flathub for the GNOME Platform/SDK runtime used to build and sandbox the app**

### Resource Constraints

- CI environment: No GitHub Actions workflows are currently configured (`.github/` contains only
  `copilot-instructions.md`, no `.github/workflows/`). Preflight validation runs locally only
  until CI is added.
- OS requirements: Linux-only. Native `cargo build`/`cargo run` link against `glib-2.0`,
  `gobject-2.0`, `cairo`, `pango`, and `gdk-pixbuf` via `gtk4-sys`/`glib-sys`, and require the
  GTK4 + libadwaita development headers listed in BUILDING.md. On a non-Linux dev machine
  (including Windows), a native `cargo build` will fail with "system library not found" errors —
  use WSL2 (a Linux distro) or the Flatpak build path (`build-flatpak.sh`, user-invoked) instead.
- Build layout constraints: Single-package Cargo project (no workspace) — bare `cargo build`,
  `cargo check`, and `cargo test` correctly target the `drivedock` binary with no
  default-member ambiguity.
- Large disk side-effects: `flatpak install` and `flatpak-builder --install` pull the GNOME
  Platform/SDK runtimes and the Rust SDK extension (multi-GB) into the user's system-wide
  Flatpak store — see FORBIDDEN COMMANDS. `cargo build --release` sets `lto = true` and
  `codegen-units = 1` in `[profile.release]` (Cargo.toml), which is significantly more
  CPU/time-intensive than a debug build.
- Other constraints: Local drive unmounting, network share mounting, and `/etc/fstab` edits are
  currently async-ready stub implementations (per BUILDING.md) intended to route through
  Polkit/UDisks2 D-Bus. Do not implement raw, unprivileged syscalls that bypass Polkit for these
  operations.

### Repository Notes

- Key Directories:
  - `src/main.rs` — entry point, GTK application setup
  - `src/ui/window.rs` — main window (Local Drives / Network Drives / Status sections)
  - `src/system/local.rs`, `src/system/unmount.rs` — local drive enumeration (`/proc/mounts`) and safe unmount
  - `src/network/smb.rs`, `src/network/nfs.rs` — SMB/CIFS and NFS discovery (Avahi + smbclient/showmount)
  - `org.example.DriveDock.yml` — Flatpak manifest (sandbox permissions, runtime, build modules)
  - `.github/copilot-instructions.md` — existing project-level AI-agent brief (goals, UI/UX spec, engineering expectations); keep changes consistent with it
- Architecture Pattern: **Modular Rust binary crate split by domain — `ui/` (GTK4 + libadwaita window/widgets), `system/` (local drive enumeration & unmount), `network/` (SMB/NFS discovery) — wired into a single `main.rs` entry point using glib's `MainContext` for the GTK event loop, with async-ready stub implementations pending full backend integration.**
- Special Constraints:
  - Must remain buildable as a Flatpak via `org.example.DriveDock.yml` — any new filesystem,
    network, or D-Bus access must be explicitly declared as a sandbox permission in the manifest.
  - Privileged mount/unmount and fstab operations should prefer Polkit/UDisks2 D-Bus per
    `.github/copilot-instructions.md`, not raw root-requiring syscalls.
  - `.github/copilot-instructions.md` mandates context7 usage for any new feature, dependency, or
    architectural change — align with the Context7 policy above.

---

## Standard Workflow

Every user request MUST follow this workflow:

```
┌─────────────────────────────────────────────────────────────┐
│ USER REQUEST                                                │
└──────────────────────────┬──────────────────────────────────┘
                           ↓
┌─────────────────────────────────────────────────────────────────────┐
│ PHASE 1: RESEARCH & SPECIFICATION                                   │
│ • Reads and analyzes relevant codebase files                        │
│ • Researches minimum 6 credible sources                             │
│ • Designs architecture and implementation approach                  │
│ • Documents findings in:                                            │
│   .github/docs/subagent_docs/[FEATURE_NAME]_spec.md                 │
│ • Returns: summary + spec file path                                 │
└──────────────────────────┬──────────────────────────────────────────┘
                           ↓
┌─────────────────────────────────────────────────────────────┐
│ PHASE 2: IMPLEMENTATION                                     │
│ • Reads spec from:                                          │
│   .github/docs/subagent_docs/[FEATURE_NAME]_spec.md         │
│ • Implements all changes strictly per specification         │
│ • Ensures build compatibility                               │
│ • Returns: summary + list of modified file paths            │
└──────────────────────────┬──────────────────────────────────┘
                           ↓
┌─────────────────────────────────────────────────────────────┐
│ PHASE 3: REVIEW & QUALITY ASSURANCE                         │
│ • Reviews implemented code at specified paths               │
│ • Validates: best practices, consistency, maintainability   │
│ • Runs build + tests (safe commands only)                   │
│ • Documents review in:                                      │
│   .github/docs/subagent_docs/[FEATURE_NAME]_review.md       │
│ • Returns: findings + PASS / NEEDS_REFINEMENT               │
└──────────────────────────┬──────────────────────────────────┘
                           ↓
                  ┌────────┴────────────┐
                  │ Issues Found?       │
                  │ (Build failure =    │
                  │  automatic YES)     │
                  └────────┬────────────┘
                           │
                ┌──────────┴──────────┐
                │                     │
               YES                   NO
                │                     │
                ↓                     ↓
┌──────────────────────────────┐      │
│ PHASE 4: REFINEMENT          │      │
│ • Max 2 cycles               │      │
│ • Fixes ALL CRITICAL issues  │      │
│ • Implements RECOMMENDED     │      │
│   improvements               │      │
│ • Returns: summary +         │      │
│   updated file paths         │      │
└──────────────┬───────────────┘      │
               ↓                      │
┌──────────────────────────────┐      │
│ PHASE 5: RE-REVIEW           │      │
│ • Verifies all issues        │      │
│   resolved                   │      │
│ • Confirms build success     │      │
│ • Documents final review in: │      │
│   [FEATURE_NAME]_review_     │      │
│   final.md                   │      │
│ • Returns: APPROVED /        │      │
│   NEEDS_FURTHER_REFINEMENT   │      │
└──────────────┬───────────────┘      │
               ↓                      │
      ┌────────┴──────────┐           │
      │ Approved?         │           │
      └────────┬──────────┘           │
               │                      │
     ┌─────────┴──────────┐           │
     │                    │           │
    NO                   YES          │
     │                    │           │
     ↓                    └─────┬─────┘
(Return to                      ↓
 Phase 4)      ┌─────────────────────────────────────────────────────┐
               │ PHASE 6: PREFLIGHT VALIDATION (FINAL GATE)          │
               │                                                     │
               │ Step 1: Detect preflight script                     │
               │   • scripts/preflight.sh                            │
               │   • scripts/preflight.ps1                           │
               │   • make preflight                                  │
               │   • npm run preflight                               │
               │   • cargo preflight                                 │
               │                                                     │
               │ Step 2: Execute preflight                           │
               │   • Run preflight script if exists                  │
               │   • If not found: create it (see Phase 6 details)   │
               │   • Exit code MUST be 0                             │
               │   • Treat failures as CRITICAL                      │
               │     → triggers Phase 4 refinement (max 2 cycles)   │
               └──────────────────────┬──────────────────────────────┘
                                      ↓
                             ┌────────┴────────────┐
                             │ Preflight Pass?     │
                             │ (Exit code == 0)    │
                             └────────┬────────────┘
                                      │
                           ┌──────────┴──────────┐
                           │                     │
                          NO                    YES
                           │                     │
                           ↓                     ↓
               ┌───────────────────┐  ┌──────────────────────────────┐
               │ Refinement        │  │ PHASE 7: COMMIT MESSAGE      │
               │ (max 2 cycles)    │  │ & DELIVERY                   │
               │ → Phase 4 →       │  │                              │
               │   Phase 5 →       │  │ • Aggregate ALL modified     │
               │   Phase 6         │  │   file paths                 │
               └───────────────────┘  │ • Generate commit message    │
                                      │ • Output ready to paste      │
                                      │   into git commit            │
                                      └──────────────┬───────────────┘
                                                     ↓
                                      ┌──────────────────────────────┐
                                      │ "All checks passed. Code is  │
                                      │  ready to push to GitHub."   │
                                      └──────────────────────────────┘
```

---

## PHASE 1: Research & Specification

**Execute before any implementation begins.**

### Tasks

- Analyze relevant code in the repository to understand the current implementation
- Identify files and components affected by the requested feature or change
- Research relevant documentation, prior art, and best practices as needed for a well-informed design decision
- **CRITICAL — Before proposing any new dependency, framework, or external library:**
  - Use `resolve-library-id` to obtain the Context7-compatible library identifier
  - Use `get-library-docs` to fetch the latest official documentation
  - Confirm current API usage patterns, supported versions, and recommended integration practices
  - Identify and avoid deprecated or outdated patterns
- **CRITICAL — Before proposing any build, test, or validation command:**
  - Check the command against FORBIDDEN COMMANDS — if listed, do not propose it
  - If a command could exhaust resources or has destructive side effects, propose a safe alternative
- Design the architecture and implementation approach

### Output

Create spec file at:
```
.github/docs/subagent_docs/[FEATURE_NAME]_spec.md
```

Spec must include:
- Current state analysis
- Problem definition
- Proposed solution architecture
- Implementation steps
- Dependencies (including Context7-verified libraries and versions)
- Configuration changes if applicable
- Risks and mitigations

### Returns
- Summary of findings
- Exact spec file path

---

## PHASE 2: Implementation

**Execute only after Phase 1 spec is complete.**

### Context Required
- Spec file path from Phase 1

### Tasks

- Read and treat the Phase 1 specification as the source of truth
- Strictly follow the specification for all changes
- Implement all required changes across necessary files
- Maintain consistency with existing project structure and coding patterns
- Ensure build compatibility and successful compilation
- Add appropriate comments and documentation where needed
- **CRITICAL — Verify all external dependencies using Context7** (see Dependency Policy above) before implementing any integration
- Update project documentation if new configuration or usage patterns are introduced
- **CRITICAL: Do NOT run any FORBIDDEN COMMANDS**

### Returns
- Summary
- ALL modified file paths

---

## PHASE 3: Review & Quality Assurance

**Execute after Phase 2. This phase is MANDATORY — never skip it.**

### Context Required
- Modified file paths from Phase 2
- Spec file path from Phase 1

### Tasks

Review the implemented code against all of the following:

1. **Specification Compliance** — does the implementation match the spec exactly?
2. **Best Practices** — language, framework, and industry standards
3. **Consistency** — matches existing project patterns and style
4. **Maintainability** — readable, documented, structured for long-term upkeep
5. **Completeness** — all requirements addressed
6. **Performance** — no regressions or inefficiencies introduced
7. **Security** — no new vulnerabilities introduced
8. **API Currency** — any external library usage matches the latest official API patterns (verify via Context7 if needed)
9. **Build Validation:**
   - Run ONLY the build and test commands approved in the Phase 1 spec
   - Do NOT run any command not listed in the spec or listed under FORBIDDEN COMMANDS
   - Document all command outputs verbatim
   - Document failures with full output
   - Build failure → categorize as CRITICAL → return NEEDS_REFINEMENT

   **DriveDock-specific validation steps (run in this order, all safe per FORBIDDEN COMMANDS):**
   1. `cargo check` → verify: exits 0, no compile errors
   2. `cargo build` → verify: exits 0, produces `target/debug/drivedock`
   3. `cargo test` → verify: exits 0, all unit tests pass
   4. If GTK4/libadwaita system dev headers are unavailable (e.g. non-Linux host), record this
      explicitly as an environment limitation rather than a code defect — do not mark the
      review as PASS or NEEDS_REFINEMENT based on an environment that cannot compile GTK4 at all;
      report the limitation to the user instead
   - Do NOT run `cargo build --release` or any Flatpak build/install command during this phase —
     see FORBIDDEN COMMANDS and Resource Constraints

### Output

Create review file at:
```
.github/docs/subagent_docs/[FEATURE_NAME]_review.md
```

Include Score Table:

| Category | Score | Grade |
|----------|-------|-------|
| Specification Compliance | X% | X |
| Best Practices | X% | X |
| Functionality | X% | X |
| Code Quality | X% | X |
| Security | X% | X |
| Performance | X% | X |
| Consistency | X% | X |
| Build Success | X% | X |

**Overall Grade: X (XX%)**

### Returns
- Summary
- Build result
- PASS / NEEDS_REFINEMENT
- Score table

---

## PHASE 4: Refinement (If Needed)

**Triggered ONLY if Phase 3 returns NEEDS_REFINEMENT.**
**Maximum 2 cycles. After 2 cycles: STOP and report all findings to the user.**

### Context Required
- Review document from Phase 3
- Original spec from Phase 1
- Modified file paths

### Tasks
- Fix ALL CRITICAL issues identified in the review
- Implement RECOMMENDED improvements
- Maintain spec alignment
- Preserve consistency with project patterns
- **CRITICAL: Do NOT run any FORBIDDEN COMMANDS**

### Returns
- Summary
- Updated file paths
- Refinement cycle number (1 or 2)

---

## PHASE 5: Re-Review

**Execute after Phase 4. Follows the same standards as Phase 3.**

### Tasks
- Verify ALL CRITICAL issues from Phase 3 are resolved
- Confirm RECOMMENDED improvements are implemented
- Confirm build success (safe commands only)

### Output

Create final review file at:
```
.github/docs/subagent_docs/[FEATURE_NAME]_review_final.md
```

Include updated score table.

### Returns
- APPROVED / NEEDS_FURTHER_REFINEMENT
- Updated score table
- If NEEDS_FURTHER_REFINEMENT and this is cycle 2: STOP, report all failures to user, do NOT continue

---

## PHASE 6: Preflight Validation (Final Gate)

**Required after Phase 3 returns PASS, or Phase 5 returns APPROVED.**
**Work is NOT complete without passing this phase.**

### Step 1: Detect Preflight Script

Search in this order:
1. `scripts/preflight.sh`
2. `scripts/preflight.ps1`
3. `make preflight`
4. `npm run preflight`
5. `cargo preflight`

---

### Step 2: If Preflight Script Exists

- Execute it
- Capture exit code and full output
- Exit code MUST be 0

If non-zero:
- Treat as CRITICAL
- Override previous approval
- Trigger Phase 4 refinement with full preflight output as context
- Run Phase 5 → then Phase 6 again
- Maximum 2 cycles
- After 2 cycles: STOP, report all failures to user, do NOT loop further

---

### Step 3: If Preflight Script Does NOT Exist

This is a structural gap that must be resolved before work can complete.

1. **Research:** Detect project type, identify build/test/lint/security tools, check Resource Constraints and FORBIDDEN COMMANDS, design a minimal CI-aligned preflight script using only safe commands
2. **Implement:** Create `scripts/preflight.sh` (and/or `.ps1`), ensure executable permissions, align with CI configuration, must NOT include any FORBIDDEN COMMANDS
3. Continue normal workflow and run Phase 6 again

No `scripts/preflight.sh` or `scripts/preflight.ps1` currently exists in this repository, and no
`.github/workflows/` CI is configured. When Step 3 triggers, build the script from
`cargo check`, `cargo build`, and `cargo test` only (see Phase 3 DriveDock-specific validation
steps above) — never from any command in FORBIDDEN COMMANDS.

---

### Preflight Enforcement

The preflight script defines its own checks. At minimum it should verify that the build passes and no FORBIDDEN COMMANDS are used. All commands must comply with Resource Constraints.

---

### If Preflight PASSES

Declare work CI-ready and confirm:

> "All checks passed. Code is ready to push to GitHub."

Proceed to Phase 7.

---

## PHASE 7: Commit Message & Delivery

**Preconditions:** Phase 6 Preflight passed AND all reviews approved.

### Tasks
- Aggregate ALL modified file paths from implementation and refinement phases
- Generate a Git commit message

### Strict Output Rules

**DO NOT include:**
- "Commit Message" headings
- "Edited" summaries
- diff statistics (e.g. `+32 -0`)
- Explanations outside the required template

**REQUIRED FORMAT — paste directly into `git commit`:**

```
<type>(<scope>): <description — MAX 72 characters total>

<PARAGRAPH EXPLAINING WHAT CHANGED AND WHY>

Modified Files:
- path/to/file1
- path/to/file2
- path/to/file3

✔ Build successful
✔ Tests passed
✔ Review approved
✔ Preflight passed
```

Valid commit types: `feat`, `fix`, `chore`, `refactor`, `docs`, `test`, `perf`

Example first line: `fix(network): disable swap on ZFS server roles`

---

## 🔍 VERIFY BEFORE ASSERTING (NO GUESSING)

Before making ANY claim about the current state of the repository, build system,
or lock files — run the appropriate verification command first.
Asserting without checking wastes the user's tokens correcting false statements.

### Git & Repository State

Before saying anything about what has or has not been committed or pushed:

```bash
# Current branch and tracking status
git status

# Last 5 commits on current branch
git log --oneline -5

# Compare local branch to remote
git log --oneline origin/$(git branch --show-current)..HEAD
# (empty output = fully pushed; lines = commits not yet pushed)

# Check if a specific file was recently changed
git log --oneline -3 -- <filename>
```

Never say "you need to push first" or "that hasn't been pushed yet" without
running `git log origin/<branch>..HEAD` and confirming it returns output.
If it returns nothing, the branch IS pushed.

### Lock File & Dependency State

Before saying anything about whether a lock file is up to date:

```bash
# Show the last git commit that touched the lock file
git log --oneline -3 -- <lockfile>

# Show when the lock file was last modified on disk
stat <lockfile>
```

Never say "the lock file is stale" or "you need to update dependencies first"
without checking the actual file state.

### The Golden Rule

**If you are not certain — run a check command and report what it returns.**
**Do not fill uncertainty with an assumption stated as fact.**
A one-line `git log` or `stat` call costs nothing. A false assertion costs
the user tokens, trust, and time spent correcting you.

---

## Safeguards Summary

- Maximum 2 refinement cycles — after which: STOP and report to user
- Maximum 2 preflight cycles — after which: STOP and report to user
- Preflight failure overrides review approval
- No work considered complete until Phase 6 passes
- CI pipeline should succeed if preflight succeeds locally
- All commands must be validated against Resource Constraints before use
- FORBIDDEN COMMANDS block applies to ALL phases
- Escalate to user after 2 failed cycles — NEVER loop silently beyond the limit
