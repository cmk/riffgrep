//! Action enum for decoupling keybindings from behavior.

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Every user-triggerable action in the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    // Navigation
    MoveDown,
    MoveUp,
    MoveToTop,
    MoveToBottom,
    PageDown,
    PageUp,
    MoveColumnLeft,
    MoveColumnRight,

    // Sort
    SortAscending,
    SortDescending,

    // Playback
    TogglePlayback,
    StopPlayback,
    SeekForwardSmall,
    SeekForwardLarge,
    SeekBackwardSmall,
    SeekBackwardLarge,
    ToggleAutoAdvance,
    ToggleTimeDisplay,

    // Marks
    ToggleMark,
    ClearMarks,
    ToggleMarkedFilter,
    SaveMarkers,

    // Mode
    EnterInsertMode,
    EnterNormalMode,
    SearchSubmit,
    ClearQuery,

    // Selection
    OpenSelected,

    // Help
    ShowHelp,

    // App
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
        Action::TogglePlayback,
        Action::StopPlayback,
        Action::SeekForwardSmall,
        Action::SeekForwardLarge,
        Action::SeekBackwardSmall,
        Action::SeekBackwardLarge,
        Action::ToggleAutoAdvance,
        Action::ToggleTimeDisplay,
        Action::ToggleMark,
        Action::ClearMarks,
        Action::ToggleMarkedFilter,
        Action::SaveMarkers,
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
            "toggle_playback" => Some(Action::TogglePlayback),
            "stop_playback" => Some(Action::StopPlayback),
            "seek_forward_small" => Some(Action::SeekForwardSmall),
            "seek_forward_large" => Some(Action::SeekForwardLarge),
            "seek_backward_small" => Some(Action::SeekBackwardSmall),
            "seek_backward_large" => Some(Action::SeekBackwardLarge),
            "toggle_auto_advance" => Some(Action::ToggleAutoAdvance),
            "toggle_time_display" => Some(Action::ToggleTimeDisplay),
            "toggle_mark" => Some(Action::ToggleMark),
            "clear_marks" => Some(Action::ClearMarks),
            "toggle_marked_filter" => Some(Action::ToggleMarkedFilter),
            "save_markers" => Some(Action::SaveMarkers),
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
            Action::TogglePlayback => "toggle_playback",
            Action::StopPlayback => "stop_playback",
            Action::SeekForwardSmall => "seek_forward_small",
            Action::SeekForwardLarge => "seek_forward_large",
            Action::SeekBackwardSmall => "seek_backward_small",
            Action::SeekBackwardLarge => "seek_backward_large",
            Action::ToggleAutoAdvance => "toggle_auto_advance",
            Action::ToggleTimeDisplay => "toggle_time_display",
            Action::ToggleMark => "toggle_mark",
            Action::ClearMarks => "clear_marks",
            Action::ToggleMarkedFilter => "toggle_marked_filter",
            Action::SaveMarkers => "save_markers",
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
            Action::TogglePlayback => "Play / pause",
            Action::StopPlayback => "Stop playback",
            Action::SeekForwardSmall => "Seek forward (small)",
            Action::SeekForwardLarge => "Seek forward (large)",
            Action::SeekBackwardSmall => "Seek backward (small)",
            Action::SeekBackwardLarge => "Seek backward (large)",
            Action::ToggleAutoAdvance => "Toggle auto-advance",
            Action::ToggleTimeDisplay => "Toggle elapsed/remaining",
            Action::ToggleMark => "Toggle mark on row",
            Action::ClearMarks => "Clear all marks",
            Action::ToggleMarkedFilter => "Filter to marked only",
            Action::SaveMarkers => "Save markers to file",
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
            Action::SortAscending | Action::SortDescending => "Sort",
            Action::TogglePlayback
            | Action::StopPlayback
            | Action::SeekForwardSmall
            | Action::SeekForwardLarge
            | Action::SeekBackwardSmall
            | Action::SeekBackwardLarge
            | Action::ToggleAutoAdvance
            | Action::ToggleTimeDisplay => "Playback",
            Action::ToggleMark
            | Action::ClearMarks
            | Action::ToggleMarkedFilter
            | Action::SaveMarkers => "Marks",
            Action::EnterInsertMode
            | Action::EnterNormalMode
            | Action::SearchSubmit
            | Action::ClearQuery => "Mode",
            Action::OpenSelected => "Selection",
            Action::ShowHelp | Action::Quit => "App",
        }
    }
}

