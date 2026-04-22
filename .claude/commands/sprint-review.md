---
description: Tier-1 local pre-push code review. Spawns an independent reviewer agent to examine the branch diff against origin/main and writes the review to doc/reviews/review-NNNNN.md.
argument-hint: (no args)
---

# Sprint Review — Tier 1 (Local)

You are orchestrating a **local, pre-push** code review. This is Tier 1 of
a two-tier system:

- **Tier 1 (this command):** Independent agent reviews `origin/main...HEAD` locally.
  Gate before pushing.
- **Tier 2 (GitHub):** After push, CI runs `cargo test --workspace` and
  `cargo clippy --all-targets -- -D warnings` (see
  `.github/workflows/ci.yml`). Claude Code Action and/or Copilot review
  the PR on GitHub.

Your job: gather inputs, launch the reviewer, place the output, then help
the user push if the review passes.

---

## Step 0: Autosquash any pending fixups

Per CLAUDE.md, CI-repair commits are made as `--fixup`s and must be
collapsed before review/push. Refresh the remote-tracking ref first so
the check isn't against a stale base, then scan for fixups:

```
git fetch --quiet origin main
git -c color.ui=never log --oneline origin/main..HEAD | grep -E '^[0-9a-f]+ fixup!' || true
```

If any fixups exist, run `scripts/autosquash.sh` to collapse them.
Abort if the working tree is dirty (the script checks this). After
autosquash, re-run the fixup check to confirm the branch is clean.

## Step 1: Identify the plan (optional)

Check for a plan doc associated with this branch:

```
ls -t doc/plans/plan-*.md | head -3
```

If a plan exists and is clearly related to the branch work (check dates,
topic), read it. You'll pass its text to the reviewer.

If no plan exists or none is relevant, that's fine — the review proceeds
in **code-only mode** (no plan-conformance section).

## Step 2: Collect the diff and verify prerequisites

The review always targets the current branch against `origin/main`.
Refresh the ref first so the base isn't stale:

```
git fetch --quiet origin main
git diff origin/main...HEAD
git log origin/main..HEAD --oneline
```

If the branch has not diverged from `origin/main`, abort with a
message — there's nothing to review.

**Verify the review file exists.** `/sprint-review` appends to a
file created by TDD step 7; it never creates the file itself. Get
its path:

- If a PR already exists for this branch:
  ```
  scripts/review_path.sh "$(gh pr view --json number --jq .number)"
  ```
- Otherwise (the normal pre-push case):
  ```
  scripts/review_path.sh
  ```

`review_path.sh` predicts (or accepts) the PR number and emits the
zero-padded filename — no need to compose the path by hand. Then
confirm the returned path exists **and contains a `## Summary`
section**. If either is missing, abort and tell the user to run TDD
step 7 (finalize plan + draft PR description). Do not create the
file and do not proceed to the reviewer — the PR body belongs in
that commit, not as a post-hoc fabrication by this command.

## Step 3: Gather context

Read these files and include them in the reviewer prompt:

- `CLAUDE.md` — repo conventions, workspace layout, TDD workflow, commit
  style, feature-gate conventions
- `doc/reviews/review-calibration.md` — if it exists, include as few-shot
  examples. If absent, skip (the reviewer prompt has built-in guidance).

## Step 4: Launch the reviewer

Spawn a **new agent** with `subagent_type: "feature-dev:code-reviewer"` and
`model: "sonnet"`.

The prompt must be self-contained. Include:

1. The full diff
2. The commit log
3. The repo conventions from CLAUDE.md
4. The plan text (if found), clearly labeled as optional context
5. Calibration examples from `doc/reviews/review-calibration.md` (if found)
6. The review instructions (below)

### Reviewer voice and calibration

The reviewer should write like a thorough human PR reviewer, not a checklist
robot. Good review comments share these qualities:

- **Cite the contract, then the violation.** When the plan says X and the code
  does Y, quote both. "The plan specifies `device = ["dep:tokio", ...]` as an
  optional feature gate (T1), but `Cargo.toml` lists tokio as unconditional."

- **Name the consequence.** Don't just say "this differs from the plan." Say
  what breaks: "This means `driver-motu` links tokio/russh/mdns despite only
  using HTTP+OSC, adding ~3s to clean builds."

- **Distinguish severity.** Some findings block the merge, others are
  improvement opportunities. Be explicit: "Must fix before merge" vs
  "Consider for a follow-up."

- **Don't pad.** If a section has no findings, one sentence: "All N planned
  tests are present and correctly gated." Don't invent concerns to fill space.

