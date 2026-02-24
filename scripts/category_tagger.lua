-- category_tagger.lua: Assign 4-char category codes based on description keywords.
--
-- Usage:
--   riffgrep --workflow scripts/category_tagger.lua --no-db ~/Samples        (dry-run)
--   riffgrep --workflow scripts/category_tagger.lua --no-db --commit ~/Samples
--
-- Skips files that already have a category set (non-destructive).
-- Keyword matching is case-insensitive and uses substring search.
--
-- Category codes (4 ASCII chars, right-padded with spaces to fill field width):
--   PERC  percussive hits, drums, cymbals
--   BASS  bass elements
--   SYNT  synthesizers, keys, pads, DX/FM instruments
--   LOOP  rhythmic loops
--   SFX   sound effects, ambiences
--   VOCA  vocals, voice, spoken word
--   GUIT  guitar, stringed instruments
--   ATMO  atmospheric pads, drones, textures
--   STAB  stabs, hits, one-shots
--   LEAD  melodic lead lines

-- Don't overwrite an existing category.
if sample:category() ~= "" then
    return
end

local function contains(text, pat)
    return string.find(text, pat, 1, true) ~= nil
end

-- Build a combined search corpus from all text fields.
local corpus = (
    sample:description() .. " " ..
    sample:sound_id()    .. " " ..
    sample:comment()     .. " " ..
    sample:vendor()      .. " " ..
    sample:library()
):lower()

if contains(corpus, "kick")   or
   contains(corpus, "snare")  or
   contains(corpus, "hihat")  or
   contains(corpus, "hi-hat") or
   contains(corpus, "hi hat") or
   contains(corpus, "cymbal") or
   contains(corpus, "clap")   or
   contains(corpus, "tom")    or
   contains(corpus, "perc")   or
   contains(corpus, "drum")
then
    sample:set_category("PERC")

elseif contains(corpus, "bass") or
       contains(corpus, "sub")
then
    sample:set_category("BASS")

elseif contains(corpus, "synth")  or
       contains(corpus, "yamaha") or
       contains(corpus, " dx ")   or
       contains(corpus, "dx-")    or
       contains(corpus, "moog")   or
       contains(corpus, "juno")   or
       contains(corpus, "oberheim") or
       contains(corpus, "prophet") or
       contains(corpus, "chord")  or
       contains(corpus, "pad ")   or
       contains(corpus, "keys")
then
    sample:set_category("SYNT")

elseif contains(corpus, "loop") or
       contains(corpus, "groove") or
       contains(corpus, "beat")
then
    sample:set_category("LOOP")

elseif contains(corpus, " sfx")   or
       contains(corpus, "effect")  or
       contains(corpus, "foley")   or
       contains(corpus, "ambien")  or
       contains(corpus, "texture")
then
    sample:set_category("SFX ")

elseif contains(corpus, "vocal")  or
       contains(corpus, "voice")   or
       contains(corpus, " vox")    or
       contains(corpus, "spoken")  or
       contains(corpus, "choir")
then
    sample:set_category("VOCA")

elseif contains(corpus, "guitar") or
       contains(corpus, "strum")   or
       contains(corpus, "string")
then
    sample:set_category("GUIT")

elseif contains(corpus, "atmosphere") or
       contains(corpus, "atmospheric") or
       contains(corpus, "drone")        or
       contains(corpus, "ambient")
then
    sample:set_category("ATMO")

elseif contains(corpus, "stab") or
       contains(corpus, "hit")  or
       contains(corpus, "shot")
then
    sample:set_category("STAB")

elseif contains(corpus, "lead") or
       contains(corpus, "melody") or
       contains(corpus, "melodic")
then
    sample:set_category("LEAD")

end
