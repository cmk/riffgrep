"""P2.5 — hypothesis property tests for `embed_preprocess` transforms.

Per CLAUDE.md: "Property-based testing is mandatory for any module
that parses, encodes, or transforms data." `embed_preprocess.py`'s
pure helpers (`_trim_silence`, `_fit_window`, `_peak_normalize`) are
transforms on float32 arrays with invariants that are awkward to
enumerate via example tests but easy to state as properties.

The top-level `preprocess(path)` function is not property-tested
here — it reads audio from disk, so its pure behavior is composed
from the helpers. If a property holds for every helper and the
composition order is stable, the end-to-end transform inherits the
guarantees.
"""

from __future__ import annotations

import numpy as np
from hypothesis import assume, given
from hypothesis import strategies as st
from hypothesis.extra import numpy as hnp

import embed_preprocess


# --- Generators -------------------------------------------------------

# Keep arrays small enough that property runs stay fast; the invariants
# we care about are scale-invariant so 1..4096 samples is plenty.
_AUDIO_LEN = st.integers(min_value=1, max_value=4096)
_AUDIO_ELEMS = st.floats(
    min_value=-2.0, max_value=2.0, allow_nan=False, allow_infinity=False, width=32
)


def _audio_arrays() -> st.SearchStrategy[np.ndarray]:
    return _AUDIO_LEN.flatmap(
        lambda n: hnp.arrays(dtype=np.float32, shape=n, elements=_AUDIO_ELEMS)
    )


# --- _fit_window ------------------------------------------------------


@given(audio=_audio_arrays(), n_samples=st.integers(min_value=1, max_value=4096))
def test_fit_window_output_length_is_n_samples(
    audio: np.ndarray, n_samples: int
) -> None:
    """Post-condition: output length is exactly `n_samples` regardless of
    the input length. This is the invariant the CLAP encoder depends on
    — it wants a fixed-length window."""
    out = embed_preprocess._fit_window(audio, n_samples)
    assert out.shape == (n_samples,)


@given(audio=_audio_arrays(), n_samples=st.integers(min_value=1, max_value=4096))
def test_fit_window_preserves_dtype(audio: np.ndarray, n_samples: int) -> None:
    """Dtype must stay float32 — downstream stacking via np.stack would
    raise otherwise."""
    out = embed_preprocess._fit_window(audio, n_samples)
    assert out.dtype == audio.dtype


@given(
    audio=_audio_arrays(), n_samples=st.integers(min_value=1, max_value=4096)
)
def test_fit_window_pad_is_trailing_zeros(
    audio: np.ndarray, n_samples: int
) -> None:
    """When the input is shorter than the window, padding is trailing
    zeros, not prepended or interleaved."""
    if len(audio) >= n_samples:
        return
    out = embed_preprocess._fit_window(audio, n_samples)
    np.testing.assert_array_equal(out[: len(audio)], audio)
    np.testing.assert_array_equal(out[len(audio) :], np.zeros(n_samples - len(audio), dtype=audio.dtype))


# --- _peak_normalize --------------------------------------------------


@given(audio=_audio_arrays())
def test_peak_normalize_output_peak_matches_target(audio: np.ndarray) -> None:
    """Post-condition: for non-silent input, the maximum absolute value
    of the output equals `_db_to_amp(target_db)` within float tolerance.
    This is what lets the downstream model see a consistent loudness."""
    assume(float(np.max(np.abs(audio))) > 0.0)

    target_db = embed_preprocess.PEAK_DB
    out = embed_preprocess._peak_normalize(audio, target_db)
    expected_peak = embed_preprocess._db_to_amp(target_db)
    actual_peak = float(np.max(np.abs(out)))
    # Allow 1% relative tolerance — float32 rounding on large arrays can
    # push the peak slightly off the target after the division.
    assert abs(actual_peak - expected_peak) / expected_peak < 0.01, (
        f"peak {actual_peak} vs target {expected_peak} on len={len(audio)}"
    )


