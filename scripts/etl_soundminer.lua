-- etl_soundminer.lua: Port SoundMiner metadata into riffgrep's BEXT packed schema.
--
-- SoundMiner stores metadata in its own SQLite database (SUB.sqlite).  This script
-- queries that database for each file and stamps the results into the BEXT packed
-- Description block so riffgrep can use it natively.
--
-- Migration receipt:
--   sample:bext_umid() — non-empty once a file has been ported; the original SM
--                        _UMID hex string written into the 64-byte BEXT UMID field
--
-- Run as:
--   riffgrep --workflow scripts/etl_soundminer.lua --no-db ~/SoundMinerExport
--                                  (dry-run: shows diffs, writes nothing)
--   riffgrep --workflow scripts/etl_soundminer.lua --no-db --commit ~/SoundMinerExport
--                                  (apply changes)
--   riffgrep --workflow scripts/etl_soundminer.lua --no-db --force --commit ~/SoundMinerExport
--                                  (re-port even already-ported files)

-- ── Configuration (edit these to match your environment) ──────────────────────
local SM_DB     = "/Users/cmk/Library/SMDataBeta/Databases/SUB.sqlite"
local SM_PREFIX = "/Volumes/SSD"   -- volume prefix SoundMiner used at scan time
-- ─────────────────────────────────────────────────────────────────────────────

-- ── Early-exit guard ─────────────────────────────────────────────────────────
-- Skip already-ported files unless --force is given.
-- bext_umid() is the migration receipt: riffgrep writes the SM _UMID (a 32-char
-- hex string) into the 64-byte BEXT UMID field on first port.  That field is
-- cryptographically large enough to serve as a unique identifier.  Description-
-- field values (recid, category, etc.) are too short and too easily colliding.
if not riffgrep.force and sample:bext_umid() ~= "" then
    return
end

-- ── Verify SM database is accessible ─────────────────────────────────────────
local db_file = io.open(SM_DB, "r")
if not db_file then
    -- SM database not found — skip this file silently.
    return
end
io.close(db_file)

-- ── Look up the file in the SM database ──────────────────────────────────────
-- SM stores paths as they appeared at scan time, which may differ from the
-- current mount point.  Translate by replacing the current user prefix with
-- the SM_PREFIX.
local sm_path = sample:path():gsub("^/Users/", SM_PREFIX .. "/Users/")

local db = sqlite.open(SM_DB, "readonly")
local row = db:query_one(
    "SELECT _UMID, Category, SubCategory, Library, ShortID, " ..
    "BPM, Key, Artist, Description " ..
    "FROM justinmetadata WHERE FilePath = ?",
    sm_path
)
db:close()

-- No matching row in SM database — nothing to port.
if not row then
    return
end

-- ── Port fields (non-destructive: only fill empty fields) ────────────────────
-- In --force mode, write_metadata_changes uses a blank baseline, so ALL
-- non-empty fields in the modified sample are written regardless of their
-- current on-disk value.  The per-field guards below keep the Lua logic simple.
if sample:category()    == "" then sample:set_category(row.Category    or "") end
if sample:subcategory() == "" then sample:set_subcategory(row.SubCategory or "") end
if sample:library()     == "" then sample:set_library(row.Library      or "") end
if sample:sound_id()    == "" then sample:set_sound_id(row.ShortID     or "") end
if sample:key()         == "" then sample:set_key(row.Key              or "") end
if sample:comment()     == "" then sample:set_comment(row.Description  or "") end
if sample:vendor()      == "" then sample:set_vendor(row.Artist        or "") end

if sample:bpm() == nil and row.BPM and tonumber(row.BPM) then
    sample:set_bpm(math.floor(tonumber(row.BPM)))
end

-- ── Stamp migration receipt ───────────────────────────────────────────────────
-- bext_umid is the receipt: a non-empty value marks the file as ported.
-- It stores the SM _UMID hex string in the 64-byte BEXT UMID field, which is
-- cryptographically large enough to serve as a unique identifier.
if row._UMID then
    sample:set_bext_umid(row._UMID)
end
