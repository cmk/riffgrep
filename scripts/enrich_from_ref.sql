-- enrich_from_ref.sql: Enrich riffgrep's index with metadata from SoundMiner REF.sqlite.
--
-- Usage:
--   sqlite3 ~/Library/Application\ Support/riffgrep/index.db < scripts/enrich_from_ref.sql
--
-- Attaches REF.sqlite as 'sm', joins on FilePath (case-insensitive),
-- and updates vendor, library, key, bpm, rating, comment, umid.

ATTACH DATABASE '/Users/cmk/Library/SMDataBeta/Databases/REF.sqlite' AS sm;

UPDATE samples
SET
    vendor  = COALESCE(NULLIF(ref.Artist, ''), samples.vendor),
    library = COALESCE(NULLIF(ref.Library, ''), NULLIF(ref.CDTitle, ''), samples.library),
    key     = COALESCE(NULLIF(ref.Key, ''), samples.key),
    bpm     = COALESCE(CAST(NULLIF(ref.BPM, '') AS INTEGER), samples.bpm),
    rating  = CASE
                WHEN CAST(ref.Rating AS INTEGER) BETWEEN 1 AND 5
                THEN SUBSTR('*****', 1, CAST(ref.Rating AS INTEGER))
                ELSE samples.rating
              END,
    comment = COALESCE(NULLIF(ref.Description, ''), samples.comment),
    umid    = COALESCE(NULLIF(ref._UMID, ''), samples.umid)
FROM sm.justinmetadata AS ref
WHERE samples.path = ref.FilePath COLLATE NOCASE;

DETACH DATABASE sm;

SELECT changes() || ' rows enriched from REF.sqlite';
