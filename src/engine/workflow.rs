//! Lua workflow engine for scripted metadata transformations.
//!
//! Runs Lua 5.4 scripts (via mlua) against each matched WAV file's metadata.
//! Scripts interact with a `sample` userdata object that exposes getters and
//! setters. Changes are displayed as a colorized diff and optionally written
//! back to the file's BEXT chunk with `--commit`.

use std::fmt;
use std::path::Path;

use mlua::prelude::*;

use super::bext;
use super::UnifiedMetadata;

/// Convert mlua::Error (not Send+Sync) to anyhow via Display.
fn lua_err(e: mlua::Error) -> anyhow::Error {
    anyhow::anyhow!("{e}")
}

/// A loaded Lua script ready to execute against metadata.
#[derive(Debug, Clone)]
pub struct WorkflowScript {
    source: String,
}

impl Default for WorkflowScript {
    fn default() -> Self {
        Self {
            source: String::new(),
        }
    }
}

/// Load a workflow script from either an eval string or a file path.
///
/// Returns `Ok(Some(script))` if a source was provided, `Ok(None)` if neither
/// `eval` nor `workflow` was given.
pub fn load_workflow_script(
    eval: Option<&str>,
    workflow: Option<&Path>,
) -> anyhow::Result<Option<WorkflowScript>> {
    match (eval, workflow) {
        (Some(code), _) => Ok(Some(WorkflowScript {
            source: code.to_string(),
        })),
        (_, Some(path)) => {
            let source = std::fs::read_to_string(path)?;
            Ok(Some(WorkflowScript { source }))
        }
        (None, None) => Ok(None),
    }
}

/// Lua-side mutable wrapper around [`UnifiedMetadata`].
struct SampleUserData {
    meta: UnifiedMetadata,
}

impl LuaUserData for SampleUserData {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        // --- Getters ---
        methods.add_method("path", |_, this, ()| {
            Ok(this.meta.path.display().to_string())
        });
        methods.add_method("basename", |_, this, ()| {
            Ok(this
                .meta
                .path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default())
        });
        methods.add_method("dirname", |_, this, ()| {
            Ok(this
                .meta
                .path
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_default())
        });
        methods.add_method("vendor", |_, this, ()| Ok(this.meta.vendor.clone()));
        methods.add_method("library", |_, this, ()| Ok(this.meta.library.clone()));
        methods.add_method("category", |_, this, ()| Ok(this.meta.category.clone()));
        methods.add_method("sound_id", |_, this, ()| Ok(this.meta.sound_id.clone()));
        methods.add_method("description", |_, this, ()| {
            Ok(this.meta.description.clone())
        });
        methods.add_method("comment", |_, this, ()| Ok(this.meta.comment.clone()));
        methods.add_method("bpm", |_, this, ()| Ok(this.meta.bpm));
        methods.add_method("key", |_, this, ()| Ok(this.meta.key.clone()));
        methods.add_method("rating", |_, this, ()| Ok(this.meta.rating.clone()));
        methods.add_method("subcategory", |_, this, ()| {
            Ok(this.meta.subcategory.clone())
        });
        methods.add_method("genre_id", |_, this, ()| Ok(this.meta.genre_id.clone()));
        methods.add_method("usage_id", |_, this, ()| Ok(this.meta.usage_id.clone()));
        methods.add_method("take", |_, this, ()| Ok(this.meta.take.clone()));
        methods.add_method("track", |_, this, ()| Ok(this.meta.track.clone()));
        methods.add_method("item", |_, this, ()| Ok(this.meta.item.clone()));
        methods.add_method("date", |_, this, ()| Ok(this.meta.date.clone()));
        methods.add_method("bext_umid", |_, this, ()| Ok(this.meta.umid.clone()));
        methods.add_method("file_id", |_, this, ()| Ok(this.meta.file_id));
        methods.add_method("is_packed", |_, this, ()| Ok(this.meta.file_id != 0));

