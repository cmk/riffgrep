"""Shared audio preprocessing for CLAP embedding.

Loads an audio file and returns a mono, 48kHz, peak-normalized,
fixed-window numpy float32 array suitable for LAION-CLAP inference.

The preprocessing pipeline is intentionally conservative:

1. Load via soundfile (falls back to librosa's audioread for non-PCM).
2. Mono mixdown (mean across channels).
3. Resample to 48 kHz if needed.
4. Trim leading and trailing samples below -60 dBFS.
5. Window to `window_seconds` (pad with zeros if shorter, center-truncate
   if longer).
6. Peak-normalize to -1 dBFS.

LOOP-category handling (onset splitting) is intentionally NOT here — the
caller filters LOOP rows out via the SQL query. See
doc/plans/plan-2026-04-17-01.md.
"""

from __future__ import annotations

from pathlib import Path

import numpy as np

TARGET_SR = 48_000
SILENCE_DB = -60.0
PEAK_DB = -1.0


def _db_to_amp(db: float) -> float:
    return 10.0 ** (db / 20.0)


def _trim_silence(audio: np.ndarray, threshold: float) -> np.ndarray:
    """Strip leading/trailing samples with absolute amplitude below threshold."""
    mask = np.abs(audio) > threshold
    if not mask.any():
        return audio  # all silence — let caller decide
    first = int(np.argmax(mask))
    last = len(mask) - int(np.argmax(mask[::-1]))
    return audio[first:last]


def _fit_window(audio: np.ndarray, n_samples: int) -> np.ndarray:
    if len(audio) == n_samples:
        return audio
    if len(audio) > n_samples:
        start = (len(audio) - n_samples) // 2
        return audio[start : start + n_samples]
    pad = n_samples - len(audio)
    return np.pad(audio, (0, pad))


def _peak_normalize(audio: np.ndarray, target_db: float) -> np.ndarray:
    peak = float(np.max(np.abs(audio)))
    if peak <= 0.0:
        return audio
    return audio * (_db_to_amp(target_db) / peak)


def preprocess(
    path: Path | str,
    *,
    target_sr: int = TARGET_SR,
    window_seconds: float = 2.0,
) -> np.ndarray | None:
    """Load and preprocess a single audio file.

    Returns a (n_samples,) float32 array, or None if the file cannot be
    read or is entirely silence.
    """
    import soundfile as sf

    try:
        data, sr = sf.read(str(path), dtype="float32", always_2d=False)
    except Exception:
        return None

    if data.ndim == 2:
        data = data.mean(axis=1)
    data = data.astype(np.float32, copy=False)

    if sr != target_sr:
        import librosa

        data = librosa.resample(data, orig_sr=sr, target_sr=target_sr)
        sr = target_sr

    data = _trim_silence(data, _db_to_amp(SILENCE_DB))
    if len(data) == 0 or float(np.max(np.abs(data))) <= 0.0:
        return None

    n_samples = int(round(target_sr * window_seconds))
    data = _fit_window(data, n_samples)
    data = _peak_normalize(data, PEAK_DB)
    return data.astype(np.float32, copy=False)
