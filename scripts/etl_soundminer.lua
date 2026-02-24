-- etl_soundminer.lua: Port SoundMiner metadata into riffgrep's BEXT packed schema.
--
-- SoundMiner stores metadata in its own SQLite database (SUB.sqlite).  This script
-- queries that database for each file and stamps the results into the BEXT packed
-- Description block so riffgrep can use it natively.
--
-- Migration receipt:
--   sample:is_packed() — true once a file has been ported; riffgrep writes a
--                        UUID v7 file_id into the packed BEXT Description block
--                        when the first packed field is committed.
--   sample:bext_umid() — the SM _UMID hex string written by riffgrep into the
--                        standard BEXT UMID field; used as a cross-reference
--                        key to look up records in the SM database.
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
-- is_packed() checks file_id != 0 in the packed Description block — a value
-- that only riffgrep's init_packed_and_write_markers() ever writes.  We prefer
-- this over bext_umid() ~= "" because writing the UMID field alone (a standard
-- BEXT field) does NOT initialize the packed schema: if a run writes UMID but
-- fails to write packed fields (e.g. due to a bug), bext_umid() would mark the
-- file as "done" and block all future ports.  is_packed() only becomes true
-- when at least one packed Description field has been committed, which is the
-- definitive signal that the port is complete.
if not riffgrep.force and sample:is_packed() then
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

-- ── Port fields ───────────────────────────────────────────────────────────────
-- Packed Description fields (category, sound_id, key, etc.) are always empty
-- in un-packed files — no guard needed; SM is authoritative for these fields.
-- Standard BEXT fields (vendor, library) may already be populated by SM in the
-- BEXT Originator/OriginatorRef bytes — only overwrite if currently empty.
sample:set_category(row.Category       or "")
sample:set_subcategory(row.SubCategory or "")
sample:set_sound_id(row.ShortID        or "")
sample:set_key(row.Key                 or "")
sample:set_comment(row.Description     or "")

if sample:vendor()  == "" then sample:set_vendor(row.Artist  or "") end
if sample:library() == "" then sample:set_library(row.Library or "") end

-- BPM: only set when SM has a numeric value (preserve existing if SM lacks it).
if row.BPM and tonumber(row.BPM) then
    sample:set_bpm(math.floor(tonumber(row.BPM)))
end

-- ── Stamp SM cross-reference ──────────────────────────────────────────────────
-- Write the SM _UMID into the 64-byte BEXT UMID field so the file can be
-- looked up in the SM database later.  Note: SM already writes this field
-- itself, so this is a no-op for files that SM has processed — it is kept here
-- for completeness and for files SM hasn't touched.
if row._UMID then
    sample:set_bext_umid(row._UMID)
end
