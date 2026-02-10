//! Theme system for the TUI.

use ratatui::style::{Color, Modifier, Style};

/// Color theme for the TUI.
#[derive(Debug, Clone)]
pub struct Theme {
    /// Theme name.
    pub name: &'static str,
    /// Border style.
    pub border: Style,
    /// Search input text style.
    pub search_text: Style,
    /// Selected result row style.
    pub results_selected: Style,
    /// Normal (unselected) result row style.
    pub results_normal: Style,
    /// Positive amplitude waveform color.
    pub waveform_positive: Color,
    /// Negative amplitude waveform color.
    pub waveform_negative: Color,
    /// Metadata key style (label).
    pub metadata_key: Style,
    /// Metadata value style.
    pub metadata_value: Style,
    /// Status text style (match count, searching...).
    pub status_text: Style,
    /// Match count number style.
    pub match_count: Style,
    /// Playback cursor background color (for waveform overlay).
    pub playback_cursor: Color,
    /// Playback accent style (for status bar playing indicator).
    pub playback_accent: Style,
    /// Table header row style.
    pub table_header: Style,
    /// Played (previously played back) row style.
    pub table_played: Style,
    /// Marked row style.
    pub table_marked: Style,
    /// Selected column header highlight style.
    pub table_column_highlight: Style,
}

impl Theme {
    /// Telescope theme (cyan/blue accent) — the default.
    pub fn telescope() -> Self {
        Self {
            name: "telescope",
            border: Style::default().fg(Color::Cyan),
            search_text: Style::default().fg(Color::White),
            results_selected: Style::default()
                .fg(Color::White)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
            results_normal: Style::default().fg(Color::Gray),
            waveform_positive: Color::Cyan,
            waveform_negative: Color::Blue,
            metadata_key: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            metadata_value: Style::default().fg(Color::White),
            status_text: Style::default().fg(Color::DarkGray),
            match_count: Style::default().fg(Color::Cyan),
            playback_cursor: Color::White,
            playback_accent: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            table_header: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            table_played: Style::default().fg(Color::Rgb(60, 60, 60)),
            table_marked: Style::default().fg(Color::Rgb(160, 160, 160)),
            table_column_highlight: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        }
    }

    /// Ableton theme (orange accent).
    pub fn ableton() -> Self {
        Self {
            name: "ableton",
            border: Style::default().fg(Color::Rgb(255, 153, 0)),
            search_text: Style::default().fg(Color::White),
            results_selected: Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(255, 153, 0))
                .add_modifier(Modifier::BOLD),
            results_normal: Style::default().fg(Color::Gray),
            waveform_positive: Color::Rgb(255, 153, 0),
            waveform_negative: Color::Rgb(204, 102, 0),
            metadata_key: Style::default()
                .fg(Color::Rgb(255, 153, 0))
                .add_modifier(Modifier::BOLD),
            metadata_value: Style::default().fg(Color::White),
            status_text: Style::default().fg(Color::DarkGray),
            match_count: Style::default().fg(Color::Rgb(255, 153, 0)),
            playback_cursor: Color::White,
            playback_accent: Style::default()
                .fg(Color::Rgb(255, 153, 0))
                .add_modifier(Modifier::BOLD),
            table_header: Style::default()
                .fg(Color::Rgb(255, 153, 0))
                .add_modifier(Modifier::BOLD),
            table_played: Style::default().fg(Color::Rgb(60, 60, 60)),
            table_marked: Style::default().fg(Color::Rgb(160, 160, 160)),
            table_column_highlight: Style::default()
                .fg(Color::Rgb(255, 153, 0))
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        }
    }

    /// SoundMiner theme (green accent).
    pub fn soundminer() -> Self {
        Self {
            name: "soundminer",
            border: Style::default().fg(Color::Green),
            search_text: Style::default().fg(Color::White),
            results_selected: Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
            results_normal: Style::default().fg(Color::Gray),
            waveform_positive: Color::Green,
            waveform_negative: Color::DarkGray,
            metadata_key: Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            metadata_value: Style::default().fg(Color::White),
            status_text: Style::default().fg(Color::DarkGray),
            match_count: Style::default().fg(Color::Green),
            playback_cursor: Color::White,
            playback_accent: Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            table_header: Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            table_played: Style::default().fg(Color::Rgb(60, 60, 60)),
            table_marked: Style::default().fg(Color::Rgb(160, 160, 160)),
            table_column_highlight: Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        }
    }

    /// Resolve a theme by name (case-insensitive).
    pub fn by_name(name: &str) -> anyhow::Result<Self> {
        match name.to_ascii_lowercase().as_str() {
            "telescope" => Ok(Self::telescope()),
            "ableton" => Ok(Self::ableton()),
            "soundminer" => Ok(Self::soundminer()),
            _ => anyhow::bail!(
                "unknown theme '{}' (available: telescope, ableton, soundminer)",
                name
            ),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::telescope()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_telescope_default() {
        let theme = Theme::default();
        assert_eq!(theme.name, "telescope");
    }

    #[test]
    fn test_theme_ableton_orange() {
        let theme = Theme::ableton();
        assert!(
            matches!(theme.waveform_positive, Color::Rgb(255, 153, 0)),
            "Ableton should have orange waveform"
        );
    }

    #[test]
    fn test_theme_soundminer_green() {
        let theme = Theme::soundminer();
        assert!(
            matches!(theme.waveform_positive, Color::Green),
            "SoundMiner should have green waveform"
        );
    }

    #[test]
    fn test_theme_by_name_case_insensitive() {
        assert!(Theme::by_name("Ableton").is_ok());
        assert!(Theme::by_name("ableton").is_ok());
        assert!(Theme::by_name("ABLETON").is_ok());
    }

    #[test]
    fn test_theme_by_name_invalid_errors() {
        assert!(Theme::by_name("nonexistent").is_err());
    }
}
