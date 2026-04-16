//! ID3v2 tag reading for WAV files via lofty.
//!
//! Used by the workflow engine to augment [`super::UnifiedMetadata`] with
//! fields stored in ID3v2 frames at the end of the file (TCON, TBPM, TKEY, etc.).

use std::path::Path;

use lofty::config::{ParseOptions, ParsingMode};
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey, Tag, TagType};

/// Parsed ID3v2 tag data from a WAV file.
#[derive(Debug, Clone, Default)]
pub struct Id3Tags {
    /// TPE1 — Artist / Vendor.
    pub vendor: String,
    /// TPE2 — Album Artist / Library (falls back to TALB if absent).
    pub library: String,
    /// TCON — Content type / Category.
    pub category: String,
    /// TIT2 — Title / Sound ID.
    pub sound_id: String,
    /// TIT3 — Subtitle / Usage ID (mapped from TrackSubtitle).
    pub usage_id: String,
    /// COMM — Comment / Description.
    pub description: String,
    /// TBPM — Beats per minute (tries TBPM then IntegerBpm).
    pub bpm: Option<u16>,
    /// TKEY — Musical key.
    pub key: String,
    /// TDRC — Recording date.
    pub date: String,
    /// TCOM — Composer / Subcategory.
    pub subcategory: String,
    /// TIT1 — Content group / Genre ID.
    pub genre_id: String,
    /// TRCK — Track number.
    pub track: String,
}

/// Read ID3v2 tags from a WAV file using lofty.
///
/// Returns `Ok(tags)` with whatever fields are present, or an error if the
/// file cannot be read or has no ID3v2 tag.
pub fn read_id3_tags(path: &Path) -> anyhow::Result<Id3Tags> {
    let opts = ParseOptions::new()
        .parsing_mode(ParsingMode::Relaxed)
        .read_cover_art(false);
    let tagged_file = Probe::open(path)?.options(opts).read()?;

    let tag = tagged_file
        .tag(TagType::Id3v2)
        .ok_or_else(|| anyhow::anyhow!("no ID3v2 tag"))?;

    Ok(parse_tag(tag))
}

