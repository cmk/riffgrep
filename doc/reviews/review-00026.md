# PR #26 — Publish rustdoc to GitHub Pages

## Summary

Adds a CI job that builds `cargo doc --no-deps --workspace` and publishes
the result to GitHub Pages on every push to `main`. After merge the crate's
rendered documentation will be browsable at
`https://cmk.github.io/riffgrep/riffgrep/`.

**Why now.** Plan 07 (`plan-2026-04-22-02.md`) is about to rewrite
`PlaybackEngine`'s mutators to route through `PlaybackFsm` and collapse the
two reverse-playback code paths. That's a sprint where reviewers benefit
from a rendered API reference, and where a README refresh is plausibly next
on the list. Getting Pages deployment working before Plan 07 lands means
the first post-Plan-07 merge already surfaces the new surface.

**What ships.**

`.github/workflows/docs.yml` — a two-job workflow:

- `build` job: checkout, `libasound2-dev` (rodio's transitive alsa dep is
  required at link time even for doc generation), rust `1.94.0` matching
  `ci.yml:29`, `Swatinem/rust-cache@v2` sharing the CI cache layout,
  `cargo doc --no-deps --workspace`. A tiny `target/doc/index.html`
  redirect stub points the Pages root at the crate index (otherwise the
  root URL hits GitHub's blocked directory listing).
  `actions/upload-pages-artifact@v3` uploads `target/doc`.
- `deploy` job: `needs: build`, `environment: github-pages`,
  `permissions: pages: write + id-token: write`,
  `actions/deploy-pages@v4`.

Top-level `concurrency: group: pages, cancel-in-progress: false` per
GitHub's recommended default — a slow docs build shouldn't be cancelled
mid-upload by a newer push.

**Trigger scope.** `push` to `main` plus `workflow_dispatch` for manual
retries. Deliberately no `pull_request` trigger: Pages has a single live
deployment and PR previews would require per-PR environments + cleanup,
which isn't worth it today.

**One-time maintainer step.** Before the first deploy can succeed, the
repo owner must visit **Settings → Pages → Build and deployment → Source**
and select **GitHub Actions**. The workflow fails loudly with a clear
error if this is skipped — no silent breakage.

## Test plan

- [x] `cargo doc --no-deps --workspace` exits 0 locally (warnings only — 3
  rustdoc warnings in `src/engine/search_runner.rs` about redundant intra-doc
  link targets; `RUSTDOCFLAGS: -D warnings` is explicitly deferred in the
  plan so those don't block).
- [ ] PR is opened — `ci.yml` runs as normal; `docs.yml` does **not** fire
  (push-to-main-only).
- [ ] After the owner enables Pages and merges: `docs.yml` runs green and
  the Actions summary links to the live URL.
- [ ] `https://cmk.github.io/riffgrep/` redirects to `…/riffgrep/index.html`
  and the crate index renders.
- [ ] Spot-check: `PlaybackFsm`, `PlaybackEngine`, `MarkerFsm`, and
  `search_fsm` are navigable from the index.

## Local review (2026-04-22)

**Branch:** plan/2026-04-22-01
**Commits:** 3 (origin/main..plan/2026-04-22-01)
**Reviewer:** Claude (sonnet, independent)

---

Reviewing `git diff origin/main...HEAD` on branch `plan/2026-04-22-01`: three commits, two new doc/plan files, one new workflow. No source code touched.

### Commit Hygiene

Three commits, three correct conventional prefixes (`plan:`, `task:`, `doc:`). Order matches the TDD workflow: plan first, implementation second, finalization third. Each commit is atomic for its purpose. The only buildability concern is whether the workflow file is syntactically valid YAML — it is. No issues here.

### Workflow correctness

**Permissions.** Top-level `contents: read` is set. The deploy job carries `pages: write` and `id-token: write` at job level. This is exactly the pattern GitHub's own Pages documentation recommends and what the plan specifies. Correct.

**Concurrency.** `group: pages, cancel-in-progress: false`. Correct per plan and per GitHub's guidance for Pages deployments.

**Triggers.** `push: branches: [main]` plus `workflow_dispatch`. No `pull_request` trigger. Matches the plan's stated design. Correct.

**Action versions.** `actions/checkout@v4`, `dtolnay/rust-toolchain@v1`, `Swatinem/rust-cache@v2`, `actions/upload-pages-artifact@v3`, `actions/deploy-pages@v4`. All current as of the plan date. Correct.

**`libasound2-dev` on ubuntu-latest (Ubuntu 24.04).** The `-dev` header package is available on Noble under the same name and is what the existing `ci.yml` (line 25) already installs. No issue.

**Missing `lfs: true` on checkout.** `ci.yml` line 22 passes `lfs: true` to `actions/checkout@v4` because the test suite loads LFS-tracked fixture files. `docs.yml` omits it. The plan explicitly notes "no LFS needed for doc generation" — `cargo doc` does not open sample fixtures, so this is intentional and correct.

**Redirect stub.** `.github/workflows/docs.yml` line 39:

```
echo '<!doctype html><meta http-equiv="refresh" content="0; url=riffgrep/index.html">' > target/doc/index.html
```

The `meta http-equiv="refresh"` redirect is the standard approach for static Pages roots and will work in all current browsers. The target path `riffgrep/index.html` is relative, so it resolves correctly regardless of the Pages base URL. Correct.

**`environment.url` output reference.** The deploy job references `steps.deployment.outputs.page_url` and the step id is `deployment`. The id and reference match. Correct.

**First-deploy prerequisite.** The workflow will fail with a clear GitHub error if Pages is not enabled in Settings before the first merge. The plan documents this as T2 and the PR body repeats it. No code change can prevent it; the documentation path is correct.

**No missing steps.** The two-job structure (build → deploy) is the canonical GitHub Pages pattern. Nothing structural is missing.

### Plan/doc/PR-body conformance

**Workflow vs plan.** Every design point from T1 is implemented: both jobs, correct permissions split, correct concurrency, correct toolchain pin, `libasound2-dev`, `--no-deps --workspace`, redirect stub, upload artifact, deploy action, environment block. No deviations.

**Review section of the plan** (`doc/plans/plan-2026-04-22-01.md` lines 113–128). Accurately describes the local state: `cargo doc` exits 0 with three warnings, those warnings are acknowledged, `RUSTDOCFLAGS: -D warnings` is deferred, no design deviations. The post-merge verification note is appropriately forward-looking.

**`review-00026.md` Summary section.** Reads as a proper PR body: explains what ships, why now (Plan 07 prereq), describes both jobs with enough detail for a reviewer to understand the choices, calls out the one-time manual step, lists a concrete test plan with browser spot-checks. Style matches review-00024 and review-00025. No issues.

### Risks

**Secret leakage.** The workflow has no secrets, no environment variables beyond `CARGO_TERM_COLOR`, and no network calls beyond the standard GitHub Pages upload/deploy API. Top-level `contents: read` prevents any write-back to the repo from the build job. No leakage risk.

**Impact on existing CI.** `docs.yml` shares no resources with `ci.yml` except the Swatinem cache (keyed by `Cargo.lock` + OS + toolchain). Cache sharing is read-safe and a deliberate design choice for build speed. The two workflows are fully independent; a failure in `docs.yml` does not block or affect `ci.yml` status checks on PRs. No risk to existing CI.

**Rustdoc warnings.** Three warnings in `src/engine/search_runner.rs` (redundant intra-doc link targets) are acknowledged in both the plan and the PR body. They are cosmetic — they do not cause `cargo doc` to exit non-zero. The plan explicitly defers `RUSTDOCFLAGS: -D warnings` until the warnings are cleaned up. This is a reasonable and documented deferral, not a gap.

### Must fix before push

None. The workflow is correct, the permissions model is right, the plan and PR body are consistent, and no step will cause the first deploy to fail beyond the documented one-time Settings enablement.

### Follow-up (future work)

These are the plan's own deferred items, confirmed as appropriate deferrals — not new findings:

- Add `RUSTDOCFLAGS: -D warnings` after cleaning up the three `search_runner.rs` intra-doc link warnings.
- Add a docs-status badge to the README once the Pages URL is confirmed live.
- Per-PR preview docs when a reviewer actually asks for it.

<!-- gh-id: 3122826199 -->
### Copilot on [`.github/workflows/docs.yml:55`](https://github.com/cmk/riffgrep/pull/26#discussion_r3122826199) (2026-04-22 09:08 UTC)

In GitHub Actions, a job-level `permissions:` block replaces the workflow-level permissions rather than augmenting them. As written, the `deploy` job drops the workflow-level `contents: read`, which can cause unexpected failures (and contradicts the intent of having `contents: read` set globally). Consider adding `contents: read` to the `deploy` job (or move shared permissions to the workflow level and only override where needed).

<!-- gh-id: 3122826245 -->
### Copilot on [`doc/plans/plan-2026-04-22-01.md:14`](https://github.com/cmk/riffgrep/pull/26#discussion_r3122826245) (2026-04-22 09:08 UTC)

This references Plan 07 as `plan-2026-04-22-02.md`, but that file isn't present in `doc/plans/` in this PR. If the intent is to point at an existing plan, update the reference; otherwise consider rephrasing to avoid implying a navigable in-repo document (or add the referenced plan file in the same PR).
```suggestion
suite. It prefaces the planned Plan 07 follow-on work so that by the time
Plan 07 lands the engine-wiring changes, the published docs already reflect
the new `PlaybackEngine` / `PlaybackFsm` surface and reviewers can browse the
API in a rendered form.
```

<!-- gh-id: 3122826280 -->
### Copilot on [`doc/reviews/review-00026.md:15`](https://github.com/cmk/riffgrep/pull/26#discussion_r3122826280) (2026-04-22 09:08 UTC)

This refers to Plan 07 as `plan-2026-04-22-02.md`, but that file isn't present in `doc/plans/` in this PR. Consider either updating this to point at an existing plan file, or rewording so it doesn't read like a navigable cross-reference until the plan exists in-repo.
```suggestion
**Why now.** Plan 07 is about to rewrite `PlaybackEngine`'s mutators to
route through `PlaybackFsm` and collapse the two reverse-playback code
paths. That's a sprint where reviewers benefit from a rendered API
reference, and where a README refresh is plausibly next on the list.
Getting Pages deployment working before Plan 07 lands means the first
post-Plan-07 merge already surfaces the new surface.
```

<!-- gh-id: 4153446282 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-22 09:08 UTC](https://github.com/cmk/riffgrep/pull/26#pullrequestreview-4153446282))

## Pull request overview

Adds automated rustdoc publishing to GitHub Pages so rendered API docs are available after merges to `main`, with accompanying plan/review documentation.

**Changes:**
- Adds a GitHub Actions workflow to build `cargo doc --no-deps --workspace` and deploy the artifact to GitHub Pages.
- Adds a sprint plan describing the workflow design, trigger scope, and verification steps.
- Adds a review record documenting the local review and checklist.

### Reviewed changes

Copilot reviewed 3 out of 3 changed files in this pull request and generated 3 comments.

| File | Description |
| ---- | ----------- |
| `.github/workflows/docs.yml` | New “Docs” workflow that builds rustdoc and deploys it to GitHub Pages. |
| `doc/plans/plan-2026-04-22-01.md` | Plan for the Pages-based rustdoc publishing task, including verification/deferred items. |
| `doc/reviews/review-00026.md` | PR review record summarizing rationale, workflow structure, and test plan. |





<!-- gh-id: 3122892610 -->
#### ↳ cmk ([2026-04-22 09:19 UTC](https://github.com/cmk/riffgrep/pull/26#discussion_r3122892610))

Fixed in 7d6d635 — added `contents: read` to the deploy job's permissions block. `deploy-pages@v4` doesn't strictly need it (no checkout; only hits the Pages API), but restating keeps the least-privilege pattern explicit at each job boundary.

<!-- gh-id: 3122893537 -->
#### ↳ cmk ([2026-04-22 09:19 UTC](https://github.com/cmk/riffgrep/pull/26#discussion_r3122893537))

Intentional forward reference. `plan-2026-04-22-02.md` is already committed on the sibling branch `plan/2026-04-22-02` and opens as the follow-up PR right after this one — the two are a paired sprint where this CI work prefaces Plan 07's engine-wiring changes. Keeping the filename so the link resolves once both land on main.

<!-- gh-id: 3122894226 -->
#### ↳ cmk ([2026-04-22 09:19 UTC](https://github.com/cmk/riffgrep/pull/26#discussion_r3122894226))

Same as on the plan file — `plan-2026-04-22-02.md` is committed on `plan/2026-04-22-02` and opens as the immediate follow-up PR. Keeping the filename in the 'why now' paragraph so the forward-reference is navigable once the companion PR merges.
