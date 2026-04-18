---
applyTo: "doc/**/*.md"
---

# Documentation review style

These instructions apply to any markdown file under `doc/` — design
sketches, plans, review records, session notes, reference material.
They narrow Copilot's review voice for prose content. Source code is
reviewed under the default (strict) voice; nothing here relaxes that.

## What to flag

- **Factual errors.** A formula that computes the wrong quantity, a
  claim about the code that doesn't match what the code does, a
  line-number reference that points at the wrong site, a broken
  cross-link.
- **Ambiguity that could mislead a reader.** Undefined variables in a
  formula (e.g., `SNR` without saying whether it's in dB or linear),
  a statement that could be read two ways where only one is correct.
- **Contradictions within the PR.** A code snippet that disagrees with
  the prose directly above or below it; a summary table whose values
  don't match the per-entry detail tables.
- **Stale content the PR should have updated.** If the PR claims to
  fix an item in a catalog, but the catalog's top-level summary still
  lists the item as unresolved, flag the summary.

## What to skip

- **Single-word style preferences.** British vs American spelling
  variants where both are standard English (`parameterise` /
  `parameterize`, `behaviour` / `behavior`) — do not flag unless the
  file already establishes a consistent convention and this PR breaks
  it.
- **Comment-style nits in illustrative code snippets.** Rust snippets
  in design docs (`doc/designs/`) are illustrations, not `use`
  statements a reader is expected to paste verbatim. A locator comment
  like `// my-crate::module` communicates the same thing as
  `// my_crate::module`; prefer whichever is already used consistently
  in the file.
- **Pre-existing patterns on unchanged lines.** If the PR touches file
  X and a pattern the reviewer would otherwise flag (broken cross-link,
  stale terminology, inconsistent formatting, etc.) also appears on
  lines the PR did *not* modify in file X, do not flag it. Those are
  concerns for a separate cleanup PR.
- **Symmetry / punctuation preferences** (e.g., `±5 s` vs `0 to -5 s`)
  unless the notation actually misrepresents the underlying quantity.

## How to batch

- **One comment per file for trivial nits.** If a file has two or more
  small nits (missing prefixes, spelling, formatting), combine them
  into a single inline comment citing each line number, rather than
  opening a separate thread per nit. Separate threads force the author
  to reply to each one individually and inflate review rounds.
- **Substantive findings keep their own thread.** Factual errors,
  contradictions, and ambiguities large enough to change a reader's
  understanding remain as their own top-level comments.

## Severity calibration

- **Must-fix:** would cause a reader to misunderstand the system, or
  is demonstrably wrong on the current codebase.
- **Nit (optional):** stylistic or cosmetic. Prefix the comment with
  `nit:` so the author can skim past; a reply of "deferred" closes
  the thread without a code change.
- Do not surface nits without the `nit:` prefix. The prefix is how
  the author triages batches quickly.
