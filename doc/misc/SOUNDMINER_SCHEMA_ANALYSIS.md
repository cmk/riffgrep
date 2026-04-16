# SoundMiner Database Schema Analysis

## SoundMiner Database

Location: `/Users/cmk/Library/SMDataBeta/Databases/SUB.sqlite`

Key fields: `recid` (integer PK), `_UMID` (32-char hex), `FilePath` + `_FilePathHash` (SHA1). The BEXT UMID field embeds SoundMiner's `_UMID` for cross-referencing during migration.

Open in read-only mode with `PRAGMA query_only = ON` to avoid locking SoundMiner.

## 🎯 KEY DISCOVERIES

### 1. Primary Key Structure
```sql
`recid` integer primary key
```
- **Auto-incrementing integer**
- Example values: 257, 258, ...
- This is SM's internal record ID

### 2. SoundMiner Already Has UUIDs! 🎉

```sql
`_UMID` text  -- Unique Material Identifier
```

**Examples from database:**
- `976132720e774b668c36826386ae6505`
- `ad67b22c4e944a61b0a98d04bf25a99c`

**Format:**
- 32-character hex string
- Looks like MD5 hash format
- Each file has unique UMID

**This is HUGE!** SoundMiner already tracks files with UUIDs!

### 3. File Tracking System

**Three ways SM identifies files:**
1. `recid` - Database primary key (integer, auto-increment)
2. `_UMID` - Unique Material Identifier (32-char hex)
3. `FilePath` + `_FilePathHash` - Full path + SHA1 hash

**File Path Example:**
```
FilePath: /Volumes/SSD/Users/cmk/Music/Samples/.../file.wav
_FilePathHash: fc0550af41217f035ea0b75d3d9e7813b1af9ff7 (SHA1)
```

### 4. Metadata Fields in Database

**All the fields we care about exist:**
```sql
-- Core metadata
Category, SubCategory, CatID, CategoryFull
Library, ShortID
Artist, Performer
BPM, Key, Tempo
Description, Keywords, Notes
Usage

-- Track-related
Track (integer)
TrackYear, TrackTitle, CDTitle

-- Extended
Composer, Arranger, Conductor, Publisher
Recordist, Designer
Mood, Scene, Take
Location, Microphone, MicPerspective, Manufacturer
```

### 5. Normalized Schema with Link Tables

SM uses a normalized design:
```sql
_CategoryLink integer      -> justinrdb_Category table
_LibraryLink integer       -> justinrdb_Library table  
_ShortIDLink integer       -> justinrdb_ShortID table
_BPMLink integer           -> justinrdb_BPM table
_KeyLink integer           -> justinrdb_Key table
_UsageLink integer         -> justinrdb_Usage table
... (35+ link fields)
```

**Why this matters:**
- Deduplicates common values (efficient storage)
- Category "LOOP" stored once, referenced by ID
- Good for autocomplete in SM UI
- But means we can't just update text fields directly

### 6. Full-Text Search

```sql
CREATE TABLE fulltext (...)
CREATE INDEX justinfilePathindex ON justinmetadata (FilePath);
CREATE INDEX justinfilePathHashindex ON justinmetadata (_FilePathHash);
CREATE INDEX justinumidindex ON justinmetadata (_UMID);
```

**Triggers auto-update fulltext index:**
- On INSERT/UPDATE/DELETE
- Indexes: Category, Filename, Keywords, Pathname, SubCategory, BPM, Key, Library, ShortID, Usage

### 7. Sample Record Decoded

```
recid: 257 (SM's primary key)
_UMID: 976132720e774b668c36826386ae6505 (SM's UUID)
Filename: Birdcage_2 Bar Groove Fill_Loop 01.wav
FilePath: /Volumes/SSD/.../Birdcage_2 Bar Groove Fill_Loop 01.wav
_FilePathHash: fc0550af41217f035ea0b75d3d9e7813b1af9ff7

Category: LOOP
SubCategory: FILL
Library: Beats Afrobeat Multi-Track
ShortID: D
Artist: Drum Drops
BPM: 164
Usage: TRF

Track: (empty/null) ← Available for our use!
```