### Reviewer prompt template

~~~
You are reviewing code on a local feature branch before it is pushed to
GitHub. This is a pre-push quality gate — there is no PR yet. You are
reviewing the diff between `origin/main` and the branch HEAD.

You are an independent reviewer. You did not write this code and have no
context beyond what is provided here. Review what you see, not what you
assume.

## Diff (origin/main...HEAD)

{diff}

## Commit log (origin/main..HEAD)

{commit log}

## Repo conventions

{CLAUDE.md contents}

{IF plan exists:}
## Sprint plan (optional context)

{plan text}

The plan is context, not a contract. Focus on whether the code is correct,
tested, and follows conventions. If the plan specifies verification criteria
(property tests, spot checks), confirm they exist in the diff.
{END IF}

{IF calibration examples exist:}
## Examples of high-quality review comments

{doc/reviews/review-calibration.md contents}

Match this style: cite the contract (doc, plan, or naming), show how the
code violates it, and name the consequence. When something is fine, one
sentence is enough. Don't invent concerns to fill space.
{END IF}

## Review instructions

For each section, state what you found concretely. When something is wrong,
cite the specific file, line, and consequence. When something is fine, one
sentence is enough — don't pad.

### Commit Hygiene

- Does each commit leave the repo in a buildable, testable state?
- Are commit messages conventional (feat/fix/doc/test/task/debt prefix, optional scope)?
- Are commits reasonably atomic, or are unrelated changes mixed?

### Code Quality

- Does the code follow repo conventions (thiserror in libs, no unsafe,
  lints via Cargo.toml)?
- Are error messages specific enough to diagnose from a log line?
  (e.g., "qu: tcp connect: {e}" is good; "operation failed" is not)
- Is there unintended coupling between driver crates that should be
  independent? (e.g., driver-qu importing types from driver-mpc)
- Any dead code, redundant logic, or clippy-level issues?
- Were any features, config options, or feature gates described in the
  plan but absent from the implementation? This is the highest-value
  check — plans often specify build configuration that gets lost.

### Test Coverage

**Property tests are the highest-priority check.**

- For any module that parses, encodes, or transforms data: are there
  property tests? If not, flag this as a gap.
- Do fixture-gated tests use `fixture_or_skip!` from
  `project_core::testing` (return early when fixture is absent, don't
  panic, don't `#[ignore]`)?
- What edge cases do the tests miss? Be specific — "what happens if the
  TCP connection drops mid-NRPN" is useful; "more tests would be good"
  is not.

{IF plan exists:}
### Plan Conformance

- Walk each task (T1, T2, ...) and each row in the Verification table:
  was it implemented?
- For each planned test, does a corresponding test exist in the diff?
- Is there code in the diff that wasn't in the plan? Justified emergent
  requirement or undocumented scope creep?
{END IF}

### Risks

- TODOs, stubs, or placeholder implementations?
- Could any change break existing functionality in other crates?
- Security: path traversal in file operations? Command injection in SSH
  exec calls? Unsanitized input passed to shell commands?
- New dependencies justified and maintained?

### Recommendations

Separate into two lists:

**Must fix before push:**
- Issues that violate conventions, break tests, or introduce bugs.

**Follow-up (future work):**
- Improvements that are acceptable now but should be tracked.

## Output format

Structure your review as markdown with the H3 sections above. Be direct
and specific. Cite file paths and line numbers. Keep the total review under
400 lines. Prioritize by impact.
~~~

## Step 5: Place the output

Append (never overwrite) a dated section to the already-existing
`doc/reviews/review-NNNNN.md`, below the `## Summary` and any prior
review rounds:

```markdown
## Local review (YYYY-MM-DD)

**Branch:** <branch>
**Commits:** <count> (origin/main..<branch>)
**Reviewer:** Claude (sonnet, independent)

---

{reviewer output}
```

Then:

1. **Print a summary** to the conversation: how many must-fix items,
   how many follow-ups, and the path to the review file. One paragraph
   max.

2. **If zero must-fix items:**
   Tell the user the branch is clear to push. Offer to push and open
   the PR (but don't do it without confirmation). The PR-create
   command is:

   ```
   gh pr create --title "<title>" \
     --body-file <(scripts/extract_pr_body.sh NNNNN)
   ```

   This makes the GitHub body a direct copy of the `## Summary`
   section — the two can't drift. Remind the user that Tier 2 (CI +
   GitHub review) will run automatically on the PR.

3. **If must-fix items exist:**
   Stop. Do not push. Do not offer to fix the issues. The user reads
   the review and decides what to do next.
