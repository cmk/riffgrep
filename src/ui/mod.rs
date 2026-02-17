//! Interactive TUI: state machine, event loop, and async bridge.

pub mod actions;
pub mod search;
pub mod theme;
pub mod widgets;

use std::collections::HashSet;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::engine::{TableRow, UnifiedMetadata};
use crate::engine::marks::MarkStore;
use crate::engine::playback::{PlaybackEngine, PlaybackState};
use theme::Theme;

/// Preview data for the selected result.
#[derive(Debug, Clone)]
pub struct PreviewData {
    /// Metadata for the previewed file.
    pub metadata: UnifiedMetadata,
    /// Decompressed peak data (180 u8 values, or empty).
    pub peaks: Vec<u8>,
    /// Audio format info (duration, sample rate, etc.) if available.
    pub audio_info: Option<crate::engine::wav::AudioInfo>,
    /// Marker configuration from BEXT v2, if available.
    pub markers: Option<crate::engine::bext::MarkerConfig>,
}

/// Events that drive the TUI state machine.
///
/// Reserved for future event loop refactoring (currently using raw crossterm + mpsc).
#[allow(dead_code)]
pub enum AppEvent {
    /// Keyboard input.
    Key(KeyEvent),
    /// Periodic tick for UI refresh.
    Tick,
    /// A batch of search results arrived.
    SearchResults(Vec<TableRow>),
    /// Search completed with total match count.
    SearchComplete(usize),
    /// Preview data is ready for the selected result.
    PreviewReady(PreviewData),
}

/// Default number of lines to scroll per page.
const PAGE_SIZE: usize = 20;

/// Input mode for the TUI. Normal mode routes keys to navigation actions;
/// Insert mode routes keys to the search field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Navigation keys active, search field read-only.
    Normal,
    /// All keys go to the search field (except Esc/Ctrl-C).
    Insert,
}

/// Active marker bank for editing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bank {
    /// Bank A (top half of waveform).
    A,
    /// Bank B (bottom half of waveform).
    B,
}

/// A playback segment derived from marker boundaries.
///
/// Always exactly 4 segments per bank, indexed by slot (0..4).
/// Direction is encoded by marker ordering: if the boundary at slot i
/// exceeds the boundary at slot i+1, the segment plays in reverse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    /// Absolute sample where playback begins.
    pub start: u32,
    /// Absolute sample where playback ends.
    pub end: u32,
    /// Repetition count: 0=skip, 1-14=repeat, 15=infinite.
    pub rep: u8,
    /// True if start > end (reverse playback).
    pub reverse: bool,
}

/// Compute exactly 4 segments from a marker bank and total sample count.
///
/// MARKER_EMPTY slots resolve to the previous boundary, collapsing that
/// segment to zero-length (equivalent to skip). This is a pure function
/// with no App state dependency, directly testable with proptest.
pub fn segment_bounds(bank: &crate::engine::bext::MarkerBank, total: u32) -> Vec<Segment> {
    use crate::engine::bext::MARKER_EMPTY;

    let markers = [bank.m1, bank.m2, bank.m3];
    let mut boundaries = Vec::with_capacity(5);
    boundaries.push(0u32);

    for &m in &markers {
        let prev = *boundaries.last().unwrap();
        if m == MARKER_EMPTY {
            boundaries.push(prev); // collapse to previous
        } else {
            boundaries.push(m.min(total)); // clamp to file length
        }
    }
    boundaries.push(total);

    (0..4).map(|i| {
        let raw_start = boundaries[i];
        let raw_end = boundaries[i + 1];
        let reverse = raw_start > raw_end;
        Segment {
            start: raw_start,
            end: raw_end,
            rep: bank.reps[i],
            reverse,
        }
    }).collect()
}

/// TUI application state. All transitions are pure functions (no I/O).
pub struct App {
    /// Current search query text.
    pub query: String,
    /// Cursor position within the query string.
    pub cursor_pos: usize,
    /// Search results currently displayed.
    pub results: Vec<TableRow>,
    /// Index of the selected result.
    pub selected: usize,
    /// Scroll offset for the results viewport.
    pub scroll_offset: usize,
    /// Total matches reported by search completion.
    pub total_matches: usize,
    /// Whether a search is currently in progress.
    pub search_in_progress: bool,
    /// Preview data for the currently selected result.
    pub preview: Option<PreviewData>,
    /// Active color theme.
    pub theme: Theme,
    /// Whether the app should exit.
    pub should_quit: bool,
    /// Current input mode (Normal or Insert).
    pub input_mode: InputMode,
    /// Whether a new search has been dispatched but no results have arrived yet.
    /// While true, old results are kept visible to prevent flickering.
    pub search_pending: bool,
    /// Height of the visible results viewport (set by layout).
    pub viewport_height: usize,
    /// Whether the previous key was 'g' (for gg detection).
    pending_g: bool,
    /// Active column list for the metadata table.
    pub columns: Vec<String>,
    /// Whether the query changed since last search dispatch.
    pub query_changed: bool,
    /// Whether the selection changed since last preview dispatch.
    pub selection_changed: bool,
    /// Paths that have been played back (session-only, for played styling).
    pub played: HashSet<std::path::PathBuf>,
    /// Audio playback engine (None if no audio device).
    pub playback: Option<PlaybackEngine>,
    /// Mark store for file marking (None if no store available).
    pub marks: Option<Box<dyn MarkStore>>,
    /// Small seek increment in seconds (resolved from config).
    pub scrub_small: f64,
    /// Large seek increment in seconds (resolved from config).
    pub scrub_large: f64,
    /// Auto-advance to next sample when playback finishes naturally.
    pub auto_advance: bool,
    /// Whether the last tick saw Playing state (for natural completion detection).
    pub was_playing: bool,
    /// Show remaining time instead of elapsed in status bar.
    pub show_remaining: bool,
    /// Whether to show only marked files.
    pub show_marked_only: bool,
    /// Index of the selected column (for h/l navigation and sorting).
    pub selected_column: usize,
    /// Column currently used for sorting (None = unsorted).
    pub sort_column: Option<String>,
    /// Sort direction: true = ascending, false = descending.
    pub sort_ascending: bool,
    /// Normal mode keymap (key → action bindings).
    pub keymap: actions::Keymap,
    /// Whether the help overlay is currently shown.
    pub show_help: bool,
    /// Active marker bank for editing (A or B).
    pub active_bank: Bank,
    /// Segment playback: end sample boundary (stop playback when reached).
    pub segment_end: Option<u32>,
    /// Segment playback: start sample (for rep looping).
    pub segment_start: Option<u32>,
    /// Segment playback: remaining repetitions for current segment.
    pub segment_reps_remaining: u8,
    /// Program playback: ordered list of (start, end, reps) to play.
    pub program_playlist: Vec<(u32, u32, u8)>,
    /// Program playback: index into program_playlist.
    pub program_index: usize,
    /// Transient status message shown in the status bar.
    pub status_message: Option<String>,
    /// Timestamp when the status message was set (for auto-clear).
    pub status_message_time: Option<std::time::Instant>,
}

impl App {
    /// Create a new App with default state.
    pub fn new(theme: Theme) -> Self {
        // Try to initialize audio playback (may fail in CI/headless).
        let playback = PlaybackEngine::try_new().ok();

        Self {
            query: String::new(),
            cursor_pos: 0,
            results: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            total_matches: 0,
            search_in_progress: false,
            preview: None,
            theme,
            should_quit: false,
            input_mode: InputMode::Normal,
            search_pending: false,
            viewport_height: PAGE_SIZE,
            columns: crate::engine::config::default_columns(),
            pending_g: false,
            query_changed: false,
            selection_changed: false,
            played: HashSet::new(),
            playback,
            marks: None,
            scrub_small: 0.1,
            scrub_large: 1.0,
            auto_advance: false,
            was_playing: false,
            show_remaining: false,
            show_marked_only: false,
            selected_column: 0,
            sort_column: None,
            sort_ascending: true,
            keymap: actions::Keymap::default(),
            show_help: false,
            active_bank: Bank::A,
            segment_end: None,
            segment_start: None,
            segment_reps_remaining: 0,
            program_playlist: Vec::new(),
            program_index: 0,
            status_message: None,
            status_message_time: None,
        }
    }

    /// Execute an action, updating state accordingly.
    pub fn dispatch(&mut self, action: actions::Action) {
        use actions::Action;
        match action {
            Action::MoveDown => self.move_selection(1),
            Action::MoveUp => self.move_selection(-1),
            Action::MoveToTop => self.jump_top(),
            Action::MoveToBottom => self.jump_bottom(),
            Action::PageDown => self.page_down(),
            Action::PageUp => self.page_up(),
            Action::MoveColumnLeft => self.move_column(-1),
            Action::MoveColumnRight => self.move_column(1),
            Action::SortAscending => self.sort_by_selected_column(true),
            Action::SortDescending => self.sort_by_selected_column(false),
            Action::TogglePlayback => self.toggle_playback(),
            // StopPlayback removed — space toggles pause, no separate stop needed.
            Action::SeekForwardSmall => self.seek_relative(self.scrub_small),
            Action::SeekForwardLarge => self.seek_relative(self.scrub_large),
            Action::SeekBackwardSmall => self.seek_relative(-self.scrub_small),
            Action::SeekBackwardLarge => self.seek_relative(-self.scrub_large),
            Action::ToggleAutoAdvance => self.auto_advance = !self.auto_advance,
            Action::ToggleTimeDisplay => self.show_remaining = !self.show_remaining,
            Action::ToggleMark => self.toggle_mark(),
            Action::ClearMarks => self.clear_all_marks(),
            Action::ToggleMarkedFilter => self.toggle_marked_filter(),
            Action::SaveMarkers => self.save_markers(),
            Action::ToggleBank => self.toggle_bank(),
            Action::SetMarker1 => self.set_marker(0),
            Action::SetMarker2 => self.set_marker(1),
            Action::SetMarker3 => self.set_marker(2),
            Action::ClearNearestMarker => self.clear_nearest_marker(),
            Action::ClearBankMarkers => self.clear_bank_markers(),
            Action::IncrementRep => self.adjust_rep(1),
            Action::DecrementRep => self.adjust_rep(-1),
            Action::PlaySegment => self.play_segment(),
            Action::PlayProgram => self.play_program(),
            Action::EnterInsertMode => self.enter_insert_mode(),
            Action::EnterNormalMode => self.enter_normal_mode(),
            Action::SearchSubmit => {
                self.query_changed = true;
                self.enter_normal_mode();
            }
            Action::ClearQuery => {
                self.query.clear();
                self.cursor_pos = 0;
                self.query_changed = true;
            }
            Action::OpenSelected => self.open_selected(),
            Action::ShowHelp => self.show_help = !self.show_help,
            Action::Quit => self.should_quit = true,
        }
    }

    /// Handle a key event, updating state accordingly.
    pub fn on_key(&mut self, key: KeyEvent) {
        // When help overlay is shown, only ? and Esc dismiss it.
        if self.show_help {
            match key.code {
                KeyCode::Esc => self.show_help = false,
                _ => {
                    // Check if key maps to ShowHelp (i.e. ? toggle).
                    if let Some(actions::Action::ShowHelp) = self.keymap.resolve(key) {
                        self.show_help = false;
                    }
                }
            }
            return;
        }

        // Ctrl+C: exit Insert mode, or quit from Normal mode.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.pending_g = false;
            if self.input_mode == InputMode::Insert {
                self.dispatch(actions::Action::EnterNormalMode);
            } else {
                self.dispatch(actions::Action::Quit);
            }
            return;
        }