## 💡 CRITICAL IMPLICATIONS

### UUID Strategy: We Don't Need to Generate Our Own!

**SoundMiner's _UMID field:**
- ✅ Already exists for every file in database
- ✅ 32-char hex (unique identifier)
- ✅ Indexed for fast lookups
- ✅ Survives file moves (based on content, not path)

**NEW PLAN:**
Instead of generating our own UUID, **read SM's _UMID from database and store in TRCK!**

```python
# Query SM database for file
result = db.execute(
    "SELECT _UMID, recid FROM justinmetadata WHERE FilePath = ?",
    (filepath,)
)

if result:
    umid = result['_UMID']
    recid = result['recid']
    
    # Store in RIFF INFO
    tags['TRCK'] = umid  # SM's UUID
    tags['ISBJ'] = str(recid)  # SM's primary key (for quick queries)
```

**Benefits:**
- ✅ Links directly to SM database
- ✅ Can query SM by UMID later
- ✅ No need to generate/manage our own UUIDs
- ✅ Uses SM's existing tracking system
- ✅ Survives file moves (UMID likely content-based)

### Track Field Is Available!

Database shows:
```
Track: (null/empty in samples)
```

**This means:**
- `Track` field exists in database
- Currently unused in sample records
- INTEGER type (not TEXT)
- **Could conflict with our TRCK tag usage**

**Decision:**
- **Don't use database Track field** - it's integer, we need text
- **Use TRCK RIFF tag** - can store SM's _UMID (text)
- Let database Track field remain empty

## 📋 Updated UUID Strategy

### Option A: Use SM's _UMID (RECOMMENDED)

**For files IN SoundMiner database:**
```
TRCK (RIFF) = SM's _UMID (32-char hex)
ISBJ (RIFF) = SM's recid (integer as string)
```

**For files NOT YET in SM:**
```
TRCK (RIFF) = Generate temporary UUID4
ISBJ (RIFF) = empty (until SM scans it)
```

**After SM scans new file:**
```
1. Query SM database for new file
2. Get _UMID and recid
3. Update TRCK with _UMID
4. Update ISBJ with recid
```

