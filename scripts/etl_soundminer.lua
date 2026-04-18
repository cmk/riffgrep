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

-- Comment: prefer the SM DB Description, but rescue any plain-text BEXT
-- Description already on the file before auto-activation overwrites bytes
-- [0:44]. On Samples-From-Mars-style libraries SM typically writes a short
-- source tag (e.g. "Roland TR-606") to the file's BEXT Description while
-- SM DB's Description column holds a different note (e.g. "Akai MPC-60").
-- The field holds 32 ASCII bytes; the Rust setter truncates — no need to
-- size-check in Lua.
local db_desc   = row.Description     or ""
local file_desc = sample:description() or ""
local comment
if file_desc == "" then
    comment = db_desc
elseif db_desc == "" then
    comment = file_desc
elseif string.find(db_desc, file_desc, 1, true) then
    comment = db_desc       -- file text already subsumed by DB text
elseif string.find(file_desc, db_desc, 1, true) then
    comment = file_desc     -- DB text already subsumed by file text
else
    comment = file_desc .. ", " .. db_desc   -- distinct — combine, Rust truncates
end
sample:set_comment(comment)

if sample:vendor()  == "" then sample:set_vendor(row.Artist  or "") end
if sample:library() == "" then sample:set_library(row.Library or "") end

-- BPM: only set when SM has a numeric value (preserve existing if SM lacks it).
if row.BPM and tonumber(row.BPM) then
    sample:set_bpm(math.floor(tonumber(row.BPM)))
end

-- ── Stamp SM cross-reference ──────────────────────────────────────────────────
-- Write the SM _UMID into the 64-byte BEXT UMID field. Empirically (see the
-- _sm.wav fixtures in test_files/), SM does NOT write the BEXT UMID itself —
-- every SM-roundtripped file observed had the field left all-zero. So this
-- stamp is effectively the only way to cross-reference the file back to SM's
-- justinmetadata row after the port.
if row._UMID then
    sample:set_bext_umid(row._UMID)
end