        match self.input_mode {
            InputMode::Normal => self.on_key_normal(key),
            InputMode::Insert => self.on_key_insert(key),
        }
    }

    /// Resolve Normal mode key to Action via keymap, then dispatch.
    fn on_key_normal(&mut self, key: KeyEvent) {
        // Handle gg sequence: first g sets pending, second g dispatches MoveToTop.
        // If 'g' is explicitly bound in the keymap, skip the gg sequence and dispatch directly.
        if key.code == KeyCode::Char('g') && key.modifiers.is_empty() {
            if self.keymap.resolve(key).is_some() {
                // 'g' is explicitly bound — dispatch it directly, no gg sequence.
                self.pending_g = false;
            } else if self.pending_g {
                self.pending_g = false;
                self.dispatch(actions::Action::MoveToTop);
                return;
            } else {
                self.pending_g = true;
                return;
            }
        }

        // Any other key cancels pending g.
        self.pending_g = false;

        // Esc is context-dependent: clear query if non-empty, else quit.
        if key.code == KeyCode::Esc {
            if !self.query.is_empty() {
                self.dispatch(actions::Action::ClearQuery);
            } else {
                self.dispatch(actions::Action::Quit);
            }
            return;
        }

        // Look up key in configurable keymap.
        if let Some(action) = self.keymap.resolve(key) {
            self.dispatch(action);
        }
    }

    /// Handle key events in Insert mode (search field input).
    fn on_key_insert(&mut self, key: KeyEvent) {
        self.pending_g = false;
        match key.code {
            KeyCode::Esc => {
                self.enter_normal_mode();
            }
            KeyCode::Enter => {
                // Confirm search and return to Normal mode.
                self.query_changed = true;
                self.enter_normal_mode();
            }
            KeyCode::Backspace => {
                if self.cursor_pos > 0 {
                    self.query.remove(self.cursor_pos - 1);
                    self.cursor_pos -= 1;
                    self.query_changed = true;
                } else {
                    // Backspace on empty query returns to Normal mode.
                    self.enter_normal_mode();
                }
            }
            KeyCode::Down => {
                // Navigate results without leaving Insert mode.
                self.move_selection(1);
            }
            KeyCode::Up => {
                // Navigate results without leaving Insert mode.
                self.move_selection(-1);
            }
            KeyCode::Char(c) => {
                self.query.insert(self.cursor_pos, c);
                self.cursor_pos += 1;
                self.query_changed = true;
            }
            _ => {}
        }
    }

    /// Handle incoming search results.
    pub fn on_search_results(&mut self, results: Vec<TableRow>) {
        if self.search_pending {
            // First batch of a new search: replace old results.
            self.results.clear();
            self.selected = 0;
            self.scroll_offset = 0;
            self.preview = None;
            self.selection_changed = true;
            self.search_pending = false;
        } else if self.results.is_empty() {
            // First batch overall (initial load): reset selection.
            self.selected = 0;
            self.scroll_offset = 0;
            self.preview = None;
            self.selection_changed = true;
        }
        self.results.extend(results);
    }

    /// Handle search completion.
    pub fn on_search_complete(&mut self, total: usize) {
        self.total_matches = total;
        self.search_in_progress = false;
    }

    /// Handle preview data arrival.
    pub fn on_preview_ready(&mut self, data: PreviewData) {
        self.preview = Some(data);
    }

    /// Move selection by delta (positive = down, negative = up).
    pub fn move_selection(&mut self, delta: isize) {
        if self.results.is_empty() {
            return;
        }
        let max = self.results.len() - 1;
        let new_sel = if delta >= 0 {
            (self.selected + delta as usize).min(max)
        } else {
            self.selected.saturating_sub((-delta) as usize)
        };
        if new_sel != self.selected {
            self.selected = new_sel;
            self.selection_changed = true;
            self.adjust_scroll();
            // Stop playback when selection changes.
            self.stop_playback();
        }
    }

    /// Page down by viewport height.
    pub fn page_down(&mut self) {
        self.move_selection(self.viewport_height as isize);
    }

    /// Page up by viewport height.
    pub fn page_up(&mut self) {
        self.move_selection(-(self.viewport_height as isize));
    }

    /// Jump to the first result.
    pub fn jump_top(&mut self) {
        if self.selected != 0 {
            self.selected = 0;
            self.selection_changed = true;
            self.adjust_scroll();
        }
    }

    /// Jump to the last result.
    pub fn jump_bottom(&mut self) {
        if !self.results.is_empty() {
            let last = self.results.len() - 1;
            if self.selected != last {
                self.selected = last;
                self.selection_changed = true;
                self.adjust_scroll();
            }
        }
    }

    /// Adjust scroll_offset to keep selected within the visible viewport.
    fn adjust_scroll(&mut self) {
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + self.viewport_height {
            self.scroll_offset = self.selected - self.viewport_height + 1;
        }
    }

    /// Open the selected file in macOS Finder.
    fn open_selected(&self) {
        if let Some(row) = self.results.get(self.selected) {
            let _ = std::process::Command::new("open")
                .arg("-R")
                .arg(&row.meta.path)
                .spawn();
        }
    }

    /// Toggle playback: play program if stopped, else toggle pause.
    /// All playback goes through the segment system.
    fn toggle_playback(&mut self) {
        let state = self.playback_state();
        match state {
            PlaybackState::Stopped => {
                self.play_program();
            }
            PlaybackState::Playing | PlaybackState::Paused => {
                if let Some(ref engine) = self.playback {
                    engine.toggle_pause();
                }
            }
        }
    }

    /// Stop playback (manual stop — clears was_playing to prevent auto-advance).
    fn stop_playback(&mut self) {
        if let Some(ref engine) = self.playback {
            engine.stop();
        }
        self.was_playing = false;
        self.segment_end = None;
        self.segment_start = None;
        self.segment_reps_remaining = 0;
        self.program_playlist.clear();
        self.program_index = 0;
    }

    /// Playback position as fraction 0.0–1.0 (derived from engine sample offset).
    pub fn playback_position(&self) -> f32 {
        self.playback
            .as_ref()
            .map(|e| e.position_fraction())
            .unwrap_or(0.0)
    }

    /// Update playback position from elapsed time. Called on each tick.
    ///
    /// Also detects natural playback completion for auto-advance mode.
    pub fn update_playback_position(&mut self) {
        let engine = match &self.playback {
            Some(e) => e,
            None => return,
        };
        let state = engine.state();
        match state {
            PlaybackState::Playing => {
                engine.update_sample_offset();
                self.was_playing = true;

                // Segment boundary enforcement.
                if let Some(seg_end) = self.segment_end {
                    let current = engine.sample_offset();
                    if current >= seg_end {
                        if self.segment_reps_remaining == u8::MAX {
                            // Infinite loop: always seek back, never decrement.
                            if let Some(start) = self.segment_start {
                                let _ = engine.seek_to_sample(start);
                            }
                        } else if self.segment_reps_remaining > 0 {
                            // Finite loop: decrement and seek back.
                            self.segment_reps_remaining -= 1;
                            if let Some(start) = self.segment_start {
                                let _ = engine.seek_to_sample(start);
                            }
                        } else if !self.program_playlist.is_empty() {
                            // Program mode: advance to next segment.
                            self.program_index += 1;
                            self.start_program_segment();
                        } else {
                            // Single segment: stop.
                            engine.stop();
                            self.segment_end = None;
                            self.segment_start = None;
                            self.was_playing = false;
                        }
                    }
                }
            }
            PlaybackState::Stopped => {
                if self.auto_advance && self.was_playing {
                    // Natural completion: advance to next sample.
                    self.was_playing = false;
                    if self.selected + 1 < self.results.len() {
                        self.selected += 1;
                        self.selection_changed = true;
                        self.adjust_scroll();
                        // Play the next sample.
                        self.toggle_playback();
                    }
                } else {
                    self.was_playing = false;
                }
            }
            PlaybackState::Paused => {
                // Keep position frozen, don't change was_playing.
            }
        }
    }

    /// Seek relative to current position (seconds). No-op when stopped or no engine.
    fn seek_relative(&self, delta: f64) {
        if let Some(ref engine) = self.playback {
            let _ = engine.seek_relative(delta);
        }
    }

    /// Get current playback state (convenience).
    pub fn playback_state(&self) -> PlaybackState {
        self.playback
            .as_ref()
            .map(|e| e.state())
            .unwrap_or(PlaybackState::Stopped)
    }

    /// Get the filename of the currently playing file.
    pub fn playback_filename(&self) -> Option<String> {
        self.playback.as_ref().and_then(|e| {
            e.current_path()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        })
    }

    /// Toggle mark on the currently selected file.
    fn toggle_mark(&mut self) {
        if let Some(row) = self.results.get_mut(self.selected) {
            let new_marked = !row.marked;
            row.marked = new_marked;
            if let Some(ref store) = self.marks {
                if new_marked {
                    let _ = store.mark(&row.meta.path);
                } else {
                    let _ = store.unmark(&row.meta.path);
                }
            }
        }
    }

    /// Clear all marks.
    fn clear_all_marks(&mut self) {
        if let Some(ref store) = self.marks {
            let _ = store.clear_all();
        }
        for row in &mut self.results {
            row.marked = false;
        }
    }

    /// Toggle between showing all results and marked-only.
    fn toggle_marked_filter(&mut self) {
        self.show_marked_only = !self.show_marked_only;
    }

    /// Set a transient status message (auto-clears after ~2s).
    fn set_status(&mut self, msg: String) {
        self.status_message = Some(msg);
        self.status_message_time = Some(std::time::Instant::now());
    }

    /// Get current markers from preview/row, or generate duration-aware defaults.
    pub fn current_markers_or_default(&self) -> crate::engine::bext::MarkerConfig {
        // 1. Check preview.
        if let Some(ref preview) = self.preview {
            if let Some(m) = preview.markers {
                return m;
            }
        }
        // 2. Check selected row.
        if let Some(row) = self.results.get(self.selected) {
            if let Some(m) = row.markers {
                return m;
            }
        }
        // 3. Duration-aware default: short files get shot, longer files get loop.
        let total_samples = self.preview.as_ref()
            .and_then(|p| p.audio_info.as_ref())
            .map(|ai| (ai.duration_secs * ai.sample_rate as f64) as u32);
        match total_samples {
            Some(s) if s >= 2 * 48000 => crate::engine::bext::MarkerConfig::preset_loop(s),
            _ => crate::engine::bext::MarkerConfig::preset_shot(),
        }
    }

    /// Save markers to the BEXT chunk of the currently selected file.
    fn save_markers(&mut self) {
        let row = match self.results.get(self.selected) {
            Some(r) => r,
            None => {
                self.set_status("No file selected".to_string());
                return;
            }
        };

        let path = row.meta.path.clone();
        let markers = self.current_markers_or_default();

        // Try writing to existing packed BEXT first; if not packed, initialize.
        let result = match crate::engine::bext::write_markers(&path, &markers) {
            Ok(()) => Ok(()),
            Err(crate::engine::bext::BextWriteError::NotPacked) => {
                crate::engine::bext::init_packed_and_write_markers(&path, &markers)
            }
            Err(e) => Err(e),
        };

        match result {
            Ok(()) => {
                self.set_status(format!("Markers saved to {}", path.display()));
                // Update preview markers to reflect what was written.
                if let Some(ref mut preview) = self.preview {
                    preview.markers = Some(markers);
                }
                // Update the table row as well.
                if let Some(row) = self.results.get_mut(self.selected) {
                    row.markers = Some(markers);
                }
            }
            Err(e) => {
                self.set_status(format!("Save failed: {e}"));
            }
        }
    }

    /// Toggle active marker bank between A and B.
    fn toggle_bank(&mut self) {
        self.active_bank = match self.active_bank {
            Bank::A => Bank::B,
            Bank::B => Bank::A,
        };
        let label = match self.active_bank {
            Bank::A => "Bank A",
            Bank::B => "Bank B",
        };
        self.set_status(format!("Active: {label}"));
    }

    /// Get a mutable reference to the active bank's MarkerBank within preview markers.
    /// Returns None if no preview or no markers.
    fn active_bank_mut(&mut self) -> Option<&mut crate::engine::bext::MarkerBank> {
        let markers = self.preview.as_mut()?.markers.as_mut()?;
        match self.active_bank {
            Bank::A => Some(&mut markers.bank_a),
            Bank::B => Some(&mut markers.bank_b),
        }
    }

    /// Get the active bank from preview markers (immutable).
    fn active_bank_ref(&self) -> Option<&crate::engine::bext::MarkerBank> {
        let markers = self.preview.as_ref()?.markers.as_ref()?;
        match self.active_bank {
            Bank::A => Some(&markers.bank_a),
            Bank::B => Some(&markers.bank_b),
        }
    }

    /// Compute total samples from preview audio_info, or None.
    fn total_samples(&self) -> Option<u32> {
        self.preview.as_ref()
            .and_then(|p| p.audio_info.as_ref())
            .map(|ai| (ai.duration_secs * ai.sample_rate as f64) as u32)
    }

    /// Get the current playback position as an absolute sample offset.
    fn playback_sample(&self) -> Option<u32> {
        let total = self.total_samples()?;
        let pos = self.playback_position();
        if pos <= 0.0 {
            return None;
        }
        Some(((pos as f64) * total as f64) as u32)
    }

    /// Ensure preview has markers (initialize from defaults if needed).
    fn ensure_markers(&mut self) {
        if let Some(ref mut preview) = self.preview {
            if preview.markers.is_none() {
                let total = preview.audio_info.as_ref()
                    .map(|ai| (ai.duration_secs * ai.sample_rate as f64) as u32);
                preview.markers = Some(match total {
                    Some(s) if s >= 2 * 48000 => {
                        crate::engine::bext::MarkerConfig::preset_loop(s)
                    }
                    _ => crate::engine::bext::MarkerConfig::preset_shot(),
                });
            }
        }
    }

    /// Set marker at index (0=m1, 1=m2, 2=m3) to current playback position.
    fn set_marker(&mut self, index: usize) {
        let sample = match self.playback_sample() {
            Some(s) => s,
            None => {
                self.set_status("Play file first".to_string());
                return;
            }
        };

        self.ensure_markers();
        if let Some(bank) = self.active_bank_mut() {
            match index {
                0 => bank.m1 = sample,
                1 => bank.m2 = sample,
                2 => bank.m3 = sample,
                _ => return,
            }
            // No sorting — marker slot order encodes playback direction per
            // MARKERSv2 spec: m_i > m_{i+1} means reverse playback.
        }

        let bank_label = match self.active_bank { Bank::A => "A", Bank::B => "B" };
        self.set_status(format!("Marker {} set (bank {bank_label})", index + 1));
    }

    /// Clear the marker nearest to the playback cursor in the active bank.
    fn clear_nearest_marker(&mut self) {
        let sample = match self.playback_sample() {
            Some(s) => s,
            None => {
                self.set_status("Play file first".to_string());
                return;
            }
        };

        if let Some(bank) = self.active_bank_mut() {
            let markers = [bank.m1, bank.m2, bank.m3];
            let nearest = markers.iter().enumerate()
                .filter(|&(_, &v)| v != crate::engine::bext::MARKER_EMPTY)
                .min_by_key(|&(_, &v)| (v as i64 - sample as i64).unsigned_abs());

            if let Some((idx, _)) = nearest {
                match idx {
                    0 => bank.m1 = crate::engine::bext::MARKER_EMPTY,
                    1 => bank.m2 = crate::engine::bext::MARKER_EMPTY,
                    2 => bank.m3 = crate::engine::bext::MARKER_EMPTY,
                    _ => {}
                }
                let bank_label = match self.active_bank { Bank::A => "A", Bank::B => "B" };
                self.set_status(format!("Marker {} cleared (bank {bank_label})", idx + 1));
            } else {
                self.set_status("No markers to clear".to_string());
            }
        } else {
            self.set_status("No markers".to_string());
        }
    }

    /// Clear all markers in the active bank.
    fn clear_bank_markers(&mut self) {
        if let Some(bank) = self.active_bank_mut() {
            *bank = crate::engine::bext::MarkerBank::empty();
            let bank_label = match self.active_bank { Bank::A => "A", Bank::B => "B" };
            self.set_status(format!("Bank {bank_label} cleared"));
        } else {
            self.set_status("No markers".to_string());
        }
    }

    /// Determine which segment (0-3) the playback cursor is in.
    /// Uses the active bank's segment boundaries.
    fn current_segment_index(&self) -> Option<usize> {
        let sample = self.playback_sample()?;
        let segs = self.segments();
        if segs.is_empty() {
            return None;
        }
        for (i, seg) in segs.iter().enumerate() {
            let (lo, hi) = if seg.reverse {
                (seg.end, seg.start)
            } else {
                (seg.start, seg.end)
            };
            if sample >= lo && sample < hi {
                return Some(i);
            }
        }
        // Past the last boundary — return last segment.
        Some(3)
    }

    /// Adjust repetition count for the segment containing the playback cursor.
    /// When stopped, defaults to segment 0.
    fn adjust_rep(&mut self, delta: i8) {
        let seg = match self.current_segment_index() {
            Some(s) if s < 4 => s,
            None => {
                // Default to segment 0 when stopped.
                if self.segments().is_empty() {
                    self.set_status("No segments".to_string());
                    return;
                }
                0
            }
            _ => {
                self.set_status("No active segment".to_string());
                return;
            }
        };

        self.ensure_markers();
        if let Some(bank) = self.active_bank_mut() {
            let current = bank.reps[seg];
            let new_val = (current as i16 + delta as i16).clamp(0, 15) as u8;
            bank.reps[seg] = new_val;
            let label = if new_val == 15 { "inf".to_string() } else { format!("{new_val}") };
            self.set_status(format!("Segment {} rep: {label}", seg + 1));
        } else {
            self.set_status("No markers".to_string());
        }
    }

    /// Get segments for the active bank. Always returns exactly 4 segments,
    /// or empty if no preview/audio_info/markers.
    fn segments(&self) -> Vec<Segment> {
        let total = match self.total_samples() {
            Some(t) if t > 0 => t,
            _ => return Vec::new(),
        };
        let bank = match self.active_bank_ref() {
            Some(b) => b,
            None => return Vec::new(),
        };
        segment_bounds(bank, total)
    }

    /// Play the current segment (bounded by markers around playback cursor).
    /// When stopped, defaults to segment 0 and auto-starts playback.
    fn play_segment(&mut self) {
        self.ensure_markers();
        let seg_idx = match self.current_segment_index() {
            Some(s) => s,
            None => {
                // Default to segment 0 when stopped or no position.
                if self.segments().is_empty() {
                    self.set_status("No segments".to_string());
                    return;
                }
                0
            }
        };

        let segs = self.segments();
        if seg_idx >= segs.len() {
            self.set_status("No active segment".to_string());
            return;
        }

        let seg = segs[seg_idx];

        if seg.reverse {
            self.set_status(format!("Segment {}: reverse not yet supported", seg_idx + 1));
            return;
        }

        // Start playback from segment start.
        if let Some(row) = self.results.get(self.selected) {
            let path = row.meta.path.clone();
            if let Some(ref engine) = self.playback {
                if engine.state() == PlaybackState::Stopped {
                    let _ = engine.play(&path);
                    self.played.insert(path);
                }
                let _ = engine.seek_to_sample(seg.start);
            }
        }

        // Store segment boundaries for tick-based enforcement.
        self.segment_start = Some(seg.start);
        self.segment_end = Some(seg.end);
        self.segment_reps_remaining = if seg.rep == 15 {
            u8::MAX // infinite loop sentinel
        } else if seg.rep == 0 {
            0
        } else {
            seg.rep - 1
        };

        self.set_status(format!("Segment {}", seg_idx + 1));
    }

    /// Play full program: all non-skipped, non-reverse segments with repetitions.
    fn play_program(&mut self) {
        self.ensure_markers();
        let segs = self.segments();
        if segs.is_empty() {
            self.set_status("No segments".to_string());
            return;
        }

        // Build playlist: (start, end, reps) for non-skipped, non-reverse segments.
        let playlist: Vec<(u32, u32, u8)> = segs.iter()
            .filter(|seg| seg.rep > 0 && !seg.reverse && seg.start != seg.end)
            .map(|seg| (seg.start, seg.end, seg.rep))
            .collect();

        if playlist.is_empty() {
            self.set_status("All segments skipped".to_string());
            return;
        }

        self.program_playlist = playlist;
        self.program_index = 0;

        // Start playing the first segment.
        self.start_program_segment();
    }

    /// Start playing the current segment in the program playlist.
    fn start_program_segment(&mut self) {
        if self.program_index >= self.program_playlist.len() {
            self.program_playlist.clear();
            self.segment_end = None;
            self.segment_start = None;
            self.set_status("Program complete".to_string());
            return;
        }

        let (start, end, reps) = self.program_playlist[self.program_index];

        if let Some(row) = self.results.get(self.selected) {
            let path = row.meta.path.clone();
            if let Some(ref engine) = self.playback {
                if engine.state() == PlaybackState::Stopped {
                    let _ = engine.play(&path);
                    self.played.insert(path);
                }
                let _ = engine.seek_to_sample(start);
            }
        }

        self.segment_start = Some(start);
        self.segment_end = Some(end);
        self.segment_reps_remaining = if reps == 15 {
            u8::MAX // infinite loop sentinel
        } else {
            reps.saturating_sub(1)
        };

        let seg_num = self.program_index + 1;
        let total_segs = self.program_playlist.len();
        self.set_status(format!("Program {seg_num}/{total_segs} rep {reps}"));
    }

    /// Sort results by the currently selected column.
    pub fn sort_by_selected_column(&mut self, ascending: bool) {
        if self.results.is_empty() || self.selected_column >= self.columns.len() {
            return;
        }
        let key = self.columns[self.selected_column].clone();
        self.sort_ascending = ascending;
        self.sort_column = Some(key.clone());

        self.results.sort_by(|a, b| {
            let ka = column_sort_key(a, &key);
            let kb = column_sort_key(b, &key);
            if ascending { ka.cmp(&kb) } else { kb.cmp(&ka) }
        });

        // Reset selection to top after sort.
        self.selected = 0;
        self.scroll_offset = 0;
        self.selection_changed = true;
    }

    /// Move column selection by delta with wrapping.
    pub fn move_column(&mut self, delta: isize) {
        let len = self.columns.len();
        if len == 0 {
            return;
        }
        let new = (self.selected_column as isize + delta).rem_euclid(len as isize) as usize;
        self.selected_column = new;
    }

    /// Switch to Insert mode (search field active).
    pub fn enter_insert_mode(&mut self) {
        self.input_mode = InputMode::Insert;
    }

    /// Switch to Normal mode (navigation keys active).
    pub fn enter_normal_mode(&mut self) {
        self.input_mode = InputMode::Normal;
    }

    /// Get the total number of marked files (from store if available, else from results).
    pub fn mark_count(&self) -> usize {
        if let Some(ref store) = self.marks {
            store.mark_count()
        } else {
            self.results.iter().filter(|r| r.marked).count()
        }
    }

}