@given(audio=_audio_arrays())
def test_peak_normalize_shape_preserved(audio: np.ndarray) -> None:
    """Peak normalization is an elementwise scalar multiply — shape and
    dtype must survive."""
    out = embed_preprocess._peak_normalize(audio, embed_preprocess.PEAK_DB)
    assert out.shape == audio.shape


def test_peak_normalize_silent_input_unchanged() -> None:
    """A truly-zero input can't be normalized (division by zero). The
    function must return it as-is rather than producing NaNs."""
    silent = np.zeros(128, dtype=np.float32)
    out = embed_preprocess._peak_normalize(silent, embed_preprocess.PEAK_DB)
    np.testing.assert_array_equal(out, silent)


# --- _trim_silence ----------------------------------------------------


@given(audio=_audio_arrays())
def test_trim_silence_output_is_contiguous_subsequence(
    audio: np.ndarray,
) -> None:
    """The trim function can only drop leading/trailing samples — it
    must not reorder or inject values. For any input, the output must
    appear as a contiguous slice of the input."""
    threshold = embed_preprocess._db_to_amp(embed_preprocess.SILENCE_DB)
    out = embed_preprocess._trim_silence(audio, threshold)
    assert len(out) <= len(audio)
    if len(out) == 0:
        return
    # Find out as a slice of audio. If not found, the function reordered
    # or modified values, which it shouldn't.
    n = len(out)
    found = False
    for start in range(len(audio) - n + 1):
        if np.array_equal(audio[start : start + n], out):
            found = True
            break
    assert found, "trimmed output is not a contiguous slice of input"


@given(audio=_audio_arrays())
def test_trim_silence_all_silent_returns_empty(audio: np.ndarray) -> None:
    """When every sample is below the silence threshold, the trim must
    return an empty array so `preprocess`'s downstream `len == 0` guard
    can convert to None. Otherwise `_peak_normalize` amplifies
    sub-threshold noise to -1 dBFS — a bug fixed in round 3 of PR #10."""
    threshold = embed_preprocess._db_to_amp(embed_preprocess.SILENCE_DB)
    assume(float(np.max(np.abs(audio))) <= threshold)
    out = embed_preprocess._trim_silence(audio, threshold)
    assert len(out) == 0


@given(audio=_audio_arrays())
def test_trim_silence_nonsilent_boundaries_exceed_threshold(
    audio: np.ndarray,
) -> None:
    """When the input contains at least one sample above threshold, the
    first and last samples of the trimmed output must themselves exceed
    the threshold — otherwise the trim didn't actually trim."""
    threshold = embed_preprocess._db_to_amp(embed_preprocess.SILENCE_DB)
    assume(float(np.max(np.abs(audio))) > threshold)
    out = embed_preprocess._trim_silence(audio, threshold)
    assert len(out) > 0
    assert float(np.abs(out[0])) > threshold
    assert float(np.abs(out[-1])) > threshold


# --- Composed pipeline ------------------------------------------------


@given(audio=_audio_arrays(), window=st.integers(min_value=1, max_value=4096))
def test_composed_pipeline_length_and_peak(
    audio: np.ndarray, window: int
) -> None:
    """End-to-end property over the helpers in the order `preprocess`
    uses them: _trim_silence → _fit_window → _peak_normalize. The final
    array must be `window` samples long with peak at or below
    `_db_to_amp(PEAK_DB)` (lower is possible if fit_window padded with
    zeros and the non-zero prefix was already below peak)."""
    threshold = embed_preprocess._db_to_amp(embed_preprocess.SILENCE_DB)
    trimmed = embed_preprocess._trim_silence(audio, threshold)
    if len(trimmed) == 0:
        # Pipeline short-circuits to None at this point. Skip composition.
        return
    windowed = embed_preprocess._fit_window(trimmed, window)
    assert windowed.shape == (window,)

    normalized = embed_preprocess._peak_normalize(
        windowed, embed_preprocess.PEAK_DB
    )
    assert normalized.shape == (window,)
    peak = float(np.max(np.abs(normalized)))
    target = embed_preprocess._db_to_amp(embed_preprocess.PEAK_DB)
    # Peak may be below target when fit_window padded zeros to reach the
    # window length; it may not exceed the target.
    assert peak <= target * 1.01
