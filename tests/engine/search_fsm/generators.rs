//! Action generators for the search-FSM property suite.
//!
//! Each generator is parameterised so individual properties can exclude
//! inputs that would invalidate the invariant under test.

use proptest::prelude::*;

use riffgrep::engine::search_fsm::{Input, SearchFsmState};

/// Max generated query length. Short enough to keep cases fast;
/// the semantics are the same regardless of string size.
const MAX_QUERY_LEN: usize = 16;

fn arb_query() -> impl Strategy<Value = String> {
    // ASCII-only printable chars; the FSM doesn't care about unicode
    // but the serde round-trip shrinks cleaner with a bounded alphabet.
    proptest::collection::vec(0x20u8..0x7F, 0..MAX_QUERY_LEN)
        .prop_map(|bytes| String::from_utf8(bytes).unwrap())
}

fn arb_total() -> impl Strategy<Value = usize> {
    0usize..1_000_000
}

/// Base generator: any input, state-independent.
pub fn any_input(_state: &SearchFsmState) -> BoxedStrategy<Input> {
    prop_oneof![
        arb_query().prop_map(Input::QueryChanged),
        Just(Input::QueryCleared),
        Just(Input::SubmitQuery),
        Just(Input::DebounceTick),
        Just(Input::SearchStarted),
        arb_total().prop_map(|total| Input::SearchSettled { total }),
        Just(Input::SearchCancelled),
        Just(Input::SearchFailed),
        Just(Input::FireSelection),
        Just(Input::EnterSimilarityMode),
        Just(Input::ExitSimilarityMode),
    ]
    .boxed()
}

/// Like [`any_input`] but never emits `EnterSimilarityMode` or
/// `ExitSimilarityMode`. Used by R3 (no `SpawnSearch` in Similarity
/// mode) so the mode stays fixed for the length of the stream.
#[allow(dead_code)] // Referenced by prop.rs.
pub fn transitions_no_mode_toggle(_state: &SearchFsmState) -> BoxedStrategy<Input> {
    prop_oneof![
        arb_query().prop_map(Input::QueryChanged),
        Just(Input::QueryCleared),
        Just(Input::SubmitQuery),
        Just(Input::DebounceTick),
        Just(Input::SearchStarted),
        arb_total().prop_map(|total| Input::SearchSettled { total }),
        Just(Input::SearchCancelled),
        Just(Input::SearchFailed),
        Just(Input::FireSelection),
    ]
    .boxed()
}
