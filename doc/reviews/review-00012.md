# PR #12 — ci: Gate Python tests alongside Rust

<!-- gh-id: 3104418773 -->
### Copilot on [`.github/workflows/ci.yml:55`](https://github.com/cmk/riffgrep/pull/12#discussion_r3104418773) (2026-04-18 03:38 UTC)

`actions/setup-python` caching is enabled (`cache: pip`), but the repo doesn't have a default dependency file like `requirements.txt` at the root, so the cache key likely won't track dependency changes (and may be ineffective). Consider setting `cache-dependency-path: pyproject.toml` (or a dedicated CI requirements file) so the cache key is stable and updates when deps change, or drop the cache setting if you don't want that coupling.


<!-- gh-id: 4133431153 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-18 03:38 UTC](https://github.com/cmk/riffgrep/pull/12#pullrequestreview-4133431153))

## Pull request overview

Adds CI enforcement for the Python `scripts/tests` suite by introducing a dedicated GitHub Actions job that runs alongside the existing Rust checks, preventing Python regressions from merging unnoticed.

**Changes:**
- Add a new `python` job to run `pytest scripts/tests/` on pushes to `main` and PRs targeting `main`.
- Configure Python 3.12 via `actions/setup-python@v5` with pip caching and a minimal dependency install.





<!-- gh-id: 3104418790 -->
### Copilot on [`.github/workflows/ci.yml:63`](https://github.com/cmk/riffgrep/pull/12#discussion_r3104418790) (2026-04-18 03:38 UTC)

The default test suite under `scripts/tests` mocks out `embed_encode.preprocess`, and `soundfile` is only imported lazily inside `embed_preprocess.preprocess()`, so `soundfile` isn't actually required to run these tests. Installing it can introduce extra native-library coupling (libsndfile) and slows installs; consider removing `soundfile` here, or explicitly installing the required system package (e.g., `libsndfile1`) if you want CI to cover real audio decoding.


<!-- gh-id: 3104418794 -->
### Copilot on [`.github/workflows/ci.yml:66`](https://github.com/cmk/riffgrep/pull/12#discussion_r3104418794) (2026-04-18 03:38 UTC)

To ensure the commands use the interpreter installed by `actions/setup-python` (and avoid PATH surprises), prefer `python -m pip ...` and `python -m pytest ...` instead of invoking `pip`/`pytest` directly.