/// Parse a key name string into a crossterm KeyEvent.
///
/// Handles: single chars ("j", "G"), special keys ("Space", "Esc", "Enter",
/// "Up", "Down", "Backspace", "Tab", "/", "?"), and modifier combos ("Ctrl-C",
/// "Ctrl-D", "Ctrl-U").
pub fn parse_key(s: &str) -> Option<KeyEvent> {
    // Modifier prefix: Ctrl-
    if let Some(rest) = s.strip_prefix("Ctrl-") {
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

        // Navigation
        bindings.insert((KeyCode::Char('j'), none), Action::MoveDown);
        bindings.insert((KeyCode::Char('k'), none), Action::MoveUp);
        bindings.insert((KeyCode::Down, none), Action::MoveDown);
        bindings.insert((KeyCode::Up, none), Action::MoveUp);
        bindings.insert((KeyCode::Char('G'), shift), Action::MoveToBottom);
        bindings.insert((KeyCode::Char('d'), ctrl), Action::PageDown);
        bindings.insert((KeyCode::Char('u'), ctrl), Action::PageUp);
        bindings.insert((KeyCode::Char('h'), none), Action::MoveColumnLeft);
        bindings.insert((KeyCode::Char('l'), none), Action::MoveColumnRight);

        // Sort
        bindings.insert((KeyCode::Char('o'), none), Action::SortAscending);
        bindings.insert((KeyCode::Char('O'), shift), Action::SortDescending);

        // Playback
        bindings.insert((KeyCode::Char(' '), none), Action::TogglePlayback);
        bindings.insert((KeyCode::Char('s'), none), Action::StopPlayback);
        bindings.insert((KeyCode::Right, none), Action::SeekForwardSmall);
        bindings.insert((KeyCode::Left, none), Action::SeekBackwardSmall);
        bindings.insert((KeyCode::Right, shift), Action::SeekForwardLarge);
        bindings.insert((KeyCode::Left, shift), Action::SeekBackwardLarge);
        bindings.insert((KeyCode::Char('a'), none), Action::ToggleAutoAdvance);
        bindings.insert((KeyCode::Char('t'), none), Action::ToggleTimeDisplay);

        // Marks
        bindings.insert((KeyCode::Char('m'), none), Action::ToggleMark);
        bindings.insert((KeyCode::Char('M'), shift), Action::ClearMarks);
        bindings.insert((KeyCode::Char('f'), none), Action::ToggleMarkedFilter);
        bindings.insert((KeyCode::Char('w'), none), Action::SaveMarkers);

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
    if modifiers.contains(KeyModifiers::CONTROL) {
        s.push_str("Ctrl-");
    } else if modifiers.contains(KeyModifiers::SHIFT) {
        // Shift prefix for non-char keys (arrows, etc).
        match code {
            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => {
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
        let order = ["Navigation", "Sort", "Playback", "Marks", "Mode", "Selection", "App"];
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
        assert_eq!(Action::ALL.len(), 29);
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
        let x = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(km.resolve(x), None);
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
        // '?' is not uppercase alpha, so no SHIFT added by parse_key for symbol chars.
        // But in terminal events, ? comes with SHIFT. Let's verify our keymap handles it.
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

    // --- S8-T6 tests: Action count ---

    #[test]
    fn test_action_all_count_final() {
        assert_eq!(Action::ALL.len(), 29, "Sprint 9 final count should be 29");
    }

    // --- S9-T9 tests: SaveMarkers action ---

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
}
