//! Action enum for decoupling keybindings from behavior.

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Every user-triggerable action in the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    // Navigation
    /// Move down one row.
    MoveDown,
    /// Move up one row.
    MoveUp,
    /// Jump to the first row.
    MoveToTop,
    /// Jump to the last row.
    MoveToBottom,
    /// Page down.
    PageDown,
    /// Page up.
    PageUp,
    /// Move column selection left.
    MoveColumnLeft,
    /// Move column selection right.
    MoveColumnRight,

    // Sort
    /// Sort the selected column in ascending order.
    SortAscending,
    /// Sort the selected column in descending order.
    SortDescending,
    /// Shuffle results randomly.
    RandomSort,
    /// Sort by embedding similarity to the selected file.
    SortBySimilarity,

    // Playback
    /// Toggle audio playback.
    TogglePlayback,
    /// Seek forward by a small increment.
    SeekForwardSmall,
    /// Seek forward by a large increment.
    SeekForwardLarge,
    /// Seek backward by a small increment.
    SeekBackwardSmall,
    /// Seek backward by a large increment.
    SeekBackwardLarge,
    /// Rewind to the start of the file.
    RewindToStart,
    /// Toggle auto-advance to the next file.
    ToggleAutoAdvance,
    /// Toggle elapsed/remaining time display.
    ToggleTimeDisplay,
    /// Toggle global loop mode.
    ToggleGlobalLoop,
    /// Reverse playback direction.
    ReversePlayback,
    /// Increase volume by 1 dBFS.
    VolumeUp,
    /// Decrease volume by 1 dBFS.
    VolumeDown,
    /// Increase speed by 100 cents (coarse).
    SpeedIncCents,
    /// Decrease speed by 100 cents (coarse).
    SpeedDecCents,
    /// Increase speed by 1 cent (fine).
    SpeedIncCentsFine,
    /// Decrease speed by 1 cent (fine).
    SpeedDecCentsFine,
    /// Increase speed by 1 BPM (linear).
    SpeedIncBpm,
    /// Decrease speed by 1 BPM (linear).
    SpeedDecBpm,
    /// Increase speed by 0.1 BPM (fine).
    SpeedIncBpmFine,
    /// Decrease speed by 0.1 BPM (fine).
    SpeedDecBpmFine,
    /// Reset playback speed to 1x.
    SpeedReset,
    /// Increase session BPM by 1.
    SessionBpmInc,
    /// Decrease session BPM by 1.
    SessionBpmDec,
    /// Increase session BPM by 0.1.
    SessionBpmIncFine,
    /// Decrease session BPM by 0.1.
    SessionBpmDecFine,

    // Marks
    /// Toggle mark on the current row.
    ToggleMark,
    /// Clear all marks.
    ClearMarks,
    /// Filter results to marked rows only.
    ToggleMarkedFilter,
    /// Save markers to file.
    SaveMarkers,

    // Markers
    /// Toggle marker bank A/B.
    ToggleBank,
    /// Toggle bank sync mode.
    ToggleBankSync,
    /// Set marker 1 at the playback cursor.
    SetMarker1,
    /// Set marker 2 at the playback cursor.
    SetMarker2,
    /// Set marker 3 at the playback cursor.
    SetMarker3,
    /// Clear the nearest marker.
    ClearNearestMarker,
    /// Clear all markers in the current bank.
    ClearBankMarkers,
    /// Increment segment repeat count.
    IncrementRep,
    /// Decrement segment repeat count.
    DecrementRep,
    /// Select the next marker.
    SelectNextMarker,
    /// Select the previous marker.
    SelectPrevMarker,
    /// Toggle infinite loop on the current segment.
    ToggleInfiniteLoop,
    /// Toggle preview loop.
    TogglePreviewLoop,
    /// Nudge marker forward by a small amount.
    NudgeMarkerForwardSmall,
    /// Nudge marker backward by a small amount.
    NudgeMarkerBackwardSmall,
    /// Nudge marker forward by a large amount.
    NudgeMarkerForwardLarge,
    /// Nudge marker backward by a large amount.
    NudgeMarkerBackwardLarge,
    /// Snap marker to the next zero-crossing.
    SnapZeroCrossingForward,
    /// Snap marker to the previous zero-crossing.
    SnapZeroCrossingBackward,
    /// Reset markers to preset positions.
    MarkerReset,
    /// Export markers to a CSV file.
    ExportMarkersCsv,
    /// Import markers from a CSV file.
    ImportMarkersCsv,
    /// Toggle marker line display on the waveform.
    ToggleMarkerDisplay,

    // Waveform
    /// Zoom into the waveform.
    ZoomIn,
    /// Zoom out of the waveform.
    ZoomOut,
    /// Reset waveform zoom to default.
    ZoomReset,

    // Mode
    /// Enter search (insert) mode.
    EnterInsertMode,
    /// Exit search mode and return to normal mode.
    EnterNormalMode,
    /// Submit the current search query.
    SearchSubmit,
    /// Clear the search query.
    ClearQuery,

    // Selection
    /// Open the currently selected file.
    OpenSelected,

    // Help
    /// Toggle the help overlay.
    ShowHelp,

    // App
    /// Quit the application.
    Quit,
}