**Benefits:**
- Direct linkage to SM database
- Can query: `SELECT * FROM justinmetadata WHERE _UMID = 'xxx'`
- Can query: `SELECT * FROM justinmetadata WHERE recid = 123`
- Handles files during and after transition
- Future-proof (keeps SM's tracking system)

### How _UMID Is Generated

Need to investigate, but likely:
- MD5 hash of audio content?
- MD5 hash of file path + audio?
- Random UUID converted to hex?

**To find out:**
```sql
-- Check if _UMID changes when file is moved
SELECT _UMID, FilePath FROM justinmetadata 
WHERE Filename = 'test_file.wav';

-- Move file, rescan in SM, check again
```

## 🚀 Implementation Plan

### Phase 1: Read SM Database

```python
import sqlite3

SM_DB = '/Users/cmk/Library/SMDataBeta/Databases/SUB.sqlite'

def get_sm_metadata(filepath):
    """Get SM's UMID and metadata for a file"""
    conn = sqlite3.connect(SM_DB)
    conn.row_factory = sqlite3.Row
    
    result = conn.execute('''
        SELECT 
            recid,
            _UMID,
            BPM,
            Key,
            Category,
            Library,
            ShortID,
            ScannedDate
        FROM justinmetadata
        WHERE FilePath = ?
    ''', (filepath,)).fetchone()
    
    conn.close()
    return dict(result) if result else None
```

### Phase 2: Tag Files

```python
def tag_file(filepath):
    # Try to get from SM database
    sm_data = get_sm_metadata(filepath)
    
    if sm_data:
        # File is in SM database - use its identifiers
        tags['TRCK'] = sm_data['_UMID']
        tags['ISBJ'] = str(sm_data['recid'])
    else:
        # File not in SM yet - generate temporary UUID
        tags['TRCK'] = str(uuid.uuid4())
        tags['ISBJ'] = ''  # Empty until SM scans it
    
    # Add other metadata
    tags['IALB'] = derive_library(filepath)
    tags['IKEY'] = derive_shortid(filepath)
    # ... etc
```

### Phase 3: Sync After SM Scan

```python
def update_umid_after_scan(filepath):
    """After SM scans a file, update TRCK with real _UMID"""
    sm_data = get_sm_metadata(filepath)
    
    if sm_data:
        # Read current RIFF tags
        current_trck = read_trck(filepath)
        
        # If TRCK is a temp UUID4, replace with SM's _UMID
        if len(current_trck) > 32:  # UUID4 is longer
            write_trck(filepath, sm_data['_UMID'])
            write_isbj(filepath, str(sm_data['recid']))
```

## 🎯 Final Schema Recommendation

### TIER 1 - RIFF INFO (Header)
```
IART → Vendor (Artist)
IALB → Library (Album) [SM cannot see]
INAM → Name (Title)
IGNR → Category (Genre)
IKEY → ShortID (Keywords)
ICMT → Description (Comment)
ISBJ → SM recid [SM can see, but we'll document it]
TRCK → SM _UMID [SM cannot see] ← Direct link to SM database!
TYER → Year
```

### TIER 2 - ID3v2 (Header, for BPM/Key visibility)
```
TBPM → BPM [SM can see]
TKEY → Key [SM can see]
TXXX → Usage, SubCategory, Rating
```

### Why This Schema Works

1. **TRCK = SM's _UMID**
   - Links to SM database permanently
   - Invisible to SM (won't corrupt)
   - Can query: `WHERE _UMID = 'xxx'`

2. **ISBJ = SM's recid**
   - Integer primary key (fast queries)
   - Can query: `WHERE recid = 123`
   - SM can see it (visible as Subject field)
   - Users probably won't edit Subject field

3. **IALB = Library**
   - Invisible to SM
   - Safe from corruption

4. **TBPM/TKEY = BPM/Key**
   - Visible in SM during transition
   - Verifiable while working

## 📊 Next Steps

1. **Test _UMID behavior:**
   - Move a file
   - Rescan in SM
   - Check if _UMID stays same

2. **Test ISBJ field:**
   - Tag file with recid in ISBJ
   - Import to SM
   - See if it shows as "Subject"
   - See if SM corrupts it

3. **Build SM database query module:**
   - Read-only queries
   - Get UMID by filepath
   - Get metadata by UMID

4. **Implement tagging workflow:**
   - Check if file in SM database
   - Use SM's _UMID if available
   - Generate temp UUID if not
   - Update after SM scan

## ⚠️ Risks & Considerations

### Risk: _UMID Might Change

If _UMID is path-based, moving files could change it.
- **Test this!**
- If _UMID changes, we need to track files independently

### Risk: ISBJ Field Visible

SM shows ISBJ as "Subject" field.
- Users might edit it accidentally
- Could break recid linkage
- **Alternative:** Use a different invisible field (ISRC, ISHP, etc.)

### Risk: Database Access

SM database might be locked while SM is running.
- Use read-only mode
- Handle "database locked" errors
- Cache results to minimize queries

## 🎉 Summary

**SoundMiner already has a UUID system!**

Instead of inventing our own, we can:
1. Read SM's _UMID from database
2. Store in TRCK tag (invisible to SM)
3. Store recid in ISBJ tag (visible but rarely used)
4. Link files directly to SM database
5. Query by _UMID or recid as needed

This makes the transition much cleaner and provides permanent linkage to SM's tracking system!