/// Sort key for column-based sorting.
///
/// Numeric values sort numerically; text sorts case-insensitive.
/// Empty/"-" values sort after all non-empty values.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SortKey {
    /// Numeric sort value.
    Numeric(i64),
    /// Text sort value (lowercase).
    Text(String),
    /// Empty/missing value (sorts last).
    None,
}

impl Ord for SortKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (SortKey::None, SortKey::None) => std::cmp::Ordering::Equal,
            (SortKey::None, _) => std::cmp::Ordering::Greater,
            (_, SortKey::None) => std::cmp::Ordering::Less,
            (SortKey::Numeric(a), SortKey::Numeric(b)) => a.cmp(b),
            (SortKey::Text(a), SortKey::Text(b)) => a.cmp(b),
            // Mixed: numeric before text.
            (SortKey::Numeric(_), SortKey::Text(_)) => std::cmp::Ordering::Less,
            (SortKey::Text(_), SortKey::Numeric(_)) => std::cmp::Ordering::Greater,
        }
    }
}

impl PartialOrd for SortKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Numeric column keys for sort key extraction.
const SORT_NUMERIC_COLUMNS: &[&str] = &[
    "bpm", "sample_rate", "bit_depth", "channels",
];

/// Extract a sort key from a TableRow for a given column.
fn column_sort_key(row: &TableRow, key: &str) -> SortKey {
    let value = widgets::column_value(row, key);
    if value.is_empty() || value == "-" {
        return SortKey::None;
    }
    if SORT_NUMERIC_COLUMNS.contains(&key) {
        // Try parsing numeric value.
        if let Ok(n) = value.parse::<i64>() {
            return SortKey::Numeric(n);
        }
    }
    // Duration: parse "M:SS" → total seconds.
    if key == "duration" {
        if let Some(secs) = parse_duration_sort(&value) {
            return SortKey::Numeric(secs);
        }
    }
    // Sample rate "48k" → numeric.
    if key == "sample_rate" {
        if let Some(stripped) = value.strip_suffix('k') {
            if let Ok(n) = stripped.parse::<i64>() {
                return SortKey::Numeric(n * 1000);
            }
        }
    }
    SortKey::Text(value.to_ascii_lowercase())
}

/// Parse "M:SS" or "H:MM:SS" to total seconds for sorting.
fn parse_duration_sort(s: &str) -> Option<i64> {
    let parts: Vec<&str> = s.split(':').collect();
    match parts.len() {
        2 => {
            let m = parts[0].parse::<i64>().ok()?;
            let s = parts[1].parse::<i64>().ok()?;
            Some(m * 60 + s)
        }
        3 => {
            let h = parts[0].parse::<i64>().ok()?;
            let m = parts[1].parse::<i64>().ok()?;
            let s = parts[2].parse::<i64>().ok()?;
            Some(h * 3600 + m * 60 + s)
        }
        _ => None,
    }
}