fn parse_tag(tag: &Tag) -> Id3Tags {
    let get = |key: &ItemKey| -> String {
        tag.get_string(key).unwrap_or_default().trim().to_string()
    };

    let bpm = tag
        .get_string(&ItemKey::Bpm)
        .or_else(|| tag.get_string(&ItemKey::IntegerBpm))
        .and_then(|s| s.trim().parse::<u16>().ok());

    let library = {
        let tpe2 = get(&ItemKey::AlbumArtist);
        if tpe2.is_empty() {
            get(&ItemKey::AlbumTitle)
        } else {
            tpe2
        }
    };

    Id3Tags {
        vendor: tag.artist().unwrap_or_default().trim().to_string(),
        library,
        category: tag.genre().unwrap_or_default().trim().to_string(),
        sound_id: tag.title().unwrap_or_default().trim().to_string(),
        usage_id: get(&ItemKey::TrackSubtitle),
        description: tag.comment().unwrap_or_default().trim().to_string(),
        bpm,
        key: get(&ItemKey::InitialKey),
        date: get(&ItemKey::RecordingDate),
        subcategory: get(&ItemKey::Composer),
        genre_id: get(&ItemKey::ContentGroup),
        track: get(&ItemKey::TrackNumber),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::UnifiedMetadata;

    fn test_files_exist() -> bool {
        std::path::Path::new("test_files").exists()
    }

    #[test]
    fn read_id3_tags_from_id3_all() {
        if !test_files_exist() {
            return;
        }
        let tags = read_id3_tags(Path::new("test_files/id3-all.wav")).unwrap();
        // File has ID3v2 tags — at minimum some fields should be non-empty.
        assert!(
            !tags.vendor.is_empty()
                || !tags.category.is_empty()
                || !tags.sound_id.is_empty()
                || !tags.description.is_empty(),
            "expected at least one non-empty ID3 field, got: {tags:?}"
        );
    }

    #[test]
    fn read_id3_tags_from_id3_only() {
        if !test_files_exist() {
            return;
        }
        let tags = read_id3_tags(Path::new("test_files/id3-only.wav")).unwrap();
        assert!(
            !tags.vendor.is_empty()
                || !tags.category.is_empty()
                || !tags.sound_id.is_empty(),
            "id3-only.wav should have at least one ID3 field: {tags:?}"
        );
    }

    #[test]
    fn read_id3_no_tag_returns_error() {
        if !test_files_exist() {
            return;
        }
        // clean_base.wav has BEXT but no ID3v2 tag.
        let result = read_id3_tags(Path::new("test_files/clean_base.wav"));
        assert!(result.is_err(), "clean_base.wav should have no ID3v2 tag");
    }

    #[test]
    fn read_id3_nonexistent_file() {
        let result = read_id3_tags(Path::new("/nonexistent/path.wav"));
        assert!(result.is_err());
    }

    #[test]
    fn merge_fills_empty_fields() {
        let mut meta = UnifiedMetadata::default();
        let id3 = Id3Tags {
            vendor: "TestVendor".to_string(),
            category: "DRUMS".to_string(),
            bpm: Some(128),
            key: "Cmin".to_string(),
            ..Default::default()
        };
        merge_id3_into_unified(&mut meta, &id3);
        assert_eq!(meta.vendor, "TestVendor");
        assert_eq!(meta.category, "DRUMS");
        assert_eq!(meta.bpm, Some(128));
        assert_eq!(meta.key, "Cmin");
    }

    #[test]
    fn merge_does_not_overwrite_existing() {
        let mut meta = UnifiedMetadata {
            vendor: "OriginalVendor".to_string(),
            category: "SFX".to_string(),
            bpm: Some(100),
            ..Default::default()
        };
        let id3 = Id3Tags {
            vendor: "ID3Vendor".to_string(),
            category: "DRUMS".to_string(),
            bpm: Some(128),
            key: "Dmin".to_string(),
            ..Default::default()
        };
        merge_id3_into_unified(&mut meta, &id3);
        // Existing fields should not be overwritten.
        assert_eq!(meta.vendor, "OriginalVendor");
        assert_eq!(meta.category, "SFX");
        assert_eq!(meta.bpm, Some(100));
        // Empty field should be filled.
        assert_eq!(meta.key, "Dmin");
    }

    #[test]
    fn merge_empty_id3_is_noop() {
        let mut meta = UnifiedMetadata {
            vendor: "V".to_string(),
            library: "L".to_string(),
            ..Default::default()
        };
        let original = meta.clone();
        let id3 = Id3Tags::default();
        merge_id3_into_unified(&mut meta, &id3);
        assert_eq!(meta.vendor, original.vendor);
        assert_eq!(meta.library, original.library);
    }

    #[test]
    fn merge_fills_new_fields() {
        let mut meta = UnifiedMetadata::default();
        let id3 = Id3Tags {
            date: "2026-01-15".to_string(),
            subcategory: "PERC".to_string(),
            genre_id: "GRP1".to_string(),
            track: "3".to_string(),
            ..Default::default()
        };
        merge_id3_into_unified(&mut meta, &id3);
        assert_eq!(meta.date, "2026-01-15");
        assert_eq!(meta.subcategory, "PERC");
        assert_eq!(meta.genre_id, "GRP1");
        assert_eq!(meta.track, "3");
    }

    #[test]
    fn integration_all_id3_files_parseable() {
        if !test_files_exist() {
            return;
        }
        let id3_files = [
            "test_files/id3.wav",
            "test_files/id3-2.wav",
            "test_files/id3-all.wav",
            "test_files/id3-all_r7.wav",
            "test_files/id3-all_sm.wav",
            "test_files/id3-only.wav",
        ];
        for path in &id3_files {
            let result = read_id3_tags(Path::new(path));
            assert!(result.is_ok(), "failed to read ID3 from {path}: {}", result.unwrap_err());
        }
    }

    use proptest::prelude::*;

    fn arb_id3_tags() -> impl Strategy<Value = Id3Tags> {
        (
            any::<String>(),
            any::<String>(),
            any::<String>(),
            any::<String>(),
            any::<String>(),
            any::<String>(),
            proptest::option::of(any::<u16>()),
            any::<String>(),
            any::<String>(),
            any::<String>(),
            any::<String>(),
            any::<String>(),
        )
            .prop_map(
                |(vendor, library, category, sound_id, usage_id, description, bpm, key, date, subcategory, genre_id, track)| {
                    Id3Tags { vendor, library, category, sound_id, usage_id, description, bpm, key, date, subcategory, genre_id, track }
                },
            )
    }

    proptest! {
        /// Merge never overwrites a non-empty UnifiedMetadata field.
        #[test]
        fn proptest_merge_no_overwrite(
            id3 in arb_id3_tags(),
            existing_vendor in "[a-z]{1,8}",
            existing_library in "[a-z]{1,8}",
            existing_bpm in 1u16..300,
        ) {
            let mut meta = UnifiedMetadata {
                vendor: existing_vendor.clone(),
                library: existing_library.clone(),
                bpm: Some(existing_bpm),
                ..Default::default()
            };
            merge_id3_into_unified(&mut meta, &id3);
            prop_assert_eq!(&meta.vendor, &existing_vendor, "vendor was overwritten");
            prop_assert_eq!(&meta.library, &existing_library, "library was overwritten");
            prop_assert_eq!(meta.bpm, Some(existing_bpm), "bpm was overwritten");
        }

        /// Merge fills empty fields from Id3Tags.
        #[test]
        fn proptest_merge_fills_empty(id3 in arb_id3_tags()) {
            let mut meta = UnifiedMetadata::default();
            merge_id3_into_unified(&mut meta, &id3);
            prop_assert_eq!(&meta.vendor, &id3.vendor);
            prop_assert_eq!(&meta.library, &id3.library);
            prop_assert_eq!(&meta.category, &id3.category);
            prop_assert_eq!(&meta.sound_id, &id3.sound_id);
            prop_assert_eq!(&meta.usage_id, &id3.usage_id);
            prop_assert_eq!(&meta.description, &id3.description);
            prop_assert_eq!(meta.bpm, id3.bpm);
            prop_assert_eq!(&meta.key, &id3.key);
            prop_assert_eq!(&meta.date, &id3.date);
            prop_assert_eq!(&meta.subcategory, &id3.subcategory);
            prop_assert_eq!(&meta.genre_id, &id3.genre_id);
            prop_assert_eq!(&meta.track, &id3.track);
        }
    }
}

/// Merge ID3v2 tag data into a [`super::UnifiedMetadata`], filling only
/// fields that are currently empty.
pub fn merge_id3_into_unified(meta: &mut super::UnifiedMetadata, id3: &Id3Tags) {
    if meta.vendor.is_empty() && !id3.vendor.is_empty() {
        meta.vendor.clone_from(&id3.vendor);
    }
    if meta.library.is_empty() && !id3.library.is_empty() {
        meta.library.clone_from(&id3.library);
    }
    if meta.category.is_empty() && !id3.category.is_empty() {
        meta.category.clone_from(&id3.category);
    }
    if meta.sound_id.is_empty() && !id3.sound_id.is_empty() {
        meta.sound_id.clone_from(&id3.sound_id);
    }
    if meta.usage_id.is_empty() && !id3.usage_id.is_empty() {
        meta.usage_id.clone_from(&id3.usage_id);
    }
    if meta.description.is_empty() && !id3.description.is_empty() {
        meta.description.clone_from(&id3.description);
    }
    if meta.bpm.is_none() && id3.bpm.is_some() {
        meta.bpm = id3.bpm;
    }
    if meta.key.is_empty() && !id3.key.is_empty() {
        meta.key.clone_from(&id3.key);
    }
    if meta.date.is_empty() && !id3.date.is_empty() {
        meta.date.clone_from(&id3.date);
    }
    if meta.genre_id.is_empty() && !id3.genre_id.is_empty() {
        meta.genre_id.clone_from(&id3.genre_id);
    }
    if meta.subcategory.is_empty() && !id3.subcategory.is_empty() {
        meta.subcategory.clone_from(&id3.subcategory);
    }
    if meta.track.is_empty() && !id3.track.is_empty() {
        meta.track.clone_from(&id3.track);
    }
}