impl Action {
    /// All action variants (for exhaustive testing).
    #[cfg(test)]
    pub const ALL: &'static [Action] = &[
        Action::MoveDown,
        Action::MoveUp,
        Action::MoveToTop,
        Action::MoveToBottom,
        Action::PageDown,
        Action::PageUp,
        Action::MoveColumnLeft,
        Action::MoveColumnRight,
        Action::SortAscending,
        Action::SortDescending,
        Action::RandomSort,
        Action::SortBySimilarity,
        Action::TogglePlayback,
        Action::SeekForwardSmall,
        Action::SeekForwardLarge,
        Action::SeekBackwardSmall,
        Action::SeekBackwardLarge,
        Action::RewindToStart,
        Action::ToggleAutoAdvance,
        Action::ToggleTimeDisplay,
        Action::ToggleGlobalLoop,
        Action::ReversePlayback,
        Action::VolumeUp,
        Action::VolumeDown,
        Action::SpeedIncCents,
        Action::SpeedDecCents,
        Action::SpeedIncCentsFine,
        Action::SpeedDecCentsFine,
        Action::SpeedIncBpm,
        Action::SpeedDecBpm,
        Action::SpeedIncBpmFine,
        Action::SpeedDecBpmFine,
        Action::SpeedReset,
        Action::SessionBpmInc,
        Action::SessionBpmDec,
        Action::SessionBpmIncFine,
        Action::SessionBpmDecFine,
        Action::ToggleMark,
        Action::ClearMarks,
        Action::ToggleMarkedFilter,
        Action::SaveMarkers,
        Action::ToggleBank,
        Action::ToggleBankSync,
        Action::SetMarker1,
        Action::SetMarker2,
        Action::SetMarker3,
        Action::ClearNearestMarker,
        Action::ClearBankMarkers,
        Action::IncrementRep,
        Action::DecrementRep,
        Action::SelectNextMarker,
        Action::SelectPrevMarker,
        Action::ToggleInfiniteLoop,
        Action::TogglePreviewLoop,
        Action::NudgeMarkerForwardSmall,
        Action::NudgeMarkerBackwardSmall,
        Action::NudgeMarkerForwardLarge,
        Action::NudgeMarkerBackwardLarge,
        Action::SnapZeroCrossingForward,
        Action::SnapZeroCrossingBackward,
        Action::MarkerReset,
        Action::ExportMarkersCsv,
        Action::ImportMarkersCsv,
        Action::ToggleMarkerDisplay,
        Action::ZoomIn,
        Action::ZoomOut,
        Action::ZoomReset,
        Action::EnterInsertMode,
        Action::EnterNormalMode,
        Action::SearchSubmit,
        Action::ClearQuery,
        Action::OpenSelected,
        Action::ShowHelp,
        Action::Quit,
    ];

    /// Parse an action from its canonical string name (for keymap config).
    pub fn from_name(name: &str) -> Option<Action> {
        match name {
            "move_down" => Some(Action::MoveDown),
            "move_up" => Some(Action::MoveUp),
            "move_to_top" => Some(Action::MoveToTop),
            "move_to_bottom" => Some(Action::MoveToBottom),
            "page_down" => Some(Action::PageDown),
            "page_up" => Some(Action::PageUp),
            "move_column_left" => Some(Action::MoveColumnLeft),
            "move_column_right" => Some(Action::MoveColumnRight),
            "sort_ascending" => Some(Action::SortAscending),
            "sort_descending" => Some(Action::SortDescending),
            "random_sort" => Some(Action::RandomSort),
            "sort_by_similarity" => Some(Action::SortBySimilarity),
            "toggle_playback" => Some(Action::TogglePlayback),
            "seek_forward_small" => Some(Action::SeekForwardSmall),
            "seek_forward_large" => Some(Action::SeekForwardLarge),
            "seek_backward_small" => Some(Action::SeekBackwardSmall),
            "seek_backward_large" => Some(Action::SeekBackwardLarge),
            "rewind_to_start" => Some(Action::RewindToStart),
            "toggle_auto_advance" => Some(Action::ToggleAutoAdvance),
            "toggle_time_display" => Some(Action::ToggleTimeDisplay),
            "toggle_global_loop" => Some(Action::ToggleGlobalLoop),
            "reverse_playback" => Some(Action::ReversePlayback),
            "volume_up" => Some(Action::VolumeUp),
            "volume_down" => Some(Action::VolumeDown),
            "speed_inc_cents" => Some(Action::SpeedIncCents),
            "speed_dec_cents" => Some(Action::SpeedDecCents),
            "speed_inc_cents_fine" => Some(Action::SpeedIncCentsFine),
            "speed_dec_cents_fine" => Some(Action::SpeedDecCentsFine),
            "speed_inc_bpm" => Some(Action::SpeedIncBpm),
            "speed_dec_bpm" => Some(Action::SpeedDecBpm),
            "speed_inc_bpm_fine" => Some(Action::SpeedIncBpmFine),
            "speed_dec_bpm_fine" => Some(Action::SpeedDecBpmFine),
            "speed_reset" => Some(Action::SpeedReset),
            "session_bpm_inc" => Some(Action::SessionBpmInc),
            "session_bpm_dec" => Some(Action::SessionBpmDec),
            "session_bpm_inc_fine" => Some(Action::SessionBpmIncFine),
            "session_bpm_dec_fine" => Some(Action::SessionBpmDecFine),
            "toggle_mark" => Some(Action::ToggleMark),
            "clear_marks" => Some(Action::ClearMarks),
            "toggle_marked_filter" => Some(Action::ToggleMarkedFilter),
            "save_markers" => Some(Action::SaveMarkers),
            "toggle_bank" => Some(Action::ToggleBank),
            "toggle_bank_sync" => Some(Action::ToggleBankSync),
            "set_marker_1" => Some(Action::SetMarker1),
            "set_marker_2" => Some(Action::SetMarker2),
            "set_marker_3" => Some(Action::SetMarker3),
            "clear_nearest_marker" => Some(Action::ClearNearestMarker),
            "clear_bank_markers" => Some(Action::ClearBankMarkers),
            "increment_rep" => Some(Action::IncrementRep),
            "decrement_rep" => Some(Action::DecrementRep),
            "select_next_marker" => Some(Action::SelectNextMarker),
            "select_prev_marker" => Some(Action::SelectPrevMarker),
            "toggle_infinite_loop" => Some(Action::ToggleInfiniteLoop),
            "toggle_preview_loop" => Some(Action::TogglePreviewLoop),
            "nudge_marker_forward_small" => Some(Action::NudgeMarkerForwardSmall),
            "nudge_marker_backward_small" => Some(Action::NudgeMarkerBackwardSmall),
            "nudge_marker_forward_large" => Some(Action::NudgeMarkerForwardLarge),
            "nudge_marker_backward_large" => Some(Action::NudgeMarkerBackwardLarge),
            "snap_zero_crossing_forward" => Some(Action::SnapZeroCrossingForward),
            "snap_zero_crossing_backward" => Some(Action::SnapZeroCrossingBackward),
            "marker_reset" => Some(Action::MarkerReset),
            "export_markers_csv" => Some(Action::ExportMarkersCsv),
            "import_markers_csv" => Some(Action::ImportMarkersCsv),
            "toggle_marker_display" => Some(Action::ToggleMarkerDisplay),
            "zoom_in" => Some(Action::ZoomIn),
            "zoom_out" => Some(Action::ZoomOut),
            "zoom_reset" => Some(Action::ZoomReset),
            "enter_insert_mode" => Some(Action::EnterInsertMode),
            "enter_normal_mode" => Some(Action::EnterNormalMode),
            "search_submit" => Some(Action::SearchSubmit),
            "clear_query" => Some(Action::ClearQuery),
            "open_selected" => Some(Action::OpenSelected),
            "show_help" => Some(Action::ShowHelp),
            "quit" => Some(Action::Quit),
            _ => None,
        }
    }

    /// Canonical string name for this action (for keymap config).
    pub fn name(&self) -> &'static str {
        match self {
            Action::MoveDown => "move_down",
            Action::MoveUp => "move_up",
            Action::MoveToTop => "move_to_top",
            Action::MoveToBottom => "move_to_bottom",
            Action::PageDown => "page_down",
            Action::PageUp => "page_up",
            Action::MoveColumnLeft => "move_column_left",
            Action::MoveColumnRight => "move_column_right",
            Action::SortAscending => "sort_ascending",
            Action::SortDescending => "sort_descending",
            Action::RandomSort => "random_sort",
            Action::SortBySimilarity => "sort_by_similarity",
            Action::TogglePlayback => "toggle_playback",
            Action::SeekForwardSmall => "seek_forward_small",
            Action::SeekForwardLarge => "seek_forward_large",
            Action::SeekBackwardSmall => "seek_backward_small",
            Action::SeekBackwardLarge => "seek_backward_large",
            Action::RewindToStart => "rewind_to_start",
            Action::ToggleAutoAdvance => "toggle_auto_advance",
            Action::ToggleTimeDisplay => "toggle_time_display",
            Action::ToggleGlobalLoop => "toggle_global_loop",
            Action::ReversePlayback => "reverse_playback",
            Action::VolumeUp => "volume_up",
            Action::VolumeDown => "volume_down",
            Action::SpeedIncCents => "speed_inc_cents",
            Action::SpeedDecCents => "speed_dec_cents",
            Action::SpeedIncCentsFine => "speed_inc_cents_fine",
            Action::SpeedDecCentsFine => "speed_dec_cents_fine",
            Action::SpeedIncBpm => "speed_inc_bpm",
            Action::SpeedDecBpm => "speed_dec_bpm",
            Action::SpeedIncBpmFine => "speed_inc_bpm_fine",
            Action::SpeedDecBpmFine => "speed_dec_bpm_fine",
            Action::SpeedReset => "speed_reset",
            Action::SessionBpmInc => "session_bpm_inc",
            Action::SessionBpmDec => "session_bpm_dec",
            Action::SessionBpmIncFine => "session_bpm_inc_fine",
            Action::SessionBpmDecFine => "session_bpm_dec_fine",
            Action::ToggleMark => "toggle_mark",
            Action::ClearMarks => "clear_marks",
            Action::ToggleMarkedFilter => "toggle_marked_filter",
            Action::SaveMarkers => "save_markers",
            Action::ToggleBank => "toggle_bank",
            Action::ToggleBankSync => "toggle_bank_sync",
            Action::SetMarker1 => "set_marker_1",
            Action::SetMarker2 => "set_marker_2",
            Action::SetMarker3 => "set_marker_3",
            Action::ClearNearestMarker => "clear_nearest_marker",
            Action::ClearBankMarkers => "clear_bank_markers",
            Action::IncrementRep => "increment_rep",
            Action::DecrementRep => "decrement_rep",
            Action::SelectNextMarker => "select_next_marker",
            Action::SelectPrevMarker => "select_prev_marker",
            Action::ToggleInfiniteLoop => "toggle_infinite_loop",
            Action::TogglePreviewLoop => "toggle_preview_loop",
            Action::NudgeMarkerForwardSmall => "nudge_marker_forward_small",
            Action::NudgeMarkerBackwardSmall => "nudge_marker_backward_small",
            Action::NudgeMarkerForwardLarge => "nudge_marker_forward_large",
            Action::NudgeMarkerBackwardLarge => "nudge_marker_backward_large",
            Action::SnapZeroCrossingForward => "snap_zero_crossing_forward",
            Action::SnapZeroCrossingBackward => "snap_zero_crossing_backward",
            Action::MarkerReset => "marker_reset",
            Action::ExportMarkersCsv => "export_markers_csv",
            Action::ImportMarkersCsv => "import_markers_csv",
            Action::ToggleMarkerDisplay => "toggle_marker_display",
            Action::ZoomIn => "zoom_in",
            Action::ZoomOut => "zoom_out",
            Action::ZoomReset => "zoom_reset",
            Action::EnterInsertMode => "enter_insert_mode",
            Action::EnterNormalMode => "enter_normal_mode",
            Action::SearchSubmit => "search_submit",
            Action::ClearQuery => "clear_query",
            Action::OpenSelected => "open_selected",
            Action::ShowHelp => "show_help",
            Action::Quit => "quit",
        }
    }
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.description())
    }
}