// --- Theme resolution ---

/// Resolve the TUI theme from CLI and config settings.
///
/// Priority: CLI flag > config file > default (telescope).
fn resolve_theme(cli_theme: Option<&str>, config_theme: Option<&str>) -> Theme {
    let name = cli_theme.or(config_theme);
    match name {
        Some(n) => match Theme::by_name(n) {
            Ok(t) => t,
            Err(_) => {
                eprintln!("riffgrep: warning: unknown theme '{n}', using default");
                Theme::default()
            }
        },
        None => Theme::default(),
    }
}

// --- Event loop and terminal lifecycle ---

use std::io;
use std::path::PathBuf;

use crossterm::event::{Event, EventStream};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, execute};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Terminal;
use tokio::time::{Duration, Instant};

use search::{SearchHandleTable, SearchMode};

/// Waveform panel height: 16 braille rows + 1 transport info row.
const WAVEFORM_ROWS: u16 = 17;

/// Draw the 2-panel layout: search prompt | metadata table | waveform | status bar.
fn draw(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &App) -> io::Result<()> {
    terminal.draw(|frame| {
        let size = frame.area();

        // Vertical split: search prompt (3) | table (fill) | waveform (9) | status bar (1).
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(4),
                Constraint::Length(WAVEFORM_ROWS),
                Constraint::Length(1),
            ])
            .split(size);

        widgets::render_search_prompt(app, chunks[0], frame.buffer_mut());
        widgets::render_metadata_table(app, chunks[1], frame.buffer_mut(), &app.columns);
        widgets::render_waveform_panel(app, chunks[2], frame.buffer_mut());
        widgets::render_status_bar(app, chunks[3], frame.buffer_mut());

        // Help overlay rendered on top of everything when active.
        if app.show_help {
            widgets::render_help_overlay(app, size, frame.buffer_mut());
        }
    })?;
    Ok(())
}

/// Determine the search mode from CLI opts.
fn resolve_search_mode(opts: &crate::engine::cli::Opts) -> anyhow::Result<SearchMode> {
    if opts.no_db {
        let roots = if opts.paths.is_empty() {
            vec![std::env::current_dir()?]
        } else {
            opts.paths.clone()
        };
        return Ok(SearchMode::Filesystem {
            roots,
            threads: opts.threads,
        });
    }

    let db_path = crate::engine::sqlite::resolve_db_path(opts.db_path.as_deref())?;
    if db_path.exists() {
        Ok(SearchMode::Sqlite(db_path))
    } else {
        let roots = if opts.paths.is_empty() {
            vec![std::env::current_dir()?]
        } else {
            opts.paths.clone()
        };
        Ok(SearchMode::Filesystem {
            roots,
            threads: opts.threads,
        })
    }
}

/// Get the DB path if we're in SQLite mode (for peak loading).
fn resolve_db_path_for_peaks(opts: &crate::engine::cli::Opts) -> Option<PathBuf> {
    if opts.no_db {
        return None;
    }
    let db_path = crate::engine::sqlite::resolve_db_path(opts.db_path.as_deref()).ok()?;
    if db_path.exists() { Some(db_path) } else { None }
}

