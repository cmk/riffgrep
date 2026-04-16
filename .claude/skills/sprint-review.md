---
name: sprint-review
description: >
  Local pre-push code review (Tier 1). Spawns an independent reviewer agent
  to examine the branch diff against main. Use when the user says
  "/sprint-review", "review the branch", "review before push", or after
  completing work on a feature branch before pushing to GitHub.
---

# Sprint Review — Tier 1 (Local)

You are orchestrating a **local, pre-push** code review. This is Tier 1 of
a two-tier system:

- **Tier 1 (this skill):** Independent agent reviews `main...HEAD` locally.
  Gate before pushing.
- **Tier 2 (GitHub):** After push, CI runs build/test/clippy/fmt. Claude
  Code Action and/or Copilot review the PR on GitHub.

Your job: gather inputs, launch the reviewer, place the output, then help
the user push if the review passes.

---

## Step 1: Identify the plan (optional)

Check for a plan doc associated with this branch:

```
ls -t doc/plans/plan-*.md | head -3
```

If a plan exists and is clearly related to the branch work (check dates,
topic), read it. You'll pass its text to the reviewer.

If no plan exists or none is relevant, that's fine — the review proceeds
in **code-only mode** (no plan-conformance section).

## Step 2: Collect the diff

The review always targets the current branch against main:

```
git diff main...HEAD
git log main..HEAD --oneline
```

If the branch has not diverged from main, abort with a message — there's
nothing to review.

## Step 3: Gather context

Read these files and include them in the reviewer prompt:

- `CLAUDE.md` — repo conventions
- `doc/refs/review-calibration.md` — if it exists, include as few-shot
  examples. If absent, skip (the reviewer prompt has built-in guidance).

## Step 4: Launch the reviewer

Spawn a **new agent** with `subagent_type: "feature-dev:code-reviewer"` and
`model: "sonnet"`.

The prompt must be self-contained. Include:

1. The full diff
2. The commit log
3. The repo conventions from CLAUDE.md
4. The plan text (if found), clearly labeled as optional context
5. Calibration examples from `doc/refs/review-calibration.md` (if found)
6. The review instructions (below)

### Reviewer prompt template

~~~
You are reviewing code on a local feature branch before it is pushed to
GitHub. This is a pre-push quality gate — there is no PR yet. You are
reviewing the diff between main and the branch HEAD.

You are an independent reviewer. You did not write this code and have no
context beyond what is provided here. Review what you see, not what you
assume.

## Diff (main...HEAD)

{diff}

## Commit log (main..HEAD)

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

{doc/refs/review-calibration.md contents}

Match this style: cite the source (doc, convention, or naming pattern),
show how the code violates it, and name the consequence.
{END IF}

## Review instructions

For each section, state what you found concretely. When something is wrong,
cite the specific file, line, and consequence. When something is fine, one
sentence is enough — don't pad.

### Commit Hygiene

- Does each commit leave the repo in a buildable, testable state?
- Are commit messages conventional (feat/fix/test/doc/task prefix)?
- Are commits reasonably atomic, or are unrelated changes mixed?

### Code Quality

- Does the code follow repo conventions (thiserror for errors, no unsafe,
  lints via Cargo.toml)?
- Are error messages specific enough to diagnose from a log line?
- Any dead code, redundant logic, or clippy-level issues?
- Is there unintended coupling between modules?

### Test Coverage

**Property tests are the highest-priority check.**

- For any module that parses, encodes, or transforms data: are there
  property tests? If not, flag this as a gap.
- Do fixture-dependent tests return early when fixtures are absent
  (not `#[ignore]`, not panic)?
- What edge cases do the tests miss? Be specific.

{IF plan exists:}
### Plan Conformance

- Walk each task/chunk in the plan: was it implemented?
- Walk each verification criterion: does a corresponding test exist?
- Is there code in the diff that wasn't in the plan? Justified emergent
  requirement or undocumented scope creep?
{END IF}

### Risks

- TODOs, stubs, or placeholder implementations?
- Could any change break existing functionality?
- Security: path traversal, command injection, unsanitized input?
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

When the reviewer agent returns:

1. **Save the review** to `doc/reviews/review-YYYY-MM-DD-nn.md` where `nn`
   is an incrementing counter for that day (01, 02, ...). Include a header:

   ```markdown
   # Review: <branch-name> (YYYY-MM-DD)

   **Branch:** <branch>
   **Commits:** <count> (main..<branch>)
   **Reviewer:** Claude (sonnet, independent)

   ---

   {reviewer output}
   ```

2. **Print a summary** to the conversation: how many must-fix items, how
   many follow-ups, and the path to the review file. One paragraph max.

3. **If zero must-fix items:**
   Tell the user the branch is clear to push. Offer to push and create a
   PR (but don't do it without confirmation). Remind them that Tier 2
   (CI + GitHub review) will run automatically on the PR.

4. **If must-fix items exist:**
   Stop. Do not push. Do not offer to fix the issues. The user reads the
   review and decides what to do next.