impl Action {
    /// Human-readable description of this action (for help overlay).
    pub fn description(&self) -> &'static str {
        match self {
            Action::MoveDown => "Move down one row",
            Action::MoveUp => "Move up one row",
            Action::MoveToTop => "Jump to first row",
            Action::MoveToBottom => "Jump to last row",
            Action::PageDown => "Page down",
            Action::PageUp => "Page up",
            Action::MoveColumnLeft => "Move column left",
            Action::MoveColumnRight => "Move column right",
            Action::SortAscending => "Sort column ascending",
            Action::SortDescending => "Sort column descending",
            Action::RandomSort => "Shuffle results randomly",
            Action::SortBySimilarity => "Sort by similarity to selected",
            Action::TogglePlayback => "Play / pause",
            Action::SeekForwardSmall => "Seek forward (small)",
            Action::SeekForwardLarge => "Seek forward (large)",
            Action::SeekBackwardSmall => "Seek backward (small)",
            Action::SeekBackwardLarge => "Seek backward (large)",
            Action::RewindToStart => "Rewind to start",
            Action::ToggleAutoAdvance => "Toggle auto-advance",
            Action::ToggleTimeDisplay => "Toggle elapsed/remaining",
            Action::ToggleGlobalLoop => "Toggle global loop",
            Action::ReversePlayback => "Reverse playback",
            Action::VolumeUp => "Volume up (+1 dBFS)",
            Action::VolumeDown => "Volume down (-1 dBFS)",
            Action::SpeedIncCents => "Speed +100¢ (coarse)",
            Action::SpeedDecCents => "Speed -100¢ (coarse)",
            Action::SpeedIncCentsFine => "Speed +1¢ (fine)",
            Action::SpeedDecCentsFine => "Speed -1¢ (fine)",
            Action::SpeedIncBpm => "Speed +1 BPM (linear)",
            Action::SpeedDecBpm => "Speed -1 BPM (linear)",
            Action::SpeedIncBpmFine => "Speed +0.1 BPM (fine)",
            Action::SpeedDecBpmFine => "Speed -0.1 BPM (fine)",
            Action::SpeedReset => "Reset speed to 1×",
            Action::SessionBpmInc => "Session BPM +1",
            Action::SessionBpmDec => "Session BPM -1",
            Action::SessionBpmIncFine => "Session BPM +0.1",
            Action::SessionBpmDecFine => "Session BPM -0.1",
            Action::ToggleMark => "Toggle mark on row",
            Action::ClearMarks => "Clear all marks",
            Action::ToggleMarkedFilter => "Filter to marked only",
            Action::SaveMarkers => "Save markers to file",
            Action::ToggleBank => "Toggle marker bank A/B",
            Action::ToggleBankSync => "Toggle bank sync",
            Action::SetMarker1 => "Set marker 1 at cursor",
            Action::SetMarker2 => "Set marker 2 at cursor",
            Action::SetMarker3 => "Set marker 3 at cursor",
            Action::ClearNearestMarker => "Clear nearest marker",
            Action::ClearBankMarkers => "Clear all bank markers",
            Action::IncrementRep => "Increment segment rep",
            Action::DecrementRep => "Decrement segment rep",
            Action::SelectNextMarker => "Select next marker",
            Action::SelectPrevMarker => "Select previous marker",
            Action::ToggleInfiniteLoop => "Toggle infinite loop",
            Action::TogglePreviewLoop => "Toggle preview loop",
            Action::NudgeMarkerForwardSmall => "Nudge marker forward (small)",
            Action::NudgeMarkerBackwardSmall => "Nudge marker backward (small)",
            Action::NudgeMarkerForwardLarge => "Nudge marker forward (large)",
            Action::NudgeMarkerBackwardLarge => "Nudge marker backward (large)",
            Action::SnapZeroCrossingForward => "Snap to zero-crossing forward",
            Action::SnapZeroCrossingBackward => "Snap to zero-crossing backward",
            Action::MarkerReset => "Reset markers to preset",
            Action::ExportMarkersCsv => "Export markers to CSV",
            Action::ImportMarkersCsv => "Import markers from CSV",
            Action::ToggleMarkerDisplay => "Toggle marker lines on/off",
            Action::ZoomIn => "Zoom waveform in",
            Action::ZoomOut => "Zoom waveform out",
            Action::ZoomReset => "Reset waveform zoom",
            Action::EnterInsertMode => "Enter search mode",
            Action::EnterNormalMode => "Exit search mode",
            Action::SearchSubmit => "Submit search",
            Action::ClearQuery => "Clear search query",
            Action::OpenSelected => "Open selected file",
            Action::ShowHelp => "Toggle help overlay",
            Action::Quit => "Quit",
        }
    }

    /// Action category for grouping in help display.
    pub fn category(&self) -> &'static str {
        match self {
            Action::MoveDown
            | Action::MoveUp
            | Action::MoveToTop
            | Action::MoveToBottom
            | Action::PageDown
            | Action::PageUp
            | Action::MoveColumnLeft
            | Action::MoveColumnRight => "Navigation",
            Action::SortAscending | Action::SortDescending | Action::RandomSort | Action::SortBySimilarity => "Sort",
            Action::TogglePlayback
            | Action::SeekForwardSmall
            | Action::SeekForwardLarge
            | Action::SeekBackwardSmall
            | Action::SeekBackwardLarge
            | Action::RewindToStart
            | Action::ToggleAutoAdvance
            | Action::ToggleTimeDisplay
            | Action::ToggleGlobalLoop
            | Action::ReversePlayback
            | Action::VolumeUp
            | Action::VolumeDown
            | Action::SpeedIncCents
            | Action::SpeedDecCents
            | Action::SpeedIncCentsFine
            | Action::SpeedDecCentsFine
            | Action::SpeedIncBpm
            | Action::SpeedDecBpm
            | Action::SpeedIncBpmFine
            | Action::SpeedDecBpmFine
            | Action::SpeedReset
            | Action::SessionBpmInc
            | Action::SessionBpmDec
            | Action::SessionBpmIncFine
            | Action::SessionBpmDecFine => "Playback",
            Action::ToggleMark
            | Action::ClearMarks
            | Action::ToggleMarkedFilter
            | Action::SaveMarkers => "Marks",
            Action::ToggleBank
            | Action::ToggleBankSync
            | Action::SetMarker1
            | Action::SetMarker2
            | Action::SetMarker3
            | Action::ClearNearestMarker
            | Action::ClearBankMarkers
            | Action::IncrementRep
            | Action::DecrementRep
            | Action::SelectNextMarker
            | Action::SelectPrevMarker
            | Action::ToggleInfiniteLoop
            | Action::TogglePreviewLoop
            | Action::NudgeMarkerForwardSmall
            | Action::NudgeMarkerBackwardSmall
            | Action::NudgeMarkerForwardLarge
            | Action::NudgeMarkerBackwardLarge
            | Action::SnapZeroCrossingForward
            | Action::SnapZeroCrossingBackward
            | Action::MarkerReset
            | Action::ExportMarkersCsv
            | Action::ImportMarkersCsv
            | Action::ToggleMarkerDisplay => "Markers",
            Action::ZoomIn | Action::ZoomOut | Action::ZoomReset => "Waveform",
            Action::EnterInsertMode
            | Action::EnterNormalMode
            | Action::SearchSubmit
            | Action::ClearQuery => "Mode",
            Action::OpenSelected => "Selection",
            Action::ShowHelp | Action::Quit => "App",
        }
    }

    /// Returns true if this action edits markers and should be guarded
    /// against execution during playback.
    pub fn is_marker_edit(&self) -> bool {
        matches!(
            self,
            Action::SetMarker1
                | Action::SetMarker2
                | Action::SetMarker3
                | Action::ClearNearestMarker
                | Action::ClearBankMarkers
                | Action::IncrementRep
                | Action::DecrementRep
                | Action::SaveMarkers
                | Action::ToggleInfiniteLoop
                | Action::TogglePreviewLoop
                | Action::NudgeMarkerForwardSmall
                | Action::NudgeMarkerBackwardSmall
                | Action::NudgeMarkerForwardLarge
                | Action::NudgeMarkerBackwardLarge
                | Action::SnapZeroCrossingForward
                | Action::SnapZeroCrossingBackward
                | Action::MarkerReset
                | Action::ExportMarkersCsv
                | Action::ImportMarkersCsv
        )
    }
}