/// Run the interactive TUI.
pub async fn run_tui(opts: crate::engine::cli::Opts) -> anyhow::Result<()> {
    use futures::StreamExt;

    // Load config early so theme resolution can use it.
    let config = crate::engine::config::load_config();
    let theme = resolve_theme(opts.theme.as_deref(), config.theme.as_deref());

    let search_mode = resolve_search_mode(&opts)?;
    let db_path_for_peaks = resolve_db_path_for_peaks(&opts);

    // Setup terminal.
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, cursor::Hide)?;

    // Set panic hook to restore terminal.
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, cursor::Show);
        original_hook(info);
    }));

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(theme);
    if let Some(ref cols) = config.columns {
        if !cols.is_empty() {
            app.columns = cols.clone();
        }
    }

    // Apply default sort from config.
    if let Some(ref sort_col) = config.default_sort {
        if crate::engine::config::column_def(sort_col).is_some() {
            app.sort_column = Some(sort_col.clone());
            app.sort_ascending = config
                .default_sort_order
                .as_deref()
                .map(|o| o != "desc")
                .unwrap_or(true);
        }
    }

    // Apply keymap overrides from config.
    if let Some(ref keymap_overrides) = config.keymap {
        app.keymap = actions::Keymap::with_overrides(keymap_overrides);
    }

    // Apply scrub config.
    let (scrub_small, scrub_large) =
        crate::engine::config::resolve_scrub_increments(config.scrub.as_ref());
    app.scrub_small = scrub_small;
    app.scrub_large = scrub_large;
    app.auto_advance = config
        .scrub
        .as_ref()
        .and_then(|s| s.auto_advance)
        .unwrap_or(false);

    // Initialize marks store based on search mode.
    let marks_store: Box<dyn MarkStore> = if let Some(ref db_path) = db_path_for_peaks {
        Box::new(crate::engine::marks::SqliteMarkStore::new(db_path.clone()))
    } else {
        let marks_path = crate::engine::config::resolve_marks_path(&config);
        Box::new(crate::engine::marks::CsvMarkStore::new(marks_path))
    };
    app.marks = Some(marks_store);

    // Initial search: empty query returns all.
    let initial_query = crate::engine::SearchQuery::default();
    let mut current_search: Option<SearchHandleTable> = Some(SearchHandleTable::spawn(initial_query, search_mode));
    app.search_in_progress = true;

    let mut event_reader = EventStream::new();
    let tick_rate = Duration::from_millis(50);
    let debounce_duration = Duration::from_millis(150);

    let mut search_debounce: Option<Instant> = None;
    let mut preview_debounce: Option<Instant> = None;

    // Initial draw.
    draw(&mut terminal, &app)?;

    loop {
        // Calculate next wake time for debounce timers.
        let next_wake = [search_debounce, preview_debounce]
            .iter()
            .filter_map(|&t| t)
            .min()
            .unwrap_or_else(|| Instant::now() + tick_rate);

        let sleep = tokio::time::sleep_until(next_wake);

        tokio::select! {
            // Crossterm events.
            event = event_reader.next() => {
                match event {
                    Some(Ok(Event::Key(key))) => {
                        app.on_key(key);
                        if app.should_quit {
                            break;
                        }
                    }
                    Some(Ok(Event::Resize(_, _))) => {
                        // Terminal will redraw on next tick.
                    }
                    _ => {}
                }
            }

            // Search results.
            result = async {
                if let Some(ref mut handle) = current_search {
                    handle.results_rx.recv().await
                } else {
                    // Park forever if no search.
                    std::future::pending().await
                }
            } => {
                match result {
                    Some(row) => {
                        app.on_search_results(vec![row]);
                    }
                    None => {
                        // Channel closed — search complete.
                        let total = app.results.len();
                        app.on_search_complete(total);
                        current_search = None;
                    }
                }
            }

            // Timer for debounce.
            _ = sleep => {}
        }

        // Check if search debounce has fired.
        if app.query_changed {
            app.query_changed = false;
            search_debounce = Some(Instant::now() + debounce_duration);
            preview_debounce = None;
        }

        if let Some(fire_at) = search_debounce {
            if Instant::now() >= fire_at {
                search_debounce = None;

                // Cancel existing search.
                if let Some(handle) = current_search.take() {
                    handle.cancel();
                }
                // Don't clear results yet — keep them visible until the
                // first batch of new results arrives (prevents flickering).
                app.search_pending = true;
                app.search_in_progress = true;

                // Build new search: parse @field=value filters from query.
                let (freetext, column_filters) =
                    crate::engine::parse_column_filters(&app.query);
                let query = crate::engine::SearchQuery {
                    freetext: if freetext.is_empty() {
                        None
                    } else {
                        Some(freetext)
                    },
                    column_filters,
                    ..Default::default()
                };

                let mode = resolve_search_mode(&opts)?;
                current_search = Some(SearchHandleTable::spawn(query, mode));
            }
        }

        // Check if selection changed for preview debounce.
        if app.selection_changed {
            app.selection_changed = false;
            preview_debounce = Some(Instant::now() + debounce_duration);
        }

        if let Some(fire_at) = preview_debounce {
            if Instant::now() >= fire_at {
                preview_debounce = None;

                // Load peaks and audio info for the selected item.
                if let Some(row) = app.results.get(app.selected).cloned() {
                    let peaks = search::load_peaks_with_fallback(
                        db_path_for_peaks.as_deref(),
                        &row.meta.path,
                    )
                    .await;
                    // Use audio_info from TableRow if available, otherwise JIT load.
                    let audio_info = if row.audio_info.is_some() {
                        row.audio_info.clone()
                    } else {
                        search::load_audio_info(&row.meta.path).await
                    };
                    app.on_preview_ready(PreviewData {
                        metadata: row.meta,
                        peaks: peaks.unwrap_or_default(),
                        audio_info,
                        markers: row.markers,
                    });
                }
            }
        }

        // Update playback position.
        app.update_playback_position();

        // Update viewport height from terminal size.
        let size = terminal.size()?;
        // Table area = total height - 3 (prompt) - 9 (waveform) - 1 (status bar) - 1 (header row).
        app.viewport_height = size.height.saturating_sub(3 + WAVEFORM_ROWS + 1 + 1) as usize;

        draw(&mut terminal, &app)?;
    }

    // Restore terminal.
    terminal::disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, cursor::Show)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_app_with_results(n: usize) -> App {
        let mut app = App::new(Theme::default());
        app.results = (0..n)
            .map(|i| TableRow {
                meta: UnifiedMetadata {
                    path: std::path::PathBuf::from(format!("/test/{i}.wav")),
                    ..Default::default()
                },
                audio_info: None,
                marked: false,
                markers: None,
            })
            .collect();
        app
    }

    #[test]
    fn test_app_initial_state() {
        let app = App::new(Theme::default());
        assert!(app.query.is_empty());
        assert_eq!(app.cursor_pos, 0);
        assert!(app.results.is_empty());
        assert_eq!(app.selected, 0);
        assert!(!app.should_quit);
        assert!(app.preview.is_none());
    }

    #[test]
    fn test_app_type_char() {
        let mut app = App::new(Theme::default());
        // Enter Insert mode first.
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert_eq!(app.query, "a");
        assert_eq!(app.cursor_pos, 1);
        assert!(app.query_changed);
    }

    #[test]
    fn test_app_type_multiple() {
        let mut app = App::new(Theme::default());
        // Enter Insert mode first.
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        for ch in ['x', 'o', 'o'] {
            app.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        assert_eq!(app.query, "xoo");
        assert_eq!(app.cursor_pos, 3);
    }

    #[test]
    fn test_app_backspace() {
        let mut app = App::new(Theme::default());
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        for ch in ['x', 'o', 'o'] {
            app.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        app.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(app.query, "xo");
        assert_eq!(app.cursor_pos, 2);
    }

    #[test]
    fn test_app_backspace_empty() {
        let mut app = App::new(Theme::default());
        app.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(app.query, "");
        assert_eq!(app.cursor_pos, 0);
    }

    #[test]
    fn test_app_move_down() {
        let mut app = make_app_with_results(10);
        app.move_selection(1);
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn test_app_move_down_clamps() {
        let mut app = make_app_with_results(10);
        app.selected = 9;
        app.move_selection(1);
        assert_eq!(app.selected, 9);
    }

    #[test]
    fn test_app_move_up() {
        let mut app = make_app_with_results(10);
        app.selected = 5;
        app.move_selection(-1);
        assert_eq!(app.selected, 4);
    }

    #[test]
    fn test_app_move_up_clamps() {
        let mut app = make_app_with_results(10);
        app.selected = 0;
        app.move_selection(-1);
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn test_app_jump_top() {
        let mut app = make_app_with_results(100);
        app.selected = 50;
        app.jump_top();
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn test_app_jump_bottom() {
        let mut app = make_app_with_results(100);
        app.jump_bottom();
        assert_eq!(app.selected, 99);
    }

    #[test]
    fn test_app_page_down() {
        let mut app = make_app_with_results(100);
        app.viewport_height = 20;
        app.selected = 5;
        app.page_down();
        assert_eq!(app.selected, 25);
    }

    #[test]
    fn test_app_page_up() {
        let mut app = make_app_with_results(100);
        app.viewport_height = 20;
        app.selected = 50;
        app.page_up();
        assert_eq!(app.selected, 30);
    }

    #[test]
    fn test_app_scroll_follows_selection() {
        let mut app = make_app_with_results(100);
        app.viewport_height = 20;
        app.selected = 25;
        app.adjust_scroll();
        assert!(
            app.scroll_offset <= app.selected
                && app.selected < app.scroll_offset + app.viewport_height,
            "selected {} should be within viewport [{}, {})",
            app.selected,
            app.scroll_offset,
            app.scroll_offset + app.viewport_height
        );
    }

    #[test]
    fn test_app_search_results_reset_selection() {
        let mut app = make_app_with_results(10);
        app.selected = 5;
        // Simulate receiving new results (first batch clears).
        app.results.clear();
        app.on_search_results(vec![TableRow {
            meta: UnifiedMetadata::default(),
            audio_info: None,
            marked: false,
            markers: None,
        }]);
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn test_app_quit_on_q() {
        let mut app = App::new(Theme::default());
        // q quits in Normal mode.
        app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(app.should_quit);
    }

    #[test]
    fn test_app_quit_on_ctrl_c() {
        let mut app = App::new(Theme::default());
        // Ctrl+C quits from Normal mode.
        app.on_key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        ));
        assert!(app.should_quit);
    }

    #[test]
    fn test_app_gg_jumps_to_top() {
        let mut app = make_app_with_results(100);
        app.selected = 50;
        // Two g presses.
        app.on_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        assert!(!app.should_quit);
        app.on_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn test_app_g_then_other_cancels() {
        let mut app = make_app_with_results(100);
        app.selected = 50;
        app.on_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        // g was cancelled, down moved selection.
        assert_eq!(app.selected, 51);
    }

    #[test]
    fn test_app_q_types_in_insert_mode() {
        let mut app = App::new(Theme::default());
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        // In Insert mode, q types 'q', does not quit.
        app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(!app.should_quit);
        assert_eq!(app.query, "q");
    }

    #[test]
    fn test_insert_mode_chars_go_to_query() {
        let mut app = make_app_with_results(10);
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        // In Insert mode, j types 'j', does not navigate.
        app.on_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(app.query, "j");
        assert_eq!(app.selected, 0);
    }

    // --- T9 tests: Playback controls ---

    #[test]
    fn test_app_space_plays_selected() {
        let mut app = make_app_with_results(5);
        // Space on empty query should trigger playback (no crash even without device).
        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        // If no playback engine, state stays Stopped. This test just verifies no panic.
        // If engine exists, it would try to play /test/0.wav (which doesn't exist).
        assert!(!app.should_quit);
    }

    #[test]
    fn test_app_space_types_in_insert_mode() {
        let mut app = make_app_with_results(5);
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        // Space should type ' ' in Insert mode.
        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert_eq!(app.query, "x ");
    }

    #[test]
    fn test_app_s_stops_playback() {
        let mut app = make_app_with_results(5);
        // s on empty query should stop playback (no panic).
        app.on_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(app.playback_position(), 0.0);
        assert!(!app.should_quit);
    }

    #[test]
    fn test_app_s_types_in_insert_mode() {
        let mut app = make_app_with_results(5);
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        // s should type 's' in Insert mode.
        app.on_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(app.query, "xs");
    }

    #[test]
    fn test_app_playback_none_graceful() {
        let mut app = App::new(Theme::default());
        app.playback = None; // Force no audio device.
        app.results = vec![TableRow {
            meta: UnifiedMetadata {
                path: std::path::PathBuf::from("/test/0.wav"),
                ..Default::default()
            },
            audio_info: None,
            marked: false,
            markers: None,
        }];
        // All playback operations should be no-ops with no panic.
        app.toggle_playback();
        app.stop_playback();
        app.update_playback_position();
        assert_eq!(app.playback_state(), PlaybackState::Stopped);
        assert!(app.playback_filename().is_none());
    }

    #[test]
    fn test_app_selection_change_stops_playback() {
        let mut app = make_app_with_results(5);
        // Selection change triggers stop_playback, which calls engine.stop().
        // With no real engine playing, position_fraction() returns 0.0.
        app.move_selection(1);
        assert_eq!(app.playback_position(), 0.0, "selection change should reset position");
    }

    // --- S6-T1 tests: Played file coloring ---

    #[test]
    fn test_played_set_empty_initially() {
        let app = App::new(Theme::default());
        assert!(app.played.is_empty());
    }

    #[test]
    fn test_played_not_populated_on_preview() {
        let mut app = App::new(Theme::default());
        let path = std::path::PathBuf::from("/test/kick.wav");
        app.on_preview_ready(PreviewData {
            metadata: UnifiedMetadata {
                path: path.clone(),
                ..Default::default()
            },
            peaks: vec![],
            audio_info: None,
            markers: None,
        });
        assert!(!app.played.contains(&path), "preview should not add to played set");
    }

    #[test]
    fn test_selected_overrides_played() {
        let mut app = make_app_with_results(3);
        // Mark first item as played.
        app.played.insert(std::path::PathBuf::from("/test/0.wav"));
        app.selected = 0;
        // The style for selected should override played — verified via render test.
        assert!(app.played.contains(&std::path::PathBuf::from("/test/0.wav")));
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn test_marked_overrides_played() {
        let mut app = make_app_with_results(3);
        app.played.insert(std::path::PathBuf::from("/test/0.wav"));
        app.results[0].marked = true;
        // When both played and marked, marked style takes precedence.
        assert!(app.played.contains(&std::path::PathBuf::from("/test/0.wav")));
        assert!(app.results[0].marked);
    }

    #[test]
    fn test_theme_has_table_played_field() {
        let theme = Theme::default();
        // Verify the field exists and has a style set (not default).
        let played_style = theme.table_played;
        assert_ne!(
            played_style,
            ratatui::style::Style::default(),
            "table_played should have a distinct style"
        );
    }

    // --- S5-T7 tests: File Marking/Selection System ---

    #[test]
    fn test_app_m_toggles_mark() {
        let mut app = make_app_with_results(5);
        assert!(!app.results[0].marked, "initially unmarked");
        // Press 'm' to toggle mark (query empty → keybinding active).
        app.on_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        assert!(app.results[0].marked, "should be marked after 'm'");
        // Press 'm' again to unmark.
        app.on_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        assert!(!app.results[0].marked, "should be unmarked after second 'm'");
    }

    #[test]
    fn test_app_clear_all_marks() {
        let mut app = make_app_with_results(5);
        app.results[0].marked = true;
        app.results[2].marked = true;
        app.results[4].marked = true;
        // Press 'M' (Shift+M) to clear all marks.
        app.on_key(KeyEvent::new(KeyCode::Char('M'), KeyModifiers::SHIFT));
        for row in &app.results {
            assert!(!row.marked, "all should be unmarked after 'M'");
        }
    }

    #[test]
    fn test_app_f_toggles_filter() {
        let mut app = make_app_with_results(5);
        assert!(!app.show_marked_only, "initially showing all");
        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        assert!(app.show_marked_only, "should show marked-only after 'f'");
        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        assert!(!app.show_marked_only, "should show all after second 'f'");
    }

    #[test]
    fn test_mark_count_zero_initially() {
        let app = make_app_with_results(5);
        assert_eq!(app.mark_count(), 0);
    }

    #[test]
    fn test_mark_count_reflects_marked() {
        let mut app = make_app_with_results(5);
        app.results[1].marked = true;
        app.results[3].marked = true;
        assert_eq!(app.mark_count(), 2);
    }

    #[test]
    fn test_m_types_in_insert_mode() {
        let mut app = make_app_with_results(5);
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        // In Insert mode, 'm' should type 'm', not toggle mark.
        app.on_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        assert_eq!(app.query, "m");
        assert!(!app.results[0].marked);
    }

    #[test]
    fn test_f_types_in_insert_mode() {
        let mut app = make_app_with_results(5);
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        assert_eq!(app.query, "f");
        assert!(!app.show_marked_only);
    }

    // --- S6-T2 tests: Column navigation ---

    #[test]
    fn test_column_selection_initial_zero() {
        let app = App::new(Theme::default());
        assert_eq!(app.selected_column, 0);
    }

    #[test]
    fn test_column_move_right() {
        let mut app = App::new(Theme::default());
        app.move_column(1);
        assert_eq!(app.selected_column, 1);
    }

    #[test]
    fn test_column_move_left_wraps() {
        let mut app = App::new(Theme::default());
        app.selected_column = 0;
        app.move_column(-1);
        assert_eq!(app.selected_column, app.columns.len() - 1);
    }

    #[test]
    fn test_column_move_right_wraps() {
        let mut app = App::new(Theme::default());
        let last = app.columns.len() - 1;
        app.selected_column = last;
        app.move_column(1);
        assert_eq!(app.selected_column, 0);
    }

    #[test]
    fn test_h_l_type_in_insert_mode() {
        let mut app = make_app_with_results(5);
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        // h/l should type into query in Insert mode, not navigate columns.
        app.on_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));
        assert_eq!(app.query, "hl");
        assert_eq!(app.selected_column, 0);
    }

    // --- S6-T3 tests: Column sorting ---

    #[test]
    fn test_sort_ascending_text() {
        let mut app = App::new(Theme::default());
        app.columns = vec!["vendor".to_string()];
        app.selected_column = 0;
        app.results = vec![
            TableRow {
                meta: UnifiedMetadata {
                    path: std::path::PathBuf::from("/test/b.wav"),
                    vendor: "Zebra".to_string(),
                    ..Default::default()
                },
                audio_info: None,
                marked: false,
                markers: None,
            },
            TableRow {
                meta: UnifiedMetadata {
                    path: std::path::PathBuf::from("/test/a.wav"),
                    vendor: "Alpha".to_string(),
                    ..Default::default()
                },
                audio_info: None,
                marked: false,
                markers: None,
            },
        ];
        app.sort_by_selected_column(true);
        assert_eq!(app.results[0].meta.vendor, "Alpha");
        assert_eq!(app.results[1].meta.vendor, "Zebra");
    }

    #[test]
    fn test_sort_descending_text() {
        let mut app = App::new(Theme::default());
        app.columns = vec!["vendor".to_string()];
        app.selected_column = 0;
        app.results = vec![
            TableRow {
                meta: UnifiedMetadata {
                    path: std::path::PathBuf::from("/test/a.wav"),
                    vendor: "Alpha".to_string(),
                    ..Default::default()
                },
                audio_info: None,
                marked: false,
                markers: None,
            },
            TableRow {
                meta: UnifiedMetadata {
                    path: std::path::PathBuf::from("/test/b.wav"),
                    vendor: "Zebra".to_string(),
                    ..Default::default()
                },
                audio_info: None,
                marked: false,
                markers: None,
            },
        ];
        app.sort_by_selected_column(false);
        assert_eq!(app.results[0].meta.vendor, "Zebra");
        assert_eq!(app.results[1].meta.vendor, "Alpha");
    }

    #[test]
    fn test_sort_numeric_bpm() {
        let mut app = App::new(Theme::default());
        app.columns = vec!["bpm".to_string()];
        app.selected_column = 0;
        app.results = vec![
            TableRow {
                meta: UnifiedMetadata {
                    path: std::path::PathBuf::from("/test/a.wav"),
                    bpm: Some(140),
                    ..Default::default()
                },
                audio_info: None,
                marked: false,
                markers: None,
            },
            TableRow {
                meta: UnifiedMetadata {
                    path: std::path::PathBuf::from("/test/b.wav"),
                    bpm: Some(90),
                    ..Default::default()
                },
                audio_info: None,
                marked: false,
                markers: None,
            },
        ];
        app.sort_by_selected_column(true);
        assert_eq!(app.results[0].meta.bpm, Some(90));
        assert_eq!(app.results[1].meta.bpm, Some(140));
    }

    #[test]
    fn test_sort_empty_values_last() {
        let mut app = App::new(Theme::default());
        app.columns = vec!["vendor".to_string()];
        app.selected_column = 0;
        app.results = vec![
            TableRow {
                meta: UnifiedMetadata {
                    path: std::path::PathBuf::from("/test/a.wav"),
                    vendor: String::new(), // empty
                    ..Default::default()
                },
                audio_info: None,
                marked: false,
                markers: None,
            },
            TableRow {
                meta: UnifiedMetadata {
                    path: std::path::PathBuf::from("/test/b.wav"),
                    vendor: "Alpha".to_string(),
                    ..Default::default()
                },
                audio_info: None,
                marked: false,
                markers: None,
            },
        ];
        app.sort_by_selected_column(true);
        assert_eq!(app.results[0].meta.vendor, "Alpha");
        assert!(app.results[1].meta.vendor.is_empty());
    }

    #[test]
    fn test_sort_resets_selection() {
        let mut app = make_app_with_results(10);
        app.columns = vec!["vendor".to_string()];
        app.selected_column = 0;
        app.selected = 5;
        app.sort_by_selected_column(true);
        assert_eq!(app.selected, 0, "sort should reset selection to 0");
    }

    #[test]
    fn test_sort_indicator_state() {
        let mut app = App::new(Theme::default());
        app.columns = vec!["vendor".to_string()];
        app.selected_column = 0;
        app.results = vec![TableRow {
            meta: UnifiedMetadata {
                path: std::path::PathBuf::from("/test/a.wav"),
                vendor: "V".to_string(),
                ..Default::default()
            },
            audio_info: None,
            marked: false,
            markers: None,
        }];

        app.sort_by_selected_column(true);
        assert_eq!(app.sort_column, Some("vendor".to_string()));
        assert!(app.sort_ascending);

        app.sort_by_selected_column(false);
        assert_eq!(app.sort_column, Some("vendor".to_string()));
        assert!(!app.sort_ascending);
    }

    #[test]
    fn test_sort_empty_results_noop() {
        let mut app = App::new(Theme::default());
        app.columns = vec!["vendor".to_string()];
        app.selected_column = 0;
        // No results — should not panic.
        app.sort_by_selected_column(true);
        assert!(app.sort_column.is_none());
    }

    // --- S7-T2 tests: Input Mode Enum & State ---

    #[test]
    fn test_app_starts_in_normal_mode() {
        let app = App::new(Theme::default());
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn test_enter_insert_mode() {
        let mut app = App::new(Theme::default());
        app.enter_insert_mode();
        assert_eq!(app.input_mode, InputMode::Insert);
    }

    #[test]
    fn test_enter_normal_mode_from_insert() {
        let mut app = App::new(Theme::default());
        app.enter_insert_mode();
        assert_eq!(app.input_mode, InputMode::Insert);
        app.enter_normal_mode();
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    // --- S7-T3 tests: Modal Key Routing ---

    #[test]
    fn test_normal_mode_j_navigates_down() {
        let mut app = make_app_with_results(10);
        assert_eq!(app.input_mode, InputMode::Normal);
        app.on_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(app.selected, 1);
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn test_normal_mode_i_enters_insert() {
        let mut app = App::new(Theme::default());
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        assert_eq!(app.input_mode, InputMode::Insert);
    }

    #[test]
    fn test_normal_mode_slash_enters_insert() {
        let mut app = App::new(Theme::default());
        app.on_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert_eq!(app.input_mode, InputMode::Insert);
    }

    #[test]
    fn test_insert_mode_esc_returns_normal() {
        let mut app = App::new(Theme::default());
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        assert_eq!(app.input_mode, InputMode::Insert);
        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn test_insert_mode_ctrl_c_returns_normal() {
        let mut app = App::new(Theme::default());
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        assert_eq!(app.input_mode, InputMode::Insert);
        app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(!app.should_quit, "Ctrl+C in Insert should not quit");
    }

    #[test]
    fn test_insert_mode_enter_searches_and_returns_normal() {
        let mut app = App::new(Theme::default());
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
        assert_eq!(app.query, "test");
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.query, "test", "query should be preserved after Enter");
    }

    #[test]
    fn test_insert_mode_backspace_empty_returns_normal() {
        let mut app = App::new(Theme::default());
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        assert_eq!(app.input_mode, InputMode::Insert);
        // Backspace on empty query returns to Normal.
        app.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    // --- S7-T5 tests: Action Dispatch ---

    #[test]
    fn test_dispatch_move_down_changes_selected() {
        let mut app = make_app_with_results(10);
        assert_eq!(app.selected, 0);
        app.dispatch(actions::Action::MoveDown);
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn test_dispatch_toggle_playback() {
        let mut app = make_app_with_results(5);
        // No panic even without audio device.
        app.dispatch(actions::Action::TogglePlayback);
        assert!(!app.should_quit);
    }

    #[test]
    fn test_dispatch_toggle_mark() {
        let mut app = make_app_with_results(5);
        assert!(!app.results[0].marked);
        app.dispatch(actions::Action::ToggleMark);
        assert!(app.results[0].marked);
    }

    #[test]
    fn test_dispatch_quit_sets_should_quit() {
        let mut app = App::new(Theme::default());
        assert!(!app.should_quit);
        app.dispatch(actions::Action::Quit);
        assert!(app.should_quit);
    }

    // --- S7-T9 tests: Help Overlay ---

    #[test]
    fn test_help_overlay_toggles() {
        let mut app = App::new(Theme::default());
        assert!(!app.show_help);
        // ? toggles help on.
        app.on_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::SHIFT));
        assert!(app.show_help, "? should open help");
        // ? again toggles help off.
        app.on_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::SHIFT));
        assert!(!app.show_help, "? should close help");
    }

    #[test]
    fn test_help_overlay_toggles_without_shift() {
        let mut app = App::new(Theme::default());
        // Some terminals send ? without SHIFT modifier — should still work.
        app.on_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        assert!(app.show_help, "? without SHIFT should open help");
        app.on_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        assert!(!app.show_help, "? without SHIFT should close help");
    }

    #[test]
    fn test_help_overlay_esc_dismisses() {
        let mut app = App::new(Theme::default());
        app.show_help = true;
        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.show_help, "Esc should close help");
    }

    #[test]
    fn test_help_overlay_reflects_custom_keymap() {
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("x".to_string(), "quit".to_string());
        let km = actions::Keymap::with_overrides(&overrides);
        let entries = km.help_entries();
        // Find the App category entries.
        let app_entries = entries.iter().find(|(cat, _)| *cat == "App");
        assert!(app_entries.is_some(), "should have App category");
        let (_, bindings) = app_entries.unwrap();
        // Check that 'x' maps to Quit.
        assert!(
            bindings.iter().any(|(k, a)| k == "x" && *a == actions::Action::Quit),
            "custom keymap override should be reflected in help entries"
        );
    }

    #[test]
    fn test_action_description_exhaustive() {
        // Every action should have a non-empty description.
        for &action in actions::Action::ALL {
            let desc = action.description();
            assert!(
                !desc.is_empty(),
                "action {:?} should have a non-empty description",
                action
            );
        }
    }

    // --- S7-T7 tests: Insert Mode Search Behavior ---

    #[test]
    fn test_insert_mode_up_arrow_navigates() {
        let mut app = make_app_with_results(10);
        app.selected = 5;
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.selected, 4, "Up arrow should navigate up in Insert mode");
        assert_eq!(app.input_mode, InputMode::Insert, "should stay in Insert mode");
    }

    #[test]
    fn test_insert_mode_down_arrow_navigates() {
        let mut app = make_app_with_results(10);
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.selected, 1, "Down arrow should navigate down in Insert mode");
        assert_eq!(app.input_mode, InputMode::Insert, "should stay in Insert mode");
    }

    #[test]
    fn test_insert_mode_typing_triggers_search() {
        let mut app = App::new(Theme::default());
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert!(app.query_changed, "typing should set query_changed for debounce");
        assert_eq!(app.query, "k");
    }

    #[test]
    fn test_insert_mode_enter_confirms() {
        let mut app = App::new(Theme::default());
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        assert_eq!(app.query, "drum");
        // Reset query_changed to verify Enter re-triggers.
        app.query_changed = false;
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.input_mode, InputMode::Normal, "Enter should return to Normal");
        assert!(app.query_changed, "Enter should trigger search");
        assert_eq!(app.query, "drum", "query should be preserved");
    }

    // --- S7-T1 tests: Search Result Flickering Fix ---

    #[test]
    fn test_search_pending_set_on_query_change() {
        let mut app = make_app_with_results(10);
        // Simulate what the event loop does when search debounce fires.
        app.search_pending = true;
        assert!(app.search_pending);
    }

    #[test]
    fn test_results_not_cleared_before_first_batch() {
        let mut app = make_app_with_results(10);
        // Simulate new search dispatched — results should still be visible.
        app.search_pending = true;
        assert_eq!(app.results.len(), 10, "old results should persist until new batch");
    }

    #[test]
    fn test_results_replaced_on_first_batch() {
        let mut app = make_app_with_results(10);
        app.search_pending = true;
        // First batch of new search arrives — should replace old results.
        let new_row = TableRow {
            meta: UnifiedMetadata {
                path: std::path::PathBuf::from("/new/result.wav"),
                ..Default::default()
            },
            audio_info: None,
            marked: false,
            markers: None,
        };
        app.on_search_results(vec![new_row]);
        assert_eq!(app.results.len(), 1, "old results should be replaced");
        assert_eq!(app.results[0].meta.path, std::path::PathBuf::from("/new/result.wav"));
        assert!(!app.search_pending, "search_pending should be cleared");
    }

    #[test]
    fn test_subsequent_batches_append() {
        let mut app = make_app_with_results(10);
        app.search_pending = true;
        // First batch.
        let row1 = TableRow {
            meta: UnifiedMetadata {
                path: std::path::PathBuf::from("/new/1.wav"),
                ..Default::default()
            },
            audio_info: None,
            marked: false,
            markers: None,
        };
        app.on_search_results(vec![row1]);
        assert_eq!(app.results.len(), 1);
        // Second batch should append, not replace.
        let row2 = TableRow {
            meta: UnifiedMetadata {
                path: std::path::PathBuf::from("/new/2.wav"),
                ..Default::default()
            },
            audio_info: None,
            marked: false,
            markers: None,
        };
        app.on_search_results(vec![row2]);
        assert_eq!(app.results.len(), 2);
    }

    // --- S8-T1 tests: Theme resolution ---

    #[test]
    fn test_theme_resolution_cli_overrides_config() {
        let theme = super::resolve_theme(Some("ableton"), Some("soundminer"));
        assert_eq!(theme.name, "ableton");
    }

    #[test]
    fn test_theme_resolution_config_when_no_cli() {
        let theme = super::resolve_theme(None, Some("soundminer"));
        assert_eq!(theme.name, "soundminer");
    }

    #[test]
    fn test_theme_resolution_default_when_neither() {
        let theme = super::resolve_theme(None, None);
        assert_eq!(theme.name, "telescope");
    }

    #[test]
    fn test_theme_resolution_invalid_config_falls_to_default() {
        let theme = super::resolve_theme(None, Some("nonexistent"));
        assert_eq!(theme.name, "telescope");
    }

    // --- S8-T4 tests: Scrub dispatch ---

    #[test]
    fn test_dispatch_seek_forward_small() {
        let mut app = make_app_with_results(5);
        // Dispatch seek when stopped — should be no-op, no panic.
        app.dispatch(actions::Action::SeekForwardSmall);
        assert_eq!(app.playback_position(), 0.0);
    }

    #[test]
    fn test_dispatch_seek_when_stopped_is_noop() {
        let mut app = make_app_with_results(5);
        app.dispatch(actions::Action::SeekForwardSmall);
        app.dispatch(actions::Action::SeekBackwardSmall);
        app.dispatch(actions::Action::SeekForwardLarge);
        app.dispatch(actions::Action::SeekBackwardLarge);
        // All should be no-ops with no panic.
        assert!(!app.should_quit);
    }

    // --- S8-T5 tests: Auto-advance ---

    #[test]
    fn test_auto_advance_default_off() {
        let app = App::new(Theme::default());
        assert!(!app.auto_advance);
    }

    #[test]
    fn test_toggle_auto_advance() {
        let mut app = App::new(Theme::default());
        app.dispatch(actions::Action::ToggleAutoAdvance);
        assert!(app.auto_advance);
        app.dispatch(actions::Action::ToggleAutoAdvance);
        assert!(!app.auto_advance);
    }

    #[test]
    fn test_auto_advance_on_natural_completion() {
        let mut app = make_app_with_results(5);
        app.auto_advance = true;
        app.was_playing = true;
        app.selected = 0;
        // Simulate Stopped transition (engine is None, so state() returns Stopped).
        app.update_playback_position();
        // With no engine, toggle_playback is a no-op, but selection should advance.
        assert_eq!(app.selected, 1, "should advance to next row");
    }

    #[test]
    fn test_auto_advance_off_no_advance() {
        let mut app = make_app_with_results(5);
        app.auto_advance = false;
        app.was_playing = true;
        app.selected = 0;
        app.update_playback_position();
        assert_eq!(app.selected, 0, "should not advance when auto_advance is off");
    }

    #[test]
    fn test_auto_advance_manual_stop_no_advance() {
        let mut app = make_app_with_results(5);
        app.auto_advance = true;
        app.was_playing = true;
        // Manual stop clears was_playing.
        app.stop_playback();
        assert!(!app.was_playing);
        // Now a Stopped tick should not advance.
        app.update_playback_position();
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn test_auto_advance_at_last_row_stops() {
        let mut app = make_app_with_results(5);
        app.auto_advance = true;
        app.was_playing = true;
        app.selected = 4; // last row
        app.update_playback_position();
        assert_eq!(app.selected, 4, "should not advance past last row");
    }

    // --- S8-T6 tests: Time display toggle ---

    #[test]
    fn test_toggle_time_display() {
        let mut app = App::new(Theme::default());
        assert!(!app.show_remaining);
        app.dispatch(actions::Action::ToggleTimeDisplay);
        assert!(app.show_remaining);
        app.dispatch(actions::Action::ToggleTimeDisplay);
        assert!(!app.show_remaining);
    }

    // --- S9-T6 tests: Store markers in App state ---

    #[test]
    fn test_preview_data_with_markers() {
        let markers = crate::engine::bext::MarkerConfig::preset_shot();
        let preview = PreviewData {
            metadata: UnifiedMetadata::default(),
            peaks: vec![],
            audio_info: None,
            markers: Some(markers),
        };
        assert!(preview.markers.is_some());
        assert_eq!(preview.markers.unwrap(), markers);
    }

    #[test]
    fn test_preview_data_no_markers() {
        let preview = PreviewData {
            metadata: UnifiedMetadata::default(),
            peaks: vec![],
            audio_info: None,
            markers: None,
        };
        assert!(preview.markers.is_none());
    }

    #[test]
    fn test_table_row_carries_markers() {
        let markers = crate::engine::bext::MarkerConfig::preset_loop(48000);
        let row = TableRow {
            meta: UnifiedMetadata::default(),
            audio_info: None,
            marked: false,
            markers: Some(markers),
        };
        assert!(row.markers.is_some());
        assert_eq!(row.markers.unwrap(), markers);
    }

    #[test]
    fn test_on_preview_ready_carries_markers() {
        let mut app = App::new(Theme::default());
        let markers = crate::engine::bext::MarkerConfig::preset_shot();
        app.on_preview_ready(PreviewData {
            metadata: UnifiedMetadata {
                path: std::path::PathBuf::from("/test/marker.wav"),
                ..Default::default()
            },
            peaks: vec![],
            audio_info: None,
            markers: Some(markers),
        });
        assert!(app.preview.is_some());
        assert_eq!(app.preview.unwrap().markers, Some(markers));
    }

    #[test]
    fn test_preview_markers_cleared_on_new_preview() {
        let mut app = App::new(Theme::default());
        // Set up a preview with markers.
        let markers = crate::engine::bext::MarkerConfig::preset_shot();
        app.on_preview_ready(PreviewData {
            metadata: UnifiedMetadata {
                path: std::path::PathBuf::from("/test/marker.wav"),
                ..Default::default()
            },
            peaks: vec![],
            audio_info: None,
            markers: Some(markers),
        });
        assert!(app.preview.as_ref().unwrap().markers.is_some());

        // Overwrite with a preview that has no markers.
        app.on_preview_ready(PreviewData {
            metadata: UnifiedMetadata {
                path: std::path::PathBuf::from("/test/no_marker.wav"),
                ..Default::default()
            },
            peaks: vec![],
            audio_info: None,
            markers: None,
        });
        assert!(app.preview.as_ref().unwrap().markers.is_none());
    }

    // --- S9-T9 tests: SaveMarkers action ---

    #[test]
    fn test_current_markers_or_default_shot() {
        let app = App::new(Theme::default());
        // No preview, no results → should return preset_shot.
        let markers = app.current_markers_or_default();
        assert_eq!(markers, crate::engine::bext::MarkerConfig::preset_shot());
    }

    #[test]
    fn test_current_markers_or_default_uses_existing() {
        let mut app = App::new(Theme::default());
        let custom = crate::engine::bext::MarkerConfig::preset_loop(48000);
        app.on_preview_ready(PreviewData {
            metadata: UnifiedMetadata {
                path: std::path::PathBuf::from("/test/marker.wav"),
                ..Default::default()
            },
            peaks: vec![],
            audio_info: None,
            markers: Some(custom),
        });
        assert_eq!(app.current_markers_or_default(), custom);
    }

    #[test]
    fn test_current_markers_or_default_falls_through_to_row() {
        let mut app = App::new(Theme::default());
        let custom = crate::engine::bext::MarkerConfig::preset_loop(96000);
        app.results = vec![TableRow {
            meta: UnifiedMetadata::default(),
            audio_info: None,
            marked: false,
            markers: Some(custom),
        }];
        app.selected = 0;
        // No preview set → should use row markers.
        assert_eq!(app.current_markers_or_default(), custom);
    }

    #[test]
    fn test_save_markers_no_selection() {
        let mut app = App::new(Theme::default());
        app.save_markers();
        assert!(
            app.status_message.as_deref() == Some("No file selected"),
            "expected 'No file selected' but got {:?}",
            app.status_message
        );
    }

    #[test]
    fn test_status_message_set() {
        let mut app = App::new(Theme::default());
        app.set_status("hello".to_string());
        assert_eq!(app.status_message.as_deref(), Some("hello"));
        assert!(app.status_message_time.is_some());
    }

    // --- S10-T2 tests: Duration-aware preset defaults ---

    #[test]
    fn test_default_preset_loop_for_long_file() {
        let mut app = App::new(Theme::default());
        // Preview with audio_info showing 5 seconds at 48kHz = 240000 samples.
        app.on_preview_ready(PreviewData {
            metadata: UnifiedMetadata::default(),
            peaks: vec![],
            audio_info: Some(crate::engine::wav::AudioInfo {
                duration_secs: 5.0,
                sample_rate: 48000,
                bit_depth: 16,
                channels: 1,
            }),
            markers: None,
        });
        let markers = app.current_markers_or_default();
        let expected = crate::engine::bext::MarkerConfig::preset_loop(240000);
        assert_eq!(markers, expected, "5s file should get preset_loop");
    }

    #[test]
    fn test_default_preset_shot_for_short_file() {
        let mut app = App::new(Theme::default());
        // Preview with audio_info showing 0.5 seconds.
        app.on_preview_ready(PreviewData {
            metadata: UnifiedMetadata::default(),
            peaks: vec![],
            audio_info: Some(crate::engine::wav::AudioInfo {
                duration_secs: 0.5,
                sample_rate: 48000,
                bit_depth: 16,
                channels: 1,
            }),
            markers: None,
        });
        let markers = app.current_markers_or_default();
        assert_eq!(
            markers,
            crate::engine::bext::MarkerConfig::preset_shot(),
            "0.5s file should get preset_shot"
        );
    }

    // --- S10-T3 tests: Active bank state ---

    #[test]
    fn test_toggle_bank() {
        let mut app = App::new(Theme::default());
        assert_eq!(app.active_bank, Bank::A);
        app.toggle_bank();
        assert_eq!(app.active_bank, Bank::B);
        assert_eq!(app.status_message.as_deref(), Some("Active: Bank B"));
        app.toggle_bank();
        assert_eq!(app.active_bank, Bank::A);
        assert_eq!(app.status_message.as_deref(), Some("Active: Bank A"));
    }

    #[test]
    fn test_active_bank_mut_returns_correct_bank() {
        let mut app = App::new(Theme::default());
        let markers = crate::engine::bext::MarkerConfig::preset_loop(48000);
        app.on_preview_ready(PreviewData {
            metadata: UnifiedMetadata::default(),
            peaks: vec![],
            audio_info: None,
            markers: Some(markers),
        });
        // Bank A is default.
        let bank_a = app.active_bank_mut().unwrap();
        assert_eq!(bank_a.m1, 12000);

        // Toggle to Bank B and verify.
        app.active_bank = Bank::B;
        let bank_b = app.active_bank_mut().unwrap();
        assert_eq!(bank_b.m1, 12000); // preset_loop has synced banks
    }

    #[test]
    fn test_active_bank_mut_none_without_preview() {
        let mut app = App::new(Theme::default());
        assert!(app.active_bank_mut().is_none());
    }

    #[test]
    fn test_active_bank_mut_none_without_markers() {
        let mut app = App::new(Theme::default());
        app.on_preview_ready(PreviewData {
            metadata: UnifiedMetadata::default(),
            peaks: vec![],
            audio_info: None,
            markers: None,
        });
        assert!(app.active_bank_mut().is_none());
    }

    #[test]
    fn test_total_samples() {
        let mut app = App::new(Theme::default());
        assert!(app.total_samples().is_none());

        app.on_preview_ready(PreviewData {
            metadata: UnifiedMetadata::default(),
            peaks: vec![],
            audio_info: Some(crate::engine::wav::AudioInfo {
                duration_secs: 1.0,
                sample_rate: 48000,
                bit_depth: 16,
                channels: 1,
            }),
            markers: None,
        });
        assert_eq!(app.total_samples(), Some(48000));
    }

    // --- S10-T4 tests: Ensure markers ---

    #[test]
    fn test_ensure_markers_initializes_when_none() {
        let mut app = App::new(Theme::default());
        app.on_preview_ready(PreviewData {
            metadata: UnifiedMetadata::default(),
            peaks: vec![],
            audio_info: Some(crate::engine::wav::AudioInfo {
                duration_secs: 5.0,
                sample_rate: 48000,
                bit_depth: 16,
                channels: 1,
            }),
            markers: None,
        });
        assert!(app.preview.as_ref().unwrap().markers.is_none());
        app.ensure_markers();
        assert!(app.preview.as_ref().unwrap().markers.is_some());
        let markers = app.preview.as_ref().unwrap().markers.unwrap();
        // 5s @ 48kHz = 240000 samples → preset_loop
        assert_eq!(markers, crate::engine::bext::MarkerConfig::preset_loop(240000));
    }

    #[test]
    fn test_ensure_markers_preserves_existing() {
        let mut app = App::new(Theme::default());
        let custom = crate::engine::bext::MarkerConfig::preset_shot();
        app.on_preview_ready(PreviewData {
            metadata: UnifiedMetadata::default(),
            peaks: vec![],
            audio_info: None,
            markers: Some(custom),
        });
        app.ensure_markers();
        assert_eq!(app.preview.as_ref().unwrap().markers, Some(custom));
    }

    // --- S10-T5 tests: Clear bank markers ---

    #[test]
    fn test_clear_bank_markers() {
        let mut app = App::new(Theme::default());
        let markers = crate::engine::bext::MarkerConfig::preset_loop(48000);
        app.on_preview_ready(PreviewData {
            metadata: UnifiedMetadata::default(),
            peaks: vec![],
            audio_info: None,
            markers: Some(markers),
        });
        app.clear_bank_markers();
        let bank_a = app.active_bank_ref().unwrap();
        assert!(bank_a.is_empty());
        assert_eq!(app.status_message.as_deref(), Some("Bank A cleared"));
    }

    #[test]
    fn test_clear_bank_markers_no_preview() {
        let mut app = App::new(Theme::default());
        app.clear_bank_markers();
        assert_eq!(app.status_message.as_deref(), Some("No markers"));
    }

    // --- S10-T6 tests: Segment bounds ---

    #[test]
    fn test_segments_with_markers() {
        let mut app = App::new(Theme::default());
        let mut markers = crate::engine::bext::MarkerConfig::preset_loop(48000);
        markers.bank_a.m1 = 12000;
        markers.bank_a.m2 = 24000;
        markers.bank_a.m3 = 36000;
        app.on_preview_ready(PreviewData {
            metadata: UnifiedMetadata::default(),
            peaks: vec![],
            audio_info: Some(crate::engine::wav::AudioInfo {
                duration_secs: 1.0,
                sample_rate: 48000,
                bit_depth: 16,
                channels: 1,
            }),
            markers: Some(markers),
        });
        let segs = app.segments();
        assert_eq!(segs.len(), 4);
        assert_eq!((segs[0].start, segs[0].end), (0, 12000));
        assert_eq!((segs[1].start, segs[1].end), (12000, 24000));
        assert_eq!((segs[2].start, segs[2].end), (24000, 36000));
        assert_eq!((segs[3].start, segs[3].end), (36000, 48000));
        assert!(segs.iter().all(|s| !s.reverse));
    }

    #[test]
    fn test_segments_no_preview() {
        let app = App::new(Theme::default());
        assert!(app.segments().is_empty());
    }

    #[test]
    fn test_segments_empty_markers() {
        let mut app = App::new(Theme::default());
        app.on_preview_ready(PreviewData {
            metadata: UnifiedMetadata::default(),
            peaks: vec![],
            audio_info: Some(crate::engine::wav::AudioInfo {
                duration_secs: 1.0,
                sample_rate: 48000,
                bit_depth: 16,
                channels: 1,
            }),
            markers: Some(crate::engine::bext::MarkerConfig {
                bank_a: crate::engine::bext::MarkerBank::empty(),
                bank_b: crate::engine::bext::MarkerBank::empty(),
            }),
        });
        let segs = app.segments();
        // Always exactly 4 segments. EMPTY markers collapse to zero-length.
        assert_eq!(segs.len(), 4);
        // First 3 are zero-length (all EMPTY markers resolve to 0).
        assert_eq!(segs[0].start, 0);
        assert_eq!(segs[0].end, 0);
        assert_eq!(segs[1].start, 0);
        assert_eq!(segs[1].end, 0);
        assert_eq!(segs[2].start, 0);
        assert_eq!(segs[2].end, 0);
        // Last segment spans full file.
        assert_eq!(segs[3].start, 0);
        assert_eq!(segs[3].end, 48000);
    }

    #[test]
    fn test_segments_reverse_encoding() {
        // When m1 > m2, segment 2 should be marked reverse.
        use crate::engine::bext::{MarkerBank, MARKER_EMPTY};
        let bank = MarkerBank {
            m1: 30000,
            m2: 10000,
            m3: MARKER_EMPTY,
            reps: [1, 1, 1, 1],
        };
        let segs = segment_bounds(&bank, 48000);
        assert_eq!(segs.len(), 4);
        // seg 0: [0, 30000] forward
        assert_eq!((segs[0].start, segs[0].end), (0, 30000));
        assert!(!segs[0].reverse);
        // seg 1: [30000, 10000] reverse
        assert_eq!((segs[1].start, segs[1].end), (30000, 10000));
        assert!(segs[1].reverse);
        // seg 2: [10000, 10000] zero-length (EMPTY resolves to prev=10000)
        assert_eq!((segs[2].start, segs[2].end), (10000, 10000));
        assert!(!segs[2].reverse);
        // seg 3: [10000, 48000] forward
        assert_eq!((segs[3].start, segs[3].end), (10000, 48000));
        assert!(!segs[3].reverse);
    }

    // --- S10-T7/T8 tests: Segment/program playback state ---

    #[test]
    fn test_stop_playback_clears_segment_state() {
        let mut app = App::new(Theme::default());
        app.segment_start = Some(1000);
        app.segment_end = Some(2000);
        app.segment_reps_remaining = 3;
        app.program_playlist = vec![(0, 1000, 2)];
        app.program_index = 0;
        app.stop_playback();
        assert!(app.segment_start.is_none());
        assert!(app.segment_end.is_none());
        assert_eq!(app.segment_reps_remaining, 0);
        assert!(app.program_playlist.is_empty());
        assert_eq!(app.program_index, 0);
    }

    #[test]
    fn test_play_program_empty_bounds() {
        let mut app = App::new(Theme::default());
        // No preview → empty segment_bounds → should report "No segments".
        app.play_program();
        assert_eq!(app.status_message.as_deref(), Some("No segments"));
    }

    #[test]
    fn test_start_program_segment_past_end() {
        let mut app = App::new(Theme::default());
        app.program_playlist = vec![(0, 100, 1)];
        app.program_index = 5; // past the end
        app.start_program_segment();
        assert_eq!(app.status_message.as_deref(), Some("Program complete"));
        assert!(app.program_playlist.is_empty());
    }

    // --- S10F-T5: Proptest coverage for segments ---

    mod segment_proptests {
        use super::*;
        use crate::engine::bext::{MarkerBank, MARKER_EMPTY};
        use proptest::prelude::*;

        // --- Generators ---
        //
        // Domain-aware strategies that weight interesting values:
        //   - MARKER_EMPTY (~25%): the sentinel for "no marker in this slot"
        //   - 0 and total (~10% each): file boundaries where segments collapse
        //   - Interior (~55%): uniform over [0, total]
        //
        // Total sample count is also weighted toward small values where
        // boundary collisions are more likely.

        /// File length: weighted toward small values for denser boundary coverage.
        fn arb_total() -> impl Strategy<Value = u32> {
            prop_oneof![
                3 => 1u32..=4,              // tiny: markers must collide
                3 => 5u32..=1_000,          // small: boundaries close together
                4 => 1_001u32..=1_000_000,  // normal: realistic file lengths
            ]
        }

        /// Single marker value for a given file length.
        fn arb_marker(total: u32) -> impl Strategy<Value = u32> {
            prop_oneof![
                5 => Just(MARKER_EMPTY),  // ~25%: empty slot
                2 => Just(0u32),          // ~10%: start-of-file
                2 => Just(total),         // ~10%: end-of-file
                11 => 0..=total,          // ~55%: uniform interior
            ]
        }

        /// Full MarkerBank with weighted marker and rep generation.
        fn arb_bank(total: u32) -> impl Strategy<Value = MarkerBank> {
            (
                arb_marker(total), arb_marker(total), arb_marker(total),
                0u8..16, 0u8..16, 0u8..16, 0u8..16,
            ).prop_map(|(m1, m2, m3, r0, r1, r2, r3)| {
                MarkerBank { m1, m2, m3, reps: [r0, r1, r2, r3] }
            })
        }

        /// (total, bank) pair where markers are valid for the given total.
        fn arb_total_and_bank() -> impl Strategy<Value = (u32, MarkerBank)> {
            arb_total().prop_flat_map(|total| {
                arb_bank(total).prop_map(move |bank| (total, bank))
            })
        }

        // --- Group 3: Segment computation invariants ---

        proptest! {
            #![proptest_config(proptest::prelude::ProptestConfig::with_cases(512))]

            /// P3: Always exactly 4 segments.
            #[test]
            fn proptest_always_4_segments((total, bank) in arb_total_and_bank()) {
                let segs = segment_bounds(&bank, total);
                prop_assert_eq!(segs.len(), 4);
            }

            /// P4: Rep indices match bank slots.
            #[test]
            fn proptest_rep_indices_match((total, bank) in arb_total_and_bank()) {
                let segs = segment_bounds(&bank, total);
                for i in 0..4 {
                    prop_assert_eq!(segs[i].rep, bank.reps[i]);
                }
            }

            /// P5: All boundaries within [0, total].
            #[test]
            fn proptest_boundaries_in_range((total, bank) in arb_total_and_bank()) {
                let segs = segment_bounds(&bank, total);
                for (i, seg) in segs.iter().enumerate() {
                    let hi = seg.start.max(seg.end);
                    prop_assert!(hi <= total,
                        "seg[{}]: max({}, {}) = {} > total={}", i, seg.start, seg.end, hi, total);
                }
            }

            /// P6: Forward segments have start <= end.
            #[test]
            fn proptest_forward_start_le_end((total, bank) in arb_total_and_bank()) {
                let segs = segment_bounds(&bank, total);
                for (i, seg) in segs.iter().enumerate() {
                    if !seg.reverse {
                        prop_assert!(seg.start <= seg.end,
                            "seg[{}]: forward but start={} > end={}", i, seg.start, seg.end);
                    }
                }
            }

            /// P7: Reverse segments have start > end.
            #[test]
            fn proptest_reverse_start_gt_end((total, bank) in arb_total_and_bank()) {
                let segs = segment_bounds(&bank, total);
                for (i, seg) in segs.iter().enumerate() {
                    if seg.reverse {
                        prop_assert!(seg.start > seg.end,
                            "seg[{}]: reverse but start={} <= end={}", i, seg.start, seg.end);
                    }
                }
            }

            // --- Group 4: Direction encoding ---

            /// P8: Marker order determines direction of the segment between them.
            #[test]
            fn proptest_marker_order_direction(
                total in arb_total(),
                m1_frac in 0.0f64..=1.0,
                m2_frac in 0.0f64..=1.0,
            ) {
                let m1 = (m1_frac * total as f64) as u32;
                let m2 = (m2_frac * total as f64) as u32;
                let bank = MarkerBank {
                    m1, m2, m3: MARKER_EMPTY,
                    reps: [1, 1, 0, 0],
                };
                let segs = segment_bounds(&bank, total);
                if m1 > m2 {
                    prop_assert!(segs[1].reverse,
                        "m1={} > m2={} but seg[1] not reverse", m1, m2);
                } else {
                    prop_assert!(!segs[1].reverse,
                        "m1={} <= m2={} but seg[1] reverse", m1, m2);
                }
            }

            /// P9: Identical adjacent markers produce zero-length forward segment.
            #[test]
            fn proptest_identical_markers_zero_length(
                total in arb_total(),
                pos_frac in 0.0f64..=1.0,
            ) {
                let pos = (pos_frac * total as f64) as u32;
                let bank = MarkerBank {
                    m1: pos, m2: pos, m3: MARKER_EMPTY,
                    reps: [1, 1, 0, 0],
                };
                let segs = segment_bounds(&bank, total);
                prop_assert_eq!(segs[1].start, segs[1].end);
                prop_assert!(!segs[1].reverse);
            }

            // --- Group 5: MARKER_EMPTY handling ---

            /// P10: All-EMPTY bank: first 3 zero-length, last spans full file.
            #[test]
            fn proptest_all_empty_bank(total in arb_total()) {
                let bank = MarkerBank::empty();
                let segs = segment_bounds(&bank, total);
                for i in 0..3 {
                    prop_assert_eq!(segs[i].start, 0);
                    prop_assert_eq!(segs[i].end, 0);
                }
                prop_assert_eq!(segs[3].start, 0);
                prop_assert_eq!(segs[3].end, total);
            }

            /// P11: Single EMPTY marker collapses its segment to zero-length.
            #[test]
            fn proptest_single_empty_collapses(
                total in arb_total(),
                m1_frac in 0.0f64..=1.0,
                m3_frac in 0.0f64..=1.0,
            ) {
                let m1 = (m1_frac * total as f64) as u32;
                let m3 = (m3_frac * total as f64) as u32;
                let bank = MarkerBank {
                    m1, m2: MARKER_EMPTY, m3,
                    reps: [1, 1, 1, 1],
                };
                let segs = segment_bounds(&bank, total);
                // m2 resolves to m1 (previous boundary), so seg[1] is zero-length.
                prop_assert_eq!(segs[1].start, segs[1].end,
                    "EMPTY m2: seg[1] should be zero-length, got {}..{}", segs[1].start, segs[1].end);
            }
        }

        // --- Group 7: Infinite loop / skip ---

        #[test]
        fn test_rep15_stored_in_segment() {
            let bank = MarkerBank {
                m1: 12000, m2: 24000, m3: 36000,
                reps: [15, 1, 1, 1],
            };
            let segs = segment_bounds(&bank, 48000);
            // rep=15 stored as-is in Segment; the runtime sentinel (u8::MAX)
            // is applied in play_segment/start_program_segment/update_playback_position.
            assert_eq!(segs[0].rep, 15);
        }

        #[test]
        fn test_rep0_means_skip() {
            let bank = MarkerBank {
                m1: 12000, m2: 24000, m3: 36000,
                reps: [0, 1, 0, 1],
            };
            let segs = segment_bounds(&bank, 48000);
            assert_eq!(segs[0].rep, 0);
            assert_eq!(segs[2].rep, 0);
        }
    }

    // --- S10 action name round-trip tests ---

    #[test]
    fn test_new_action_names_roundtrip() {
        use crate::ui::actions::Action;
        let actions = [
            Action::ToggleBank,
            Action::SetMarker1,
            Action::SetMarker2,
            Action::SetMarker3,
            Action::ClearNearestMarker,
            Action::ClearBankMarkers,
            Action::IncrementRep,
            Action::DecrementRep,
            Action::PlaySegment,
            Action::PlayProgram,
        ];
        for action in &actions {
            let name = action.name();
            let parsed = Action::from_name(name);
            assert_eq!(parsed, Some(*action), "round-trip failed for {name}");
        }
    }
}
