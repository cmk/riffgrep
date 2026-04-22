# Review Calibration Examples

These examples demonstrate the review style we want: **cite the contract (docs,
plan, or naming), show how the code violates it, and name the consequence.**
Each example is a real review comment from an independent reviewer on a Rust
codebase. They are injected into the code-reviewer subagent prompt as few-shot
calibration.

---

## Pattern 1: Doc says X, code does Y

> **File:** `crates/foo-api/src/lib.rs`
>
> **Diff context:**
> ```
> +/// Cloneable handle for consumers to receive events.
> +///
> +/// Obtain via [`channel`]. Call [`Handle::subscribe`] to get an independent
> +/// copy that starts receiving from the current moment.
> ```
>
> **Comment:** The `Handle` doc comment says "Cloneable handle", but
> `Handle` is not `Clone` (and `broadcast::Receiver` isn't cloneable).
> Consider rewording to something like "Resubscribable handle" or explicitly
> describing that cloning semantics are provided via `subscribe()`.

**Why this is good:** Cites the exact doc string, names the specific trait that
makes it false, and proposes a concrete fix. No ambiguity.

---

## Pattern 2: Contract vs implementation mismatch

> **File:** `crates/foo-api/src/lib.rs`
>
> **Diff context:**
> ```
> +                let record = parse_entry(name, l1, l2).expect("stored entry must parse");
> +                let state = compute_or_zero(&record, target);
> +                (i as u32, state.position)
> ```
>
> **Comment:** The `predict()` docs say invalid entries are "silently omitted",
> but the implementation uses `compute_or_zero()` and still includes the zero
> result in `states`, which can produce bogus downstream values. Either switch
> to `compute()` and `filter_map` away errors, or explicitly filter out zero
> results before building output (and update the docs accordingly).

**Why this is good:** Quotes the doc contract ("silently omitted"), shows the
code contradicts it (`compute_or_zero` includes zeros), and names the
downstream consequence. Offers two concrete fix paths.

---

## Pattern 3: Duplicated logic with drift risk

> **File:** `crates/foo-sim/src/systems/bridge.rs`
>
> **Diff context:**
> ```
> +            let latency_ms = distance_km / SPEED_OF_LIGHT_KM_S * 1_000.0;
> ```
>
> **Comment:** `SPEED_OF_LIGHT_KM_S` is imported from `foo_api`, but
> `foo_core` now also defines the same constant. Keeping two independent
> copies across crates risks them drifting (and makes it unclear which one is
> canonical). Prefer sourcing this constant from a single crate and
> re-exporting if needed for API ergonomics.

**Why this is good:** Identifies a structural concern (duplicated constant)
and names the specific risk (drift, unclear canonical source). The fix is
actionable and proportionate.

---

## Pattern 4: Semantic bug from wrong variable

> **File:** `crates/foo-core/src/lib.rs`
>
> **Diff context:**
> ```
> +    let p_rx_dbm = tx.power_dbm + g_tx + g_rx - tx.static_loss_db - l_p - l_fs;
> +    let link_up = p_rx_dbm > tx.threshold_dbm;
> ```
>
> **Comment:** `compute_budget` determines `link_up` using `tx.threshold_dbm`,
> but the threshold is defined as a minimum *received* power, so it should be
> compared against the receiver's threshold. As written, `link_up` will be
> wrong whenever TX/RX thresholds differ.

**Why this is good:** Identifies a subtle domain bug (wrong side's threshold),
explains why it's wrong (threshold is a receiver-side concept), and states the
consequence precisely.

---

## Pattern 5: Test assumption that can silently break

> **File:** `crates/foo-sim/src/systems/propagator.rs`
>
> **Diff context:**
> ```
> +            {
> +                prop_assert_eq!(id_a, id_b);
> +                prop_assert_eq!(pos_a, pos_b, "position mismatch for id {}", id_a);
> +                prop_assert_eq!(vel_a, vel_b, "velocity mismatch for id {}", id_a);
> +            }
> ```
>
> **Comment:** This test assumes query iteration order is identical between
> `states_a` and `states_b` and compares them via `zip()`. Iteration order
> is not an API guarantee and can change with layout changes, making the test
> potentially flaky. Prefer collecting into a map keyed by id (or sorting)
> and comparing by key instead of relying on iteration order.

**Why this is good:** Names the implicit assumption (iteration order), cites
the non-guarantee, and explains the failure mode (flaky, not wrong — the
distinction matters). Fix is concrete and proportionate.

---

## Pattern 6: API boundary that panics instead of returning errors

> **File:** `crates/foo-cli/src/lib.rs`
>
> **Diff context:**
> ```
> +pub fn parse_input_file(path: &PathBuf) -> Vec<(String, String, String)> {
> +    let text = fs::read_to_string(path).expect("failed to read file");
> ```
>
> **Comment:** `parse_input_file` panics on read errors
> (`expect("failed to read file")`), which makes the library API hard to
> use safely (and forces callers to crash on missing/permission errors).
> Consider returning `std::io::Result<Vec<...>>` (or a custom error) and
> taking `&Path` instead of `&PathBuf` so callers can handle failures
> gracefully.

**Why this is good:** Identifies a panic at a library boundary (not internal
code), names who's affected (callers can't recover), and bundles a related
ergonomic fix (`&Path` over `&PathBuf`) that belongs with the same change.

---

## Pattern 7: Sign convention error hiding in docs

> **File:** `crates/foo-sim/src/components.rs`
>
> **Diff context:**
> ```
> +/// To test whether a peer at position `p` is not occluded by the body,
> +/// transform the unit vector toward `p` into the body frame and check
> +/// that its Z component is negative (i.e. the peer is in the lower hemisphere).
> ```
>
> **Comment:** The docs define body `+Z = down`, but then say to check
> the body-frame Z component is *negative* to confirm the peer is in the
> lower hemisphere. If `+Z` is down, then the lower hemisphere corresponds
> to a *positive* Z component. Please clarify/correct the sign convention
> so future checks don't get implemented backwards.

**Why this is good:** Catches a doc error that will cause a *future* bug when
someone implements the check by following the docs. The reviewer traces
the sign convention through two definitions and shows the contradiction.

---

## Pattern 8: Inconsistency across systems

> **File:** `crates/foo-sim/src/systems/bridge.rs`
>
> **Diff context:**
> ```
> +                transitions.push(Event::Up { id: fwd_key });
> +            } else if !budget.link_up && was_up {
> +                transitions.push(Event::Down { id: fwd_key });
> +            }
> +            bridge.prev_up.insert(fwd_key, budget.link_up);
> ```
>
> **Comment:** `budget_system` applies hysteresis using `drop_threshold`
> for previously-up links, but `bridge_system` reports `link_up` straight
> from `compute_budget` (acquisition threshold only). This can make bridge
> events disagree with the system's own state in the hysteresis band.
> Consider mirroring the hysteresis logic in both places.

**Why this is good:** Identifies an inconsistency between two systems that
*should* agree but don't. Names the specific parameter space where they
diverge, and explains the observable consequence. This is the kind of
cross-system bug that unit tests per-system would never catch.