/// Parse a key name string into a crossterm KeyEvent.
///
/// Handles: single chars ("j", "G"), special keys ("Space", "Esc", "Enter",
/// "Up", "Down", "Backspace", "Tab", "/", "?"), modifier combos ("Ctrl-C",
/// "Ctrl-D", "Ctrl-U"), Ctrl+Shift ("Ctrl-S-Right"), Ctrl+Alt/Opt
/// ("Ctrl-Alt-d", "Ctrl-Opt-d"), Ctrl+Arrow ("Ctrl-Left", "Ctrl-Right"),
/// Cmd+Ctrl ("Cmd-Ctrl-h", "Cmd-Ctrl-H" — uppercase implies Shift),
/// standalone Alt/Option ("Alt-x", "Opt-X" — uppercase implies Shift), and
/// standalone Cmd ("Cmd-k" — macOS terminals typically intercept these).
pub fn parse_key(s: &str) -> Option<KeyEvent> {
    // Modifier prefix: Cmd-Ctrl- (Super+Control; uppercase char → adds SHIFT)
    if let Some(rest) = s.strip_prefix("Cmd-Ctrl-") {
        let ch = rest.chars().next()?;
        if rest.len() != ch.len_utf8() {
            return None;
        }
        let mods = if ch.is_ascii_uppercase() {
            KeyModifiers::SUPER | KeyModifiers::CONTROL | KeyModifiers::SHIFT
        } else {
            KeyModifiers::SUPER | KeyModifiers::CONTROL
        };
        return Some(KeyEvent::new(KeyCode::Char(ch), mods));
    }

    // Modifier prefix: Ctrl-Alt- / Ctrl-Opt- (must check before standalone Alt-/Opt-)
    for prefix in &["Ctrl-Alt-", "Ctrl-Opt-"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            let ch = rest.chars().next()?;
            if rest.len() != ch.len_utf8() {
                return None;
            }
            return Some(KeyEvent::new(
                KeyCode::Char(ch.to_ascii_lowercase()),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            ));
        }
    }

    // Modifier prefix: Alt- / Opt- (standalone; uppercase char → adds SHIFT)
    // Note: on macOS these require the terminal to forward Option as Meta (escape prefix).
    for prefix in &["Alt-", "Opt-"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            let ch = rest.chars().next()?;
            if rest.len() != ch.len_utf8() {
                return None;
            }
            let mods = if ch.is_ascii_uppercase() {
                KeyModifiers::ALT | KeyModifiers::SHIFT
            } else {
                KeyModifiers::ALT
            };
            return Some(KeyEvent::new(KeyCode::Char(ch), mods));
        }
    }

    // Modifier prefix: Cmd- (SUPER alone; uppercase char → adds SHIFT).
    // Note: macOS terminals typically intercept Command key combos at the OS level
    // before they reach the application. These bindings will parse but may not fire.
    if let Some(rest) = s.strip_prefix("Cmd-") {
        match rest {
            "Left" => return Some(KeyEvent::new(KeyCode::Left, KeyModifiers::SUPER)),
            "Right" => return Some(KeyEvent::new(KeyCode::Right, KeyModifiers::SUPER)),
            "Up" => return Some(KeyEvent::new(KeyCode::Up, KeyModifiers::SUPER)),
            "Down" => return Some(KeyEvent::new(KeyCode::Down, KeyModifiers::SUPER)),
            _ => {}
        }
        let ch = rest.chars().next()?;
        if rest.len() != ch.len_utf8() {
            return None;
        }
        let mods = if ch.is_ascii_uppercase() {
            KeyModifiers::SUPER | KeyModifiers::SHIFT
        } else {
            KeyModifiers::SUPER
        };
        return Some(KeyEvent::new(KeyCode::Char(ch), mods));
    }

    // Modifier prefix: Ctrl-S- / Ctrl-Shift- (Ctrl+Shift + special key)
    for prefix in &["Ctrl-S-", "Ctrl-Shift-"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            let inner = match rest {
                "Left" => KeyCode::Left,
                "Right" => KeyCode::Right,
                "Up" => KeyCode::Up,
                "Down" => KeyCode::Down,
                _ => return None,
            };
            return Some(KeyEvent::new(inner, KeyModifiers::CONTROL | KeyModifiers::SHIFT));
        }
    }

    // Modifier prefix: Ctrl-
    if let Some(rest) = s.strip_prefix("Ctrl-") {
        // Arrow keys with Ctrl (must check before single-char fallback).
        match rest {
            "Left" => return Some(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL)),
            "Right" => return Some(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL)),
            "Up" => return Some(KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL)),
            "Down" => return Some(KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL)),
            _ => {}
        }
        // Single char (bracket chars like ] and [ are supported).
        let ch = rest.chars().next()?;
        if rest.len() != ch.len_utf8() {
            return None;
        }
        return Some(KeyEvent::new(
            KeyCode::Char(ch.to_ascii_lowercase()),
            KeyModifiers::CONTROL,
        ));
    }

    // Modifier prefix: S- (Shift + special key)
    if let Some(rest) = s.strip_prefix("S-") {
        let inner = match rest {
            "Left" => KeyCode::Left,
            "Right" => KeyCode::Right,
            "Up" => KeyCode::Up,
            "Down" => KeyCode::Down,
            "Tab" => KeyCode::BackTab,
            _ => return None,
        };
        return Some(KeyEvent::new(inner, KeyModifiers::SHIFT));
    }

    // Special key names.
    match s {
        "Space" => return Some(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)),
        "Esc" => return Some(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        "Enter" => return Some(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        "Backspace" => return Some(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
        "Tab" => return Some(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
        "Up" => return Some(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
        "Down" => return Some(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        "Left" => return Some(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
        "Right" => return Some(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
        _ => {}
    }

    // Single character — detect shift for uppercase.
    let mut chars = s.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None; // Multi-char string that's not a known special key.
    }

    if ch.is_ascii_uppercase() {
        Some(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::SHIFT))
    } else {
        Some(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
    }
}

/// Normalize a KeyEvent to a canonical (KeyCode, KeyModifiers) for map lookup.
///
/// For non-alpha characters (symbols like `?`, `!`, `/`), strips the SHIFT
/// modifier since terminals inconsistently report it — some send SHIFT for
/// Shift+/ → ?, others don't. For ASCII letters, SHIFT is preserved to
/// distinguish e.g. 'g' (no shift) from 'G' (shift).
fn normalize_key(key: KeyEvent) -> (KeyCode, KeyModifiers) {
    match key.code {
        KeyCode::Char(c) if !c.is_ascii_alphabetic() => {
            (key.code, key.modifiers & !KeyModifiers::SHIFT)
        }
        _ => (key.code, key.modifiers),
    }
}

/// Normal mode keymap: maps key events to actions.
#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: HashMap<(KeyCode, KeyModifiers), Action>,
}

impl Keymap {
    /// Build the default keymap (matches all hardcoded bindings from pre-T6).
    pub fn default_keymap() -> Self {
        let mut bindings = HashMap::new();
        let none = KeyModifiers::NONE;
        let shift = KeyModifiers::SHIFT;
        let ctrl = KeyModifiers::CONTROL;
        let ctrl_shift = KeyModifiers::CONTROL | KeyModifiers::SHIFT;

        // Navigation (j/k for row nav; Up/Down now control volume)
        bindings.insert((KeyCode::Char('j'), none), Action::MoveDown);
        bindings.insert((KeyCode::Char('k'), none), Action::MoveUp);
        bindings.insert((KeyCode::Char('G'), shift), Action::MoveToBottom);
        bindings.insert((KeyCode::Char('d'), ctrl), Action::PageDown);
        bindings.insert((KeyCode::Char('u'), ctrl), Action::PageUp);
        bindings.insert((KeyCode::Char('h'), none), Action::MoveColumnLeft);
        bindings.insert((KeyCode::Char('l'), none), Action::MoveColumnRight);

        // Sort
        bindings.insert((KeyCode::Char('o'), none), Action::SortAscending);
        bindings.insert((KeyCode::Char('O'), shift), Action::SortDescending);
        bindings.insert((KeyCode::Char('r'), none), Action::RandomSort);

        // Playback
        bindings.insert((KeyCode::Char(' '), none), Action::TogglePlayback);
        // Volume: Up/Down arrows (j/k still navigate rows)
        bindings.insert((KeyCode::Up, none), Action::VolumeUp);
        bindings.insert((KeyCode::Down, none), Action::VolumeDown);
        // Speed (pitch): Ctrl-Up/Down for coarse ±100¢, Ctrl-Shift-Up/Down for fine ±1¢.
        // Arrow-key combos are reliably forwarded by Terminal.app and most terminals.
        // Ctrl + punctuation (e.g. Ctrl-. or Ctrl-Alt-.) is NOT forwarded by Terminal.app
        // and so is avoided here. BPM-relative actions (SpeedIncBpm etc.) are left unbound
        // by default — they require both a BPM metadata tag and Option=Meta in terminal
        // preferences; users who want them can bind via [keymap] in config.toml.
        bindings.insert((KeyCode::Up, ctrl), Action::SpeedIncCents);
        bindings.insert((KeyCode::Down, ctrl), Action::SpeedDecCents);
        bindings.insert((KeyCode::Up, ctrl_shift), Action::SpeedIncCentsFine);
        bindings.insert((KeyCode::Down, ctrl_shift), Action::SpeedDecCentsFine);
        // Speed reset: Ctrl-/ (0x1F in most terminals; may need testing on your terminal).
        bindings.insert((KeyCode::Char('/'), ctrl), Action::SpeedReset);
        // Session BPM: . / , (coarse), > / < (fine)
        bindings.insert((KeyCode::Char('.'), none), Action::SessionBpmInc);
        bindings.insert((KeyCode::Char(','), none), Action::SessionBpmDec);
        // '>' = Shift+. and '<' = Shift+, — after normalize_key SHIFT is stripped,
        // so we bind the raw char directly.
        bindings.insert((KeyCode::Char('>'), none), Action::SessionBpmIncFine);
        bindings.insert((KeyCode::Char('<'), none), Action::SessionBpmDecFine);
        bindings.insert((KeyCode::Right, none), Action::SeekForwardSmall);
        bindings.insert((KeyCode::Left, none), Action::SeekBackwardSmall);
        bindings.insert((KeyCode::Right, shift), Action::SeekForwardLarge);
        bindings.insert((KeyCode::Left, shift), Action::SeekBackwardLarge);
        bindings.insert((KeyCode::Char('g'), none), Action::RewindToStart);
        bindings.insert((KeyCode::Char('a'), none), Action::ToggleAutoAdvance);
        bindings.insert((KeyCode::Char('t'), none), Action::ToggleTimeDisplay);
        bindings.insert((KeyCode::Char('p'), none), Action::ToggleGlobalLoop);

        // Marks
        bindings.insert((KeyCode::Char('m'), none), Action::ToggleMark);
        bindings.insert((KeyCode::Char('M'), shift), Action::ClearMarks);
        bindings.insert((KeyCode::Char('f'), none), Action::ToggleMarkedFilter);
        bindings.insert((KeyCode::Char('w'), none), Action::SaveMarkers);

        // Markers
        bindings.insert((KeyCode::Char('b'), none), Action::ToggleBank);
        bindings.insert((KeyCode::Char('B'), shift), Action::ToggleBankSync);
        bindings.insert((KeyCode::Char('1'), none), Action::SetMarker1);
        bindings.insert((KeyCode::Char('2'), none), Action::SetMarker2);
        bindings.insert((KeyCode::Char('3'), none), Action::SetMarker3);
        bindings.insert((KeyCode::Char('x'), none), Action::ClearNearestMarker);
        bindings.insert((KeyCode::Char('X'), shift), Action::ClearBankMarkers);
        // IncrementRep → Ctrl-K
        bindings.insert((KeyCode::Char('k'), ctrl), Action::IncrementRep);
        // DecrementRep: Ctrl-J omitted (terminals send it as Enter/0x0A). Use Opt-j instead
        // (requires terminal Option=Meta; pairs naturally with Ctrl-K for increment).
        bindings.insert((KeyCode::Char('j'), KeyModifiers::ALT), Action::DecrementRep);
        // SelectNextMarker → Ctrl-L
        bindings.insert((KeyCode::Char('l'), ctrl), Action::SelectNextMarker);
        // SelectPrevMarker → Opt-H  (Ctrl-H = Backspace in terminals; Opt requires Option=Meta)
        bindings.insert((KeyCode::Char('h'), KeyModifiers::ALT), Action::SelectPrevMarker);
        // ToggleInfiniteLoop → Ctrl-o
        bindings.insert((KeyCode::Char('o'), ctrl), Action::ToggleInfiniteLoop);
        bindings.insert((KeyCode::Char('p'), ctrl), Action::TogglePreviewLoop);

        // Waveform zoom
        bindings.insert((KeyCode::Char('='), none), Action::ZoomIn);
        bindings.insert((KeyCode::Char('-'), none), Action::ZoomOut);
        bindings.insert((KeyCode::Char('0'), none), Action::ZoomReset);
        // Nudge: Ctrl-Left/Right (small), Ctrl-Shift-Left/Right (large).
        // Note: Cmd-Ctrl-* won't work in any standard macOS terminal (SUPER not forwarded).
        bindings.insert((KeyCode::Right, ctrl), Action::NudgeMarkerForwardSmall);
        bindings.insert((KeyCode::Left, ctrl), Action::NudgeMarkerBackwardSmall);
        bindings.insert((KeyCode::Right, ctrl_shift), Action::NudgeMarkerForwardLarge);
        bindings.insert((KeyCode::Left, ctrl_shift), Action::NudgeMarkerBackwardLarge);
        bindings.insert((KeyCode::Char(']'), ctrl), Action::SnapZeroCrossingForward);
        bindings.insert((KeyCode::Char('['), ctrl), Action::SnapZeroCrossingBackward);
        bindings.insert((KeyCode::Char('r'), ctrl), Action::MarkerReset);
        bindings.insert((KeyCode::Char('e'), ctrl), Action::ExportMarkersCsv);
        bindings.insert((KeyCode::Char('i'), ctrl), Action::ImportMarkersCsv);
        // ToggleMarkerDisplay → Ctrl-Alt-d.
        // Note: Ctrl-m = 0x0D (carriage return) — terminals convert it to Enter,
        // so Ctrl-Alt-m would arrive as Alt+Enter and never match.
        bindings.insert(
            (KeyCode::Char('d'), KeyModifiers::CONTROL | KeyModifiers::ALT),
            Action::ToggleMarkerDisplay,
        );

        // Mode
        bindings.insert((KeyCode::Char('i'), none), Action::EnterInsertMode);
        bindings.insert((KeyCode::Char('/'), none), Action::EnterInsertMode);

        // Selection
        bindings.insert((KeyCode::Enter, none), Action::OpenSelected);

        // Help (NONE because normalize_key strips SHIFT from non-alpha)
        bindings.insert((KeyCode::Char('?'), none), Action::ShowHelp);

        // App
        bindings.insert((KeyCode::Char('q'), none), Action::Quit);

        Self { bindings }
    }

    /// Build a keymap from defaults with config overrides applied.
    pub fn with_overrides(overrides: &HashMap<String, String>) -> Self {
        let mut keymap = Self::default_keymap();
        for (key_name, action_name) in overrides {
            let Some(key_event) = parse_key(key_name) else {
                eprintln!("riffgrep: warning: unknown key '{key_name}' in [keymap], ignoring");
                continue;
            };
            let Some(action) = Action::from_name(action_name) else {
                eprintln!(
                    "riffgrep: warning: unknown action '{action_name}' in [keymap], ignoring"
                );
                continue;
            };
            keymap.bindings.insert(normalize_key(key_event), action);
        }
        keymap
    }

    /// Resolve a key event to an action (Normal mode only).
    ///
    /// Returns `None` if the key has no binding. The `gg` sequence and Esc
    /// context-dependent behavior are handled by the caller.
    pub fn resolve(&self, key: KeyEvent) -> Option<Action> {
        self.bindings.get(&normalize_key(key)).copied()
    }
}

/// Format a KeyCode + modifiers as a human-readable string for help display.
pub fn key_display(code: &KeyCode, modifiers: &KeyModifiers) -> String {
    let mut s = String::new();
    if modifiers.contains(KeyModifiers::SUPER) && modifiers.contains(KeyModifiers::CONTROL) {
        // Cmd-Ctrl-h (lowercase) or Cmd-Ctrl-H (uppercase = Shift implied).
        s.push_str("Cmd-Ctrl-");
    } else if modifiers.contains(KeyModifiers::CONTROL) && modifiers.contains(KeyModifiers::ALT) {
        s.push_str("Ctrl-Alt-");
    } else if modifiers.contains(KeyModifiers::CONTROL) && modifiers.contains(KeyModifiers::SHIFT) {
        match code {
            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => {
                s.push_str("Ctrl-S-");
            }
            _ => s.push_str("Ctrl-"),
        }
    } else if modifiers.contains(KeyModifiers::CONTROL) {
        s.push_str("Ctrl-");
    } else if modifiers.contains(KeyModifiers::SUPER) {
        s.push_str("Cmd-");
    } else if modifiers.contains(KeyModifiers::ALT) {
        s.push_str("Opt-");
    } else if modifiers.contains(KeyModifiers::SHIFT) {
        // Shift prefix for non-char keys (arrows, etc).
        match code {
            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => {
                s.push_str("S-");
            }
            KeyCode::BackTab => {
                s.push_str("S-");
            }
            _ => {}
        }
    }
    match code {
        KeyCode::Char(' ') => s.push_str("Space"),
        KeyCode::Char(c) => s.push(*c),
        KeyCode::Enter => s.push_str("Enter"),
        KeyCode::Esc => s.push_str("Esc"),
        KeyCode::Backspace => s.push_str("Bksp"),
        KeyCode::Tab => s.push_str("Tab"),
        KeyCode::BackTab => s.push_str("Tab"),
        KeyCode::Up => s.push_str("Up"),
        KeyCode::Down => s.push_str("Down"),
        KeyCode::Left => s.push_str("Left"),
        KeyCode::Right => s.push_str("Right"),
        _ => s.push_str("?"),
    }
    s
}

impl Keymap {
    /// Return all bindings grouped by action category, sorted for display.
    pub fn help_entries(&self) -> Vec<(&'static str, Vec<(String, Action)>)> {
        use std::collections::BTreeMap;

        // Group bindings by category.
        let mut groups: BTreeMap<&'static str, Vec<(String, Action)>> = BTreeMap::new();
        for ((code, mods), &action) in &self.bindings {
            let key_str = key_display(code, mods);
            groups
                .entry(action.category())
                .or_default()
                .push((key_str, action));
        }

        // Sort keys within each group.
        for entries in groups.values_mut() {
            entries.sort_by(|a, b| a.1.name().cmp(b.1.name()).then(a.0.cmp(&b.0)));
        }

        // Return in a stable category order.
        let order = ["Navigation", "Sort", "Playback", "Marks", "Markers", "Waveform", "Mode", "Selection", "App"];
        let mut result = Vec::new();
        for &cat in &order {
            if let Some(entries) = groups.remove(cat) {
                result.push((cat, entries));
            }
        }
        // Append any remaining categories.
        for (cat, entries) in groups {
            result.push((cat, entries));
        }
        result
    }
}

impl Default for Keymap {
    fn default() -> Self {
        Self::default_keymap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_actions_have_dispatch_arm() {
        for &action in Action::ALL {
            let name = action.name();
            let parsed = Action::from_name(name);
            assert_eq!(parsed, Some(action), "round-trip failed for {:?}", action);
        }
    }

    #[test]
    fn test_all_count_matches_variants() {
        // 57 S12 + 1 RandomSort (Q1) + 1 SortBySimilarity + 15 audio controls (A1/A2/A3) = 74.
        assert_eq!(Action::ALL.len(), 74);
    }

    #[test]
    fn test_from_name_unknown_returns_none() {
        assert_eq!(Action::from_name("nonexistent"), None);
        assert_eq!(Action::from_name(""), None);
    }

    #[test]
    fn test_from_name_case_sensitive() {
        assert_eq!(Action::from_name("MoveDown"), None);
        assert_eq!(Action::from_name("move_down"), Some(Action::MoveDown));
    }

    // --- T6 tests: Keymap ---

    #[test]
    fn test_parse_key_lowercase() {
        let key = parse_key("j").unwrap();
        assert_eq!(key.code, KeyCode::Char('j'));
        assert_eq!(key.modifiers, KeyModifiers::NONE);
    }

    #[test]
    fn test_parse_key_uppercase_with_shift() {
        let key = parse_key("G").unwrap();
        assert_eq!(key.code, KeyCode::Char('G'));
        assert_eq!(key.modifiers, KeyModifiers::SHIFT);
    }

    #[test]
    fn test_parse_key_special_keys() {
        let space = parse_key("Space").unwrap();
        assert_eq!(space.code, KeyCode::Char(' '));

        let esc = parse_key("Esc").unwrap();
        assert_eq!(esc.code, KeyCode::Esc);

        let enter = parse_key("Enter").unwrap();
        assert_eq!(enter.code, KeyCode::Enter);

        let ctrl_c = parse_key("Ctrl-C").unwrap();
        assert_eq!(ctrl_c.code, KeyCode::Char('c'));
        assert_eq!(ctrl_c.modifiers, KeyModifiers::CONTROL);

        let ctrl_d = parse_key("Ctrl-D").unwrap();
        assert_eq!(ctrl_d.code, KeyCode::Char('d'));
        assert_eq!(ctrl_d.modifiers, KeyModifiers::CONTROL);
    }

    #[test]
    fn test_parse_key_invalid_returns_none() {
        assert!(parse_key("").is_none());
        assert!(parse_key("abc").is_none());
        assert!(parse_key("Ctrl-").is_none());
    }

    #[test]
    fn test_parse_action_valid() {
        assert_eq!(Action::from_name("move_down"), Some(Action::MoveDown));
        assert_eq!(Action::from_name("quit"), Some(Action::Quit));
        assert_eq!(
            Action::from_name("toggle_playback"),
            Some(Action::TogglePlayback)
        );
    }

    #[test]
    fn test_parse_action_invalid_returns_none() {
        assert_eq!(Action::from_name("fly_to_moon"), None);
    }

    #[test]
    fn test_default_keymap_has_all_bindings() {
        let km = Keymap::default_keymap();
        // Spot-check critical bindings.
        let j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(km.resolve(j), Some(Action::MoveDown));

        let q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        assert_eq!(km.resolve(q), Some(Action::Quit));

        let space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(km.resolve(space), Some(Action::TogglePlayback));

        let big_g = KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT);
        assert_eq!(km.resolve(big_g), Some(Action::MoveToBottom));

        let ctrl_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert_eq!(km.resolve(ctrl_d), Some(Action::PageDown));

        let i = KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE);
        assert_eq!(km.resolve(i), Some(Action::EnterInsertMode));
    }

    #[test]
    fn test_keymap_override_single_key() {
        let mut overrides = HashMap::new();
        overrides.insert("j".to_string(), "move_up".to_string());
        let km = Keymap::with_overrides(&overrides);
        let j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(km.resolve(j), Some(Action::MoveUp), "j should be overridden to MoveUp");
    }

    #[test]
    fn test_keymap_override_preserves_unmodified() {
        let mut overrides = HashMap::new();
        overrides.insert("j".to_string(), "move_up".to_string());
        let km = Keymap::with_overrides(&overrides);
        // k should still be move_up (default).
        let k = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
        assert_eq!(km.resolve(k), Some(Action::MoveUp));
        // q should still be quit.
        let q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        assert_eq!(km.resolve(q), Some(Action::Quit));
    }

    #[test]
    fn test_keymap_mode_aware() {
        let km = Keymap::default_keymap();
        // Unbound key returns None (Insert mode keys are not in the Normal keymap).
        let z = KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE);
        assert_eq!(km.resolve(z), None);
    }

    #[test]
    fn test_parse_key_slash() {
        let key = parse_key("/").unwrap();
        assert_eq!(key.code, KeyCode::Char('/'));
        assert_eq!(key.modifiers, KeyModifiers::NONE);
    }

    #[test]
    fn test_parse_key_question_mark() {
        let key = parse_key("?").unwrap();
        assert_eq!(key.code, KeyCode::Char('?'));
    }

    // --- S8-T4 tests: Scrub actions ---

    #[test]
    fn test_action_seek_variants_roundtrip() {
        for action in [
            Action::SeekForwardSmall,
            Action::SeekForwardLarge,
            Action::SeekBackwardSmall,
            Action::SeekBackwardLarge,
        ] {
            let name = action.name();
            assert_eq!(
                Action::from_name(name),
                Some(action),
                "round-trip failed for {name}"
            );
        }
    }

    #[test]
    fn test_default_keymap_has_seek_bindings() {
        let km = Keymap::default_keymap();
        let right = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(km.resolve(right), Some(Action::SeekForwardSmall));

        let left = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(km.resolve(left), Some(Action::SeekBackwardSmall));

        let s_right = KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT);
        assert_eq!(km.resolve(s_right), Some(Action::SeekForwardLarge));

        let s_left = KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT);
        assert_eq!(km.resolve(s_left), Some(Action::SeekBackwardLarge));
    }

    #[test]
    fn test_parse_key_shifted_arrows() {
        let s_left = parse_key("S-Left").unwrap();
        assert_eq!(s_left.code, KeyCode::Left);
        assert_eq!(s_left.modifiers, KeyModifiers::SHIFT);

        let s_right = parse_key("S-Right").unwrap();
        assert_eq!(s_right.code, KeyCode::Right);
        assert_eq!(s_right.modifiers, KeyModifiers::SHIFT);
    }

    // --- Sprint 11 tests ---

    #[test]
    fn test_action_all_count_final() {
        assert_eq!(Action::ALL.len(), 74, "Sprint 13: 74 = 57 + RandomSort + SortBySimilarity + 15 audio controls");
    }

    #[test]
    fn test_save_markers_action_roundtrip() {
        let name = Action::SaveMarkers.name();
        assert_eq!(name, "save_markers");
        assert_eq!(Action::from_name(name), Some(Action::SaveMarkers));
    }

    #[test]
    fn test_save_markers_category() {
        assert_eq!(Action::SaveMarkers.category(), "Marks");
    }

    #[test]
    fn test_keymap_has_save_markers() {
        let km = Keymap::default_keymap();
        let w = KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE);
        assert_eq!(km.resolve(w), Some(Action::SaveMarkers));
    }

    #[test]
    fn test_new_actions_roundtrip() {
        let new_actions = [
            Action::ToggleGlobalLoop,
            Action::ToggleBankSync,
            Action::SelectNextMarker,
            Action::SelectPrevMarker,
            Action::ToggleInfiniteLoop,
            Action::TogglePreviewLoop,
            Action::NudgeMarkerForwardSmall,
            Action::NudgeMarkerBackwardSmall,
            Action::NudgeMarkerForwardLarge,
            Action::NudgeMarkerBackwardLarge,
            Action::SnapZeroCrossingForward,
            Action::SnapZeroCrossingBackward,
            Action::MarkerReset,
            Action::ExportMarkersCsv,
            Action::ImportMarkersCsv,
            Action::ToggleMarkerDisplay,
            Action::ReversePlayback,
        ];
        for action in new_actions {
            let name = action.name();
            assert_eq!(
                Action::from_name(name),
                Some(action),
                "round-trip failed for {name}"
            );
        }
    }

    // --- Sprint 12 tests ---

    #[test]
    fn test_zoom_actions_roundtrip() {
        for action in [Action::ZoomIn, Action::ZoomOut, Action::ZoomReset] {
            let name = action.name();
            assert_eq!(Action::from_name(name), Some(action), "round-trip failed for {name}");
        }
    }

    #[test]
    fn test_zoom_actions_category() {
        assert_eq!(Action::ZoomIn.category(), "Waveform");
        assert_eq!(Action::ZoomOut.category(), "Waveform");
        assert_eq!(Action::ZoomReset.category(), "Waveform");
    }

    #[test]
    fn test_select_next_marker_bound_to_ctrl_l() {
        let km = Keymap::default_keymap();
        let ctrl_l = KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL);
        assert_eq!(km.resolve(ctrl_l), Some(Action::SelectNextMarker));
    }

    #[test]
    fn test_select_prev_marker_unbound_ctrl_h() {
        // Ctrl-H is omitted from default_keymap: terminals send it as Backspace (0x08).
        // Default is Opt-H (ALT modifier); see test_select_prev_marker_default_opt_h.
        let km = Keymap::default_keymap();
        let ctrl_h = KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL);
        assert_eq!(km.resolve(ctrl_h), None);
    }

    #[test]
    fn test_toggle_infinite_loop_bound_to_ctrl_o() {
        let km = Keymap::default_keymap();
        let ctrl_o = KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL);
        assert_eq!(km.resolve(ctrl_o), Some(Action::ToggleInfiniteLoop));
    }

    #[test]
    fn test_old_tab_binding_unbound() {
        let km = Keymap::default_keymap();
        let tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(km.resolve(tab), None, "TAB should no longer fire SelectNextMarker");
    }

    #[test]
    fn test_increment_rep_bound_to_ctrl_k() {
        let km = Keymap::default_keymap();
        let ctrl_k = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL);
        assert_eq!(km.resolve(ctrl_k), Some(Action::IncrementRep));
    }

    #[test]
    fn test_decrement_rep_unbound_ctrl_j() {
        // Ctrl-J is omitted from default_keymap: terminals send it as Enter (0x0A),
        // which would fire OpenSelected instead of DecrementRep.
        let km = Keymap::default_keymap();
        let ctrl_j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL);
        assert_eq!(km.resolve(ctrl_j), None);
    }

    #[test]
    fn test_zoom_in_bound_to_equals() {
        let km = Keymap::default_keymap();
        let eq = KeyEvent::new(KeyCode::Char('='), KeyModifiers::NONE);
        assert_eq!(km.resolve(eq), Some(Action::ZoomIn));
    }

    #[test]
    fn test_zoom_out_bound_to_minus() {
        let km = Keymap::default_keymap();
        let minus = KeyEvent::new(KeyCode::Char('-'), KeyModifiers::NONE);
        assert_eq!(km.resolve(minus), Some(Action::ZoomOut));
    }

    #[test]
    fn test_zoom_reset_bound_to_zero() {
        let km = Keymap::default_keymap();
        let zero = KeyEvent::new(KeyCode::Char('0'), KeyModifiers::NONE);
        assert_eq!(km.resolve(zero), Some(Action::ZoomReset));
    }

    #[test]
    fn test_toggle_marker_display_roundtrip() {
        let name = Action::ToggleMarkerDisplay.name();
        assert_eq!(name, "toggle_marker_display");
        assert_eq!(Action::from_name(name), Some(Action::ToggleMarkerDisplay));
    }

    #[test]
    fn test_toggle_marker_display_bound_to_ctrl_alt_d() {
        // Ctrl-Alt-d is the canonical binding. Ctrl-Alt-m is deliberately avoided:
        // Ctrl-m = 0x0D (CR); terminals canonicalize it to Enter, so Ctrl-Alt-m
        // arrives as Alt+Enter and never matches the (Char('m'), CTRL|ALT) entry.
        let km = Keymap::default_keymap();
        let ctrl_alt_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL | KeyModifiers::ALT);
        assert_eq!(km.resolve(ctrl_alt_d), Some(Action::ToggleMarkerDisplay));

        // Ctrl-Alt-m must NOT be bound (would be unreachable anyway).
        let ctrl_alt_m = KeyEvent::new(KeyCode::Char('m'), KeyModifiers::CONTROL | KeyModifiers::ALT);
        assert_eq!(km.resolve(ctrl_alt_m), None);
    }

    #[test]
    fn test_parse_key_ctrl_alt() {
        let key = parse_key("Ctrl-Alt-m").unwrap();
        assert_eq!(key.code, KeyCode::Char('m'));
        assert_eq!(key.modifiers, KeyModifiers::CONTROL | KeyModifiers::ALT);
    }

    #[test]
    fn test_parse_key_ctrl_shift_arrow() {
        let key = parse_key("Ctrl-S-Right").unwrap();
        assert_eq!(key.code, KeyCode::Right);
        assert_eq!(key.modifiers, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
    }

    #[test]
    fn test_parse_key_ctrl_shift_alias() {
        // "Ctrl-Shift-Right" is an alias for "Ctrl-S-Right".
        let key = parse_key("Ctrl-Shift-Right").unwrap();
        assert_eq!(key.code, KeyCode::Right);
        assert_eq!(key.modifiers, KeyModifiers::CONTROL | KeyModifiers::SHIFT);

        let left = parse_key("Ctrl-Shift-Left").unwrap();
        assert_eq!(left.code, KeyCode::Left);
        assert_eq!(left.modifiers, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
    }

    #[test]
    fn test_parse_key_shift_tab() {
        let key = parse_key("S-Tab").unwrap();
        assert_eq!(key.code, KeyCode::BackTab);
        assert_eq!(key.modifiers, KeyModifiers::SHIFT);
    }

    #[test]
    fn test_keymap_has_new_sprint11_bindings() {
        let km = Keymap::default_keymap();

        // p → ToggleGlobalLoop
        let p = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE);
        assert_eq!(km.resolve(p), Some(Action::ToggleGlobalLoop));

        // B → ToggleBankSync
        let big_b = KeyEvent::new(KeyCode::Char('B'), KeyModifiers::SHIFT);
        assert_eq!(km.resolve(big_b), Some(Action::ToggleBankSync));

        // Ctrl-L → SelectNextMarker (Tab was rebound in Sprint 12)
        let ctrl_l = KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL);
        assert_eq!(km.resolve(ctrl_l), Some(Action::SelectNextMarker));

        // Ctrl-r → MarkerReset
        let ctrl_r = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL);
        assert_eq!(km.resolve(ctrl_r), Some(Action::MarkerReset));
    }

    #[test]
    fn test_is_marker_edit() {
        assert!(Action::SetMarker1.is_marker_edit());
        assert!(Action::ClearNearestMarker.is_marker_edit());
        assert!(Action::MarkerReset.is_marker_edit());
        assert!(!Action::TogglePlayback.is_marker_edit());
        assert!(!Action::ToggleGlobalLoop.is_marker_edit());
        assert!(!Action::SelectNextMarker.is_marker_edit());
    }

    #[test]
    fn test_removed_play_segment_play_program() {
        // These actions were removed in Sprint 11.
        assert_eq!(Action::from_name("play_segment"), None);
        assert_eq!(Action::from_name("play_program"), None);
    }

    // --- Sprint 12 fixes tests ---

    #[test]
    fn test_rewind_to_start_bound_to_g() {
        let km = Keymap::default_keymap();
        let g = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE);
        assert_eq!(km.resolve(g), Some(Action::RewindToStart));
    }

    #[test]
    fn test_rewind_to_start_roundtrip() {
        let name = Action::RewindToStart.name();
        assert_eq!(name, "rewind_to_start");
        assert_eq!(Action::from_name(name), Some(Action::RewindToStart));
    }

    #[test]
    fn test_random_sort_bound_to_r() {
        let km = Keymap::default_keymap();
        let r = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE);
        assert_eq!(km.resolve(r), Some(Action::RandomSort), "'r' should be bound to RandomSort");
    }

    #[test]
    fn test_nudge_bindings_ctrl_arrow() {
        let km = Keymap::default_keymap();
        let ctrl = KeyModifiers::CONTROL;
        let ctrl_shift = KeyModifiers::CONTROL | KeyModifiers::SHIFT;

        let right = KeyEvent::new(KeyCode::Right, ctrl);
        assert_eq!(km.resolve(right), Some(Action::NudgeMarkerForwardSmall));

        let left = KeyEvent::new(KeyCode::Left, ctrl);
        assert_eq!(km.resolve(left), Some(Action::NudgeMarkerBackwardSmall));

        let big_right = KeyEvent::new(KeyCode::Right, ctrl_shift);
        assert_eq!(km.resolve(big_right), Some(Action::NudgeMarkerForwardLarge));

        let big_left = KeyEvent::new(KeyCode::Left, ctrl_shift);
        assert_eq!(km.resolve(big_left), Some(Action::NudgeMarkerBackwardLarge));

        // Old Cmd-Ctrl-h/l bindings no longer in default keymap.
        let cmd_ctrl = KeyModifiers::SUPER | KeyModifiers::CONTROL;
        let old_l = KeyEvent::new(KeyCode::Char('l'), cmd_ctrl);
        assert_eq!(km.resolve(old_l), None, "Cmd-Ctrl-l should no longer be nudge (SUPER not forwarded by terminals)");
    }

    #[test]
    fn test_decrement_rep_default_opt_j() {
        let km = Keymap::default_keymap();
        let opt_j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::ALT);
        assert_eq!(km.resolve(opt_j), Some(Action::DecrementRep));
    }

    #[test]
    fn test_select_prev_marker_default_opt_h() {
        let km = Keymap::default_keymap();
        let opt_h = KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT);
        assert_eq!(km.resolve(opt_h), Some(Action::SelectPrevMarker));
    }

    #[test]
    fn test_parse_key_ctrl_arrow() {
        let left = parse_key("Ctrl-Left").unwrap();
        assert_eq!(left.code, KeyCode::Left);
        assert_eq!(left.modifiers, KeyModifiers::CONTROL);

        let right = parse_key("Ctrl-Right").unwrap();
        assert_eq!(right.code, KeyCode::Right);
        assert_eq!(right.modifiers, KeyModifiers::CONTROL);

        let up = parse_key("Ctrl-Up").unwrap();
        assert_eq!(up.code, KeyCode::Up);
        assert_eq!(up.modifiers, KeyModifiers::CONTROL);

        let down = parse_key("Ctrl-Down").unwrap();
        assert_eq!(down.code, KeyCode::Down);
        assert_eq!(down.modifiers, KeyModifiers::CONTROL);
    }

    #[test]
    fn test_parse_key_cmd_ctrl() {
        // Lowercase → SUPER | CONTROL
        let h = parse_key("Cmd-Ctrl-h").unwrap();
        assert_eq!(h.code, KeyCode::Char('h'));
        assert_eq!(h.modifiers, KeyModifiers::SUPER | KeyModifiers::CONTROL);

        let l = parse_key("Cmd-Ctrl-l").unwrap();
        assert_eq!(l.code, KeyCode::Char('l'));
        assert_eq!(l.modifiers, KeyModifiers::SUPER | KeyModifiers::CONTROL);

        // Uppercase → SUPER | CONTROL | SHIFT
        let big_h = parse_key("Cmd-Ctrl-H").unwrap();
        assert_eq!(big_h.code, KeyCode::Char('H'));
        assert_eq!(big_h.modifiers, KeyModifiers::SUPER | KeyModifiers::CONTROL | KeyModifiers::SHIFT);

        let big_l = parse_key("Cmd-Ctrl-L").unwrap();
        assert_eq!(big_l.code, KeyCode::Char('L'));
        assert_eq!(big_l.modifiers, KeyModifiers::SUPER | KeyModifiers::CONTROL | KeyModifiers::SHIFT);
    }

    #[test]
    fn test_random_sort_roundtrip() {
        let name = Action::RandomSort.name();
        assert_eq!(name, "random_sort");
        assert_eq!(Action::from_name(name), Some(Action::RandomSort));
        assert_eq!(Action::RandomSort.category(), "Sort");
    }

    #[test]
    fn test_config_keymap_all_action_names_valid() {
        // Verify all canonical action names used in config round-trip through from_name().
        let names = [
            "move_down", "move_up", "move_to_bottom", "page_down", "page_up",
            "move_column_left", "move_column_right", "sort_ascending", "sort_descending",
            "random_sort",
            "toggle_playback", "seek_forward_small", "seek_backward_small",
            "seek_forward_large", "seek_backward_large", "rewind_to_start",
            "toggle_auto_advance", "toggle_time_display", "toggle_global_loop",
            "toggle_preview_loop", "toggle_mark", "clear_marks", "toggle_marked_filter",
            "toggle_bank", "toggle_bank_sync", "set_marker_1", "set_marker_2", "set_marker_3",
            "clear_nearest_marker", "clear_bank_markers", "save_markers",
            "increment_rep", "decrement_rep", "select_next_marker", "select_prev_marker",
            "toggle_infinite_loop", "toggle_marker_display",
            "nudge_marker_forward_small", "nudge_marker_backward_small",
            "nudge_marker_forward_large", "nudge_marker_backward_large",
            "snap_zero_crossing_forward", "snap_zero_crossing_backward",
            "marker_reset", "export_markers_csv", "import_markers_csv",
            "zoom_in", "zoom_out", "zoom_reset",
            "enter_insert_mode", "open_selected", "show_help", "quit",
        ];
        for name in names {
            assert!(Action::from_name(name).is_some(), "unknown action name in config: {name}");
        }
    }
}
