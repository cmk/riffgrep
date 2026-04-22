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
