-- etl_soundminer_ref.lua: Port SoundMiner REF.sqlite metadata into riffgrep BEXT.
--
-- REF database variant. Differs from SUB in:
--   - Artist → Originator (vendor); always written (REF populates Artist reliably)
--   - Library → OriginatorReference; written only if non-empty
--   - Rating → POPM as star characters ("*" to "*****"); SM uses integer 1-5, -1 = unrated
--   - Key, BPM already handled by base ETL logic
--
-- Run as:
--   rfg --workflow scripts/etl_soundminer_ref.lua --no-db ~/Music/Analysis
--                                  (dry-run: shows diffs, writes nothing)
--   rfg --workflow scripts/etl_soundminer_ref.lua --no-db --commit ~/Music/Analysis
--                                  (apply changes)
--   rfg --workflow scripts/etl_soundminer_ref.lua --no-db --force --commit ~/Music/Analysis
--                                  (re-port even already-ported files)

-- ── Configuration ───────────────────────────────────────────────────────────
local SM_DB = "/Users/cmk/Library/SMDataBeta/Databases/REF.sqlite"
-- ─────────────────────────────────────────────────────────────────────────────

-- ── Early-exit guard ─────────────────────────────────────────────────────────
if not riffgrep.force and sample:is_packed() then
    return
end

-- ── Verify SM database is accessible ─────────────────────────────────────────
local db_file = io.open(SM_DB, "r")
if not db_file then
    return
end
io.close(db_file)

-- ── Look up the file in the SM database ──────────────────────────────────────
-- REF stores paths as-is (no volume prefix translation needed for local files).
local path = sample:path()

local db = sqlite.open(SM_DB, "readonly")
-- Use COLLATE NOCASE for case-insensitive path matching (macOS is case-insensitive
-- but SM and the filesystem may disagree on capitalization).
local row = db:query_one(
    "SELECT _UMID, Library, CDTitle, BPM, Key, Artist, Description, Rating " ..
    "FROM justinmetadata WHERE FilePath = ? COLLATE NOCASE",
    path
)
db:close()

if not row then
    return
end

-- ── Port fields ──────────────────────────────────────────────────────────────

-- Standard BEXT fields.
sample:set_vendor(row.Artist or "")                -- Originator (32 chars)
-- Library → OriginatorReference; fall back to CDTitle (album) if Library is empty.
local lib = row.Library
if not lib or lib == "" then
    lib = row.CDTitle
end
if lib and lib ~= "" then
    sample:set_library(lib)                        -- OriginatorReference (32 chars)
end

-- Packed Description fields.
sample:set_key(row.Key                 or "")      -- TKEY (8 chars)
sample:set_comment(row.Description     or "")      -- COMR/Comment (32 chars)

-- BPM: only set when SM has a numeric value.
if row.BPM and tonumber(row.BPM) then
    sample:set_bpm(math.floor(tonumber(row.BPM)))  -- TBPM (4 chars)
end

-- Rating: SM stores integer 1-5 (-1 or nil = unrated).
-- POPM/Rating field is 4 ASCII chars; riffgrep uses star characters.
if row.Rating then
    local n = tonumber(row.Rating)
    if n and n >= 1 and n <= 5 then
        sample:set_rating(string.rep("*", n))      -- POPM (4 chars)
    end
end

-- ── Stamp SM cross-reference ─────────────────────────────────────────────────
if row._UMID then
    sample:set_bext_umid(row._UMID)
end