        // --- Setters ---
        methods.add_method_mut("set_vendor", |_, this, val: String| {
            this.meta.vendor = val;
            Ok(())
        });
        methods.add_method_mut("set_library", |_, this, val: String| {
            this.meta.library = val;
            Ok(())
        });
        methods.add_method_mut("set_category", |_, this, val: String| {
            this.meta.category = val;
            Ok(())
        });
        methods.add_method_mut("set_sound_id", |_, this, val: String| {
            this.meta.sound_id = val;
            Ok(())
        });
        methods.add_method_mut("set_description", |_, this, val: String| {
            this.meta.description = val;
            Ok(())
        });
        methods.add_method_mut("set_comment", |_, this, val: String| {
            this.meta.comment = val;
            Ok(())
        });
        methods.add_method_mut("set_bpm", |_, this, val: Option<u16>| {
            this.meta.bpm = val;
            Ok(())
        });
        methods.add_method_mut("set_key", |_, this, val: String| {
            this.meta.key = val;
            Ok(())
        });
        methods.add_method_mut("set_rating", |_, this, val: String| {
            this.meta.rating = val;
            Ok(())
        });
        methods.add_method_mut("set_subcategory", |_, this, val: String| {
            this.meta.subcategory = val;
            Ok(())
        });
        methods.add_method_mut("set_genre_id", |_, this, val: String| {
            this.meta.genre_id = val;
            Ok(())
        });
        methods.add_method_mut("set_usage_id", |_, this, val: String| {
            this.meta.usage_id = val;
            Ok(())
        });
    }
}

/// Run a Lua script against a single file's metadata, returning the
/// (possibly modified) metadata.
pub fn run_lua_script(
    script: &WorkflowScript,
    meta: UnifiedMetadata,
    force: bool,
    commit: bool,
) -> anyhow::Result<UnifiedMetadata> {
    if script.source.is_empty() {
        return Ok(meta);
    }

    let lua = Lua::new();

    // Expose `riffgrep` global table with flags.
    let rfg_table = lua.create_table().map_err(lua_err)?;
    rfg_table.set("force", force).map_err(lua_err)?;
    rfg_table.set("commit", commit).map_err(lua_err)?;
    lua.globals().set("riffgrep", rfg_table).map_err(lua_err)?;

    // Expose `sample` userdata.
    let ud = lua.create_userdata(SampleUserData { meta }).map_err(lua_err)?;
    lua.globals().set("sample", ud.clone()).map_err(lua_err)?;

    lua.load(&script.source).exec().map_err(lua_err)?;

    let result = ud.borrow::<SampleUserData>().map_err(lua_err)?;
    Ok(result.meta.clone())
}

/// A field-level diff between two [`UnifiedMetadata`] instances.
#[derive(Debug, Clone)]
pub struct MetaDiff {
    changes: Vec<FieldChange>,
}

#[derive(Debug, Clone)]
struct FieldChange {
    field: &'static str,
    old: String,
    new: String,
}

impl MetaDiff {
    /// Returns `true` if no fields changed.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

impl fmt::Display for MetaDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for c in &self.changes {
            writeln!(f, "  \x1b[31m- {}: {}\x1b[0m", c.field, c.old)?;
            writeln!(f, "  \x1b[32m+ {}: {}\x1b[0m", c.field, c.new)?;
        }
        Ok(())
    }
}

/// Compare two metadata snapshots and return field-level changes.
pub fn compute_meta_diff(before: &UnifiedMetadata, after: &UnifiedMetadata) -> MetaDiff {
    let mut changes = Vec::new();

    macro_rules! cmp_field {
        ($field:ident, $label:expr) => {
            if before.$field != after.$field {
                changes.push(FieldChange {
                    field: $label,
                    old: format!("{}", before.$field),
                    new: format!("{}", after.$field),
                });
            }
        };
    }

    cmp_field!(vendor, "vendor");
    cmp_field!(library, "library");
    cmp_field!(description, "description");
    cmp_field!(comment, "comment");
    cmp_field!(category, "category");
    cmp_field!(sound_id, "sound_id");
    cmp_field!(usage_id, "usage_id");
    cmp_field!(subcategory, "subcategory");
    cmp_field!(genre_id, "genre_id");
    cmp_field!(key, "key");
    cmp_field!(rating, "rating");
    cmp_field!(take, "take");
    cmp_field!(track, "track");
    cmp_field!(item, "item");

    // BPM needs special handling (Option<u16>).
    if before.bpm != after.bpm {
        changes.push(FieldChange {
            field: "bpm",
            old: before
                .bpm
                .map(|v| v.to_string())
                .unwrap_or_else(|| "—".to_string()),
            new: after
                .bpm
                .map(|v| v.to_string())
                .unwrap_or_else(|| "—".to_string()),
        });
    }

    MetaDiff { changes }
}

