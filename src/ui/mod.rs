//! Interactive TUI: state machine, event loop, and async bridge.

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
}

/// Events that drive the TUI state machine.
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
    /// Paths that have been previewed (session-only, for visited-link styling).
    pub visited: HashSet<std::path::PathBuf>,
    /// Audio playback engine (None if no audio device).
    pub playback: Option<PlaybackEngine>,
    /// Playback position as fraction 0.0–1.0 for waveform cursor.
    pub playback_position: f32,
    /// Mark store for file marking (None if no store available).
    pub marks: Option<Box<dyn MarkStore>>,
    /// Whether to show only marked files.
    pub show_marked_only: bool,
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
            viewport_height: PAGE_SIZE,
            columns: crate::engine::config::default_columns(),
            pending_g: false,
            query_changed: false,
            selection_changed: false,
            visited: HashSet::new(),
            playback,
            playback_position: 0.0,
            marks: None,
            show_marked_only: false,
        }
    }

    /// Handle a key event, updating state accordingly.
    pub fn on_key(&mut self, key: KeyEvent) {
        // Ctrl+C always quits.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            self.pending_g = false;
            return;
        }

        // Ctrl+D: page down.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('d') {
            self.page_down();
            self.pending_g = false;
            return;
        }

        // Ctrl+U: page up.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('u') {
            self.page_up();
            self.pending_g = false;
            return;
        }

        match key.code {
            KeyCode::Char('q') if key.modifiers.is_empty() && self.query.is_empty() => {
                self.should_quit = true;
                self.pending_g = false;
            }
            KeyCode::Char('j') if key.modifiers.is_empty() && self.query.is_empty() => {
                self.move_selection(1);
                self.pending_g = false;
            }
            KeyCode::Char('k') if key.modifiers.is_empty() && self.query.is_empty() => {
                self.move_selection(-1);
                self.pending_g = false;
            }
            KeyCode::Char(' ') if key.modifiers.is_empty() && self.query.is_empty() => {
                self.toggle_playback();
                self.pending_g = false;
            }
            KeyCode::Char('s') if key.modifiers.is_empty() && self.query.is_empty() => {
                self.stop_playback();
                self.pending_g = false;
            }
            KeyCode::Char('m') if key.modifiers.is_empty() && self.query.is_empty() => {
                self.toggle_mark();
                self.pending_g = false;
            }
            KeyCode::Char('M') if key.modifiers == KeyModifiers::SHIFT && self.query.is_empty() => {
                self.clear_all_marks();
                self.pending_g = false;
            }
            KeyCode::Char('f') if key.modifiers.is_empty() && self.query.is_empty() => {
                self.toggle_marked_filter();
                self.pending_g = false;
            }
            KeyCode::Char('G') if key.modifiers == KeyModifiers::SHIFT && self.query.is_empty() => {
                self.jump_bottom();
                self.pending_g = false;
            }
            KeyCode::Char('g') if key.modifiers.is_empty() && self.query.is_empty() => {
                if self.pending_g {
                    self.jump_top();
                    self.pending_g = false;
                } else {
                    self.pending_g = true;
                }
                return; // Don't reset pending_g.
            }
            KeyCode::Down => {
                self.move_selection(1);
                self.pending_g = false;
            }
            KeyCode::Up => {
                self.move_selection(-1);
                self.pending_g = false;
            }
            KeyCode::Enter => {
                self.open_selected();
                self.pending_g = false;
            }
            KeyCode::Esc => {
                if !self.query.is_empty() {
                    self.query.clear();
                    self.cursor_pos = 0;
                    self.query_changed = true;
                } else {
                    self.should_quit = true;
                }
                self.pending_g = false;
            }
            KeyCode::Backspace => {
                if self.cursor_pos > 0 {
                    self.query.remove(self.cursor_pos - 1);
                    self.cursor_pos -= 1;
                    self.query_changed = true;
                }
                self.pending_g = false;
            }
            KeyCode::Char(c) => {
                self.query.insert(self.cursor_pos, c);
                self.cursor_pos += 1;
                self.query_changed = true;
                self.pending_g = false;
            }
            _ => {
                self.pending_g = false;
            }
        }
    }

    /// Handle incoming search results.
    pub fn on_search_results(&mut self, results: Vec<TableRow>) {
        if self.results.is_empty() {
            // First batch: reset selection.
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
        self.visited.insert(data.metadata.path.clone());
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

    /// Toggle playback: play selected file if stopped, else toggle pause.
    fn toggle_playback(&mut self) {
        let engine = match &self.playback {
            Some(e) => e,
            None => return,
        };
        let state = engine.state();
        match state {
            PlaybackState::Stopped => {
                // Play the selected file.
                if let Some(row) = self.results.get(self.selected) {
                    let _ = engine.play(&row.meta.path);
                }
            }
            PlaybackState::Playing | PlaybackState::Paused => {
                engine.toggle_pause();
            }
        }
    }

    /// Stop playback.
    fn stop_playback(&mut self) {
        if let Some(ref engine) = self.playback {
            engine.stop();
        }
        self.playback_position = 0.0;
    }

    /// Update playback position from elapsed/duration. Called on each tick.
    pub fn update_playback_position(&mut self) {
        let engine = match &self.playback {
            Some(e) => e,
            None => return,
        };
        match engine.state() {
            PlaybackState::Playing => {
                if let Some(duration) = engine.duration() {
                    let secs = duration.as_secs_f32();
                    if secs > 0.0 {
                        self.playback_position =
                            (engine.elapsed().as_secs_f32() / secs).clamp(0.0, 1.0);
                    }
                }
            }
            PlaybackState::Paused => {
                // Keep position frozen.
            }
            PlaybackState::Stopped => {
                self.playback_position = 0.0;
            }
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

    /// Get the total number of marked files (from store if available, else from results).
    pub fn mark_count(&self) -> usize {
        if let Some(ref store) = self.marks {
            store.mark_count()
        } else {
            self.results.iter().filter(|r| r.marked).count()
        }
    }

    /// Get visible results (filtered by marked-only if enabled).
    pub fn visible_results(&self) -> Vec<&TableRow> {
        if self.show_marked_only {
            self.results.iter().filter(|r| r.marked).collect()
        } else {
            self.results.iter().collect()
        }
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

    let theme = match &opts.theme {
        Some(name) => Theme::by_name(name)?,
        None => Theme::default(),
    };

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

    // Apply config columns if set.
    let config = crate::engine::config::load_config();
    if let Some(ref cols) = config.columns {
        if !cols.is_empty() {
            app.columns = cols.clone();
        }
    }

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
                app.results.clear();
                app.selected = 0;
                app.scroll_offset = 0;
                app.preview = None;
                app.search_in_progress = true;

                // Build new search.
                let query = crate::engine::SearchQuery {
                    freetext: if app.query.is_empty() {
                        None
                    } else {
                        Some(app.query.clone())
                    },
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
        app.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert_eq!(app.query, "a");
        assert_eq!(app.cursor_pos, 1);
        assert!(app.query_changed);
    }

    #[test]
    fn test_app_type_multiple() {
        let mut app = App::new(Theme::default());
        // Use 'x' not 'f' since 'f' is reserved (filter toggle) when query is empty.
        for ch in ['x', 'o', 'o'] {
            app.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        assert_eq!(app.query, "xoo");
        assert_eq!(app.cursor_pos, 3);
    }

    #[test]
    fn test_app_backspace() {
        let mut app = App::new(Theme::default());
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
        }]);
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn test_app_quit_on_q() {
        let mut app = App::new(Theme::default());
        // q quits only when query is empty.
        app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(app.should_quit);
    }

    #[test]
    fn test_app_quit_on_ctrl_c() {
        let mut app = App::new(Theme::default());
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
    fn test_app_q_doesnt_quit_with_query() {
        let mut app = App::new(Theme::default());
        app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        // Now query is "x", pressing q should type 'q', not quit.
        app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(!app.should_quit);
        assert_eq!(app.query, "xq");
    }

    #[test]
    fn test_app_vim_keys_only_when_query_empty() {
        let mut app = make_app_with_results(10);
        app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        // j should type 'j', not move selection.
        app.on_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(app.query, "xj");
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
    fn test_app_space_types_when_query_active() {
        let mut app = make_app_with_results(5);
        app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        // Space should type ' ' when query is non-empty.
        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert_eq!(app.query, "x ");
    }

    #[test]
    fn test_app_s_stops_playback() {
        let mut app = make_app_with_results(5);
        // s on empty query should stop playback (no panic).
        app.on_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(app.playback_position, 0.0);
        assert!(!app.should_quit);
    }

    #[test]
    fn test_app_s_types_when_query_active() {
        let mut app = make_app_with_results(5);
        app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        // s should type 's' when query is non-empty.
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
        app.playback_position = 0.5; // Simulate mid-playback.
        app.move_selection(1);
        assert_eq!(app.playback_position, 0.0, "selection change should reset position");
    }

    // --- S5-T5 tests: Visited file coloring ---

    #[test]
    fn test_visited_set_empty_initially() {
        let app = App::new(Theme::default());
        assert!(app.visited.is_empty());
    }

    #[test]
    fn test_visited_set_populated_on_preview() {
        let mut app = App::new(Theme::default());
        let path = std::path::PathBuf::from("/test/kick.wav");
        app.on_preview_ready(PreviewData {
            metadata: UnifiedMetadata {
                path: path.clone(),
                ..Default::default()
            },
            peaks: vec![],
            audio_info: None,
        });
        assert!(app.visited.contains(&path));
    }

    #[test]
    fn test_selected_overrides_visited() {
        let mut app = make_app_with_results(3);
        // Mark first item as visited.
        app.visited.insert(std::path::PathBuf::from("/test/0.wav"));
        app.selected = 0;
        // The style for selected should override visited — verified via render test.
        // Just verify the data model is correct.
        assert!(app.visited.contains(&std::path::PathBuf::from("/test/0.wav")));
        assert_eq!(app.selected, 0);
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
    fn test_m_types_when_query_active() {
        let mut app = make_app_with_results(5);
        // Type something to activate query mode.
        app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        // Now 'm' should type 'm', not toggle mark.
        app.on_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        assert_eq!(app.query, "xm");
        assert!(!app.results[0].marked);
    }

    #[test]
    fn test_f_types_when_query_active() {
        let mut app = make_app_with_results(5);
        app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        assert_eq!(app.query, "xf");
        assert!(!app.show_marked_only);
    }
}