/// Format a diff for display.
pub fn format_meta_diff(diff: &MetaDiff) -> String {
    diff.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_meta() -> UnifiedMetadata {
        UnifiedMetadata {
            path: PathBuf::from("/test/kick.wav"),
            vendor: "Mars".to_string(),
            library: "DX100".to_string(),
            category: "DRUMS".to_string(),
            sound_id: "KCK".to_string(),
            description: "808 kick".to_string(),
            bpm: Some(120),
            key: "Cmin".to_string(),
            ..Default::default()
        }
    }

    // --- load_workflow_script ---

    #[test]
    fn load_from_eval() {
        let script = load_workflow_script(Some("sample:set_category('SFX')"), None)
            .unwrap()
            .unwrap();
        assert_eq!(script.source, "sample:set_category('SFX')");
    }

    #[test]
    fn load_from_file() {
        let dir = std::env::temp_dir().join("riffgrep_wf_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.lua");
        std::fs::write(&path, "sample:set_vendor('V')").unwrap();
        let script = load_workflow_script(None, Some(&path)).unwrap().unwrap();
        assert_eq!(script.source, "sample:set_vendor('V')");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_none_returns_none() {
        let result = load_workflow_script(None, None).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn load_nonexistent_file_errors() {
        let result = load_workflow_script(None, Some(Path::new("/nonexistent.lua")));
        assert!(result.is_err());
    }

    // --- run_lua_script ---

    #[test]
    fn lua_set_category() {
        let script = WorkflowScript {
            source: "sample:set_category('SFX')".to_string(),
        };
        let result = run_lua_script(&script, sample_meta(), false, false).unwrap();
        assert_eq!(result.category, "SFX");
        // Other fields unchanged.
        assert_eq!(result.vendor, "Mars");
    }

    #[test]
    fn lua_set_multiple_fields() {
        let script = WorkflowScript {
            source: r#"
                sample:set_vendor("Splice")
                sample:set_bpm(140)
                sample:set_key("Dmin")
            "#
            .to_string(),
        };
        let result = run_lua_script(&script, sample_meta(), false, false).unwrap();
        assert_eq!(result.vendor, "Splice");
        assert_eq!(result.bpm, Some(140));
        assert_eq!(result.key, "Dmin");
    }

    #[test]
    fn lua_read_getters() {
        let script = WorkflowScript {
            source: r#"
                assert(sample:vendor() == "Mars")
                assert(sample:category() == "DRUMS")
                assert(sample:bpm() == 120)
                assert(sample:basename() == "kick.wav")
            "#
            .to_string(),
        };
        let result = run_lua_script(&script, sample_meta(), false, false);
        assert!(result.is_ok(), "Lua assertions failed: {}", result.unwrap_err());
    }

    #[test]
    fn lua_riffgrep_globals() {
        let script = WorkflowScript {
            source: r#"
                assert(riffgrep.force == true)
                assert(riffgrep.commit == false)
            "#
            .to_string(),
        };
        let result = run_lua_script(&script, sample_meta(), true, false);
        assert!(result.is_ok(), "riffgrep globals check failed: {}", result.unwrap_err());
    }

    #[test]
    fn lua_syntax_error_returns_err() {
        let script = WorkflowScript {
            source: "this is not valid lua +++".to_string(),
        };
        let result = run_lua_script(&script, sample_meta(), false, false);
        assert!(result.is_err());
    }

    #[test]
    fn lua_runtime_error_returns_err() {
        let script = WorkflowScript {
            source: "error('intentional failure')".to_string(),
        };
        let result = run_lua_script(&script, sample_meta(), false, false);
        assert!(result.is_err());
    }

    #[test]
    fn lua_empty_script_is_noop() {
        let script = WorkflowScript::default();
        let meta = sample_meta();
        let result = run_lua_script(&script, meta.clone(), false, false).unwrap();
        assert_eq!(result.vendor, meta.vendor);
        assert_eq!(result.category, meta.category);
    }

    #[test]
    fn lua_is_packed_reflects_file_id() {
        let script = WorkflowScript {
            source: "assert(sample:is_packed() == false)".to_string(),
        };
        let result = run_lua_script(&script, sample_meta(), false, false);
        assert!(result.is_ok());

        let mut packed_meta = sample_meta();
        packed_meta.file_id = 12345;
        let script = WorkflowScript {
            source: "assert(sample:is_packed() == true)".to_string(),
        };
        let result = run_lua_script(&script, packed_meta, false, false);
        assert!(result.is_ok());
    }

    // --- compute_meta_diff ---

    #[test]
    fn diff_identical_is_empty() {
        let a = sample_meta();
        let b = a.clone();
        let diff = compute_meta_diff(&a, &b);
        assert!(diff.is_empty());
    }

    #[test]
    fn diff_detects_string_change() {
        let a = sample_meta();
        let mut b = a.clone();
        b.category = "SFX".to_string();
        let diff = compute_meta_diff(&a, &b);
        assert!(!diff.is_empty());
        assert_eq!(diff.changes.len(), 1);
        assert_eq!(diff.changes[0].field, "category");
        assert_eq!(diff.changes[0].old, "DRUMS");
        assert_eq!(diff.changes[0].new, "SFX");
    }

    #[test]
    fn diff_detects_bpm_change() {
        let a = sample_meta();
        let mut b = a.clone();
        b.bpm = Some(140);
        let diff = compute_meta_diff(&a, &b);
        assert!(!diff.is_empty());
        assert_eq!(diff.changes[0].field, "bpm");
    }

    #[test]
    fn diff_detects_multiple_changes() {
        let a = sample_meta();
        let mut b = a.clone();
        b.vendor = "Splice".to_string();
        b.key = "Dmin".to_string();
        b.bpm = None;
        let diff = compute_meta_diff(&a, &b);
        assert_eq!(diff.changes.len(), 3);
    }

    #[test]
    fn format_diff_contains_field_names() {
        let a = sample_meta();
        let mut b = a.clone();
        b.category = "SFX".to_string();
        let diff = compute_meta_diff(&a, &b);
        let formatted = format_meta_diff(&diff);
        assert!(formatted.contains("category"));
        assert!(formatted.contains("DRUMS"));
        assert!(formatted.contains("SFX"));
    }
}

/// Write metadata changes back to the file's BEXT chunk.
///
/// Performs surgical overwrites at fixed BEXT offsets — no re-encoding of
/// audio data is needed. Only writes fields that actually changed between
/// `before` and `after`.
pub fn write_metadata_changes(
    path: &Path,
    before: &UnifiedMetadata,
    after: &UnifiedMetadata,
    force: bool,
) -> anyhow::Result<()> {
    // Guard: skip files that already have a file_id unless --force.
    if !force && before.file_id != 0 {
        anyhow::bail!("file already packed (use --force to overwrite)");
    }

    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::with_capacity(4096, file);
    let map = bext::scan_chunks(&mut reader)?;
    drop(reader);

    // Helper: write a fixed-width ASCII field, right-padded with zeros.
    let write_ascii = |offset: usize, len: usize, val: &str| -> anyhow::Result<()> {
        let mut buf = vec![0u8; len];
        let bytes = val.as_bytes();
        let copy_len = bytes.len().min(len);
        buf[..copy_len].copy_from_slice(&bytes[..copy_len]);
        bext::write_bext_field(path, &map, offset, &buf)?;
        Ok(())
    };

    // Standard BEXT fields (outside packed Description).
    if before.vendor != after.vendor {
        write_ascii(256, 32, &after.vendor)?;
    }
    if before.library != after.library {
        write_ascii(288, 32, &after.library)?;
    }

    // Packed Description fields (offsets within the 256-byte Description block).
    if before.comment != after.comment {
        write_ascii(44, 32, &after.comment)?;
    }
    if before.rating != after.rating {
        write_ascii(76, 4, &after.rating)?;
    }
    if before.bpm != after.bpm {
        let bpm_str = after.bpm.map(|v| format!("{v:>4}")).unwrap_or_default();
        write_ascii(80, 4, &bpm_str)?;
    }
    if before.subcategory != after.subcategory {
        write_ascii(84, 4, &after.subcategory)?;
    }
    if before.category != after.category {
        write_ascii(88, 4, &after.category)?;
    }
    if before.genre_id != after.genre_id {
        write_ascii(92, 4, &after.genre_id)?;
    }
    if before.sound_id != after.sound_id {
        write_ascii(96, 4, &after.sound_id)?;
    }
    if before.usage_id != after.usage_id {
        write_ascii(100, 4, &after.usage_id)?;
    }
    if before.key != after.key {
        write_ascii(104, 8, &after.key)?;
    }

    Ok(())
}
