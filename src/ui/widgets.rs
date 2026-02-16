//! TUI widgets: search prompt, results list, preview pane, Braille waveform.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::engine::playback::PlaybackState;

use super::{App, InputMode};
use super::theme::Theme;

/// Render the search prompt bar.
pub fn render_search_prompt(app: &App, area: Rect, buf: &mut Buffer) {
    let theme = &app.theme;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border)
        .title(" Search ");

    let inner = block.inner(area);
    block.render(area, buf);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    // Status text on the right.
    let status = if app.search_in_progress {
        "searching...".to_string()
    } else if !app.results.is_empty() || app.total_matches > 0 {
        let count = if app.total_matches > 0 {
            app.total_matches
        } else {
            app.results.len()
        };
        format!("{count} matches")
    } else {
        String::new()
    };

    let status_width = status.len() as u16;
    let query_max_width = inner.width.saturating_sub(status_width + 1);

    // Mode indicator prefix.
    let (mode_label, mode_style) = match app.input_mode {
        InputMode::Normal => ("[NORMAL] ", theme.mode_normal),
        InputMode::Insert => ("[SEARCH] ", theme.mode_insert),
    };
    let mode_span = Span::styled(mode_label, mode_style);
    let mode_width = mode_label.len() as u16;

    // Render query text (truncated if too long).
    let effective_max = query_max_width.saturating_sub(mode_width);
    let display_query: String = if app.query.len() as u16 > effective_max {
        app.query.chars().take(effective_max as usize).collect()
    } else {
        app.query.clone()
    };

    let query_span = Span::styled(&display_query, theme.search_text);
    // Show cursor only in Insert mode.
    let mut spans = vec![mode_span, query_span];
    if app.input_mode == InputMode::Insert {
        spans.push(Span::styled("\u{2588}", theme.search_text)); // ▊ block cursor
    }
    let line = Line::from(spans);
    let para = Paragraph::new(line);
    para.render(inner, buf);

    // Render status on the right.
    if !status.is_empty() && status_width < inner.width {
        let status_x = inner.x + inner.width - status_width;
        let style = if app.search_in_progress {
            theme.status_text
        } else {
            theme.match_count
        };
        buf.set_string(status_x, inner.y, &status, style);
    }
}

/// Render the results list.
pub fn render_results_list(app: &App, area: Rect, buf: &mut Buffer) {
    let theme = &app.theme;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border)
        .title(" Results ");

    let inner = block.inner(area);
    block.render(area, buf);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    if app.results.is_empty() {
        let msg = if app.search_in_progress {
            "Searching..."
        } else {
            "No results"
        };
        let x = inner.x + inner.width.saturating_sub(msg.len() as u16) / 2;
        let y = inner.y + inner.height / 2;
        buf.set_string(x, y, msg, theme.status_text);
        return;
    }

    let visible_count = inner.height as usize;
    let start = app.scroll_offset;
    let end = (start + visible_count).min(app.results.len());

    for (i, idx) in (start..end).enumerate() {
        let row = &app.results[idx];
        let display = format_result_path(&row.meta.path);
        let is_selected = idx == app.selected;

        let style = if is_selected {
            theme.results_selected
        } else {
            theme.results_normal
        };

        let y = inner.y + i as u16;
        let prefix = if is_selected { "\u{25B6} " } else { "  " };
        let max_w = inner.width as usize;
        let text = format!("{prefix}{display}");
        let truncated: String = text.chars().take(max_w).collect();
        buf.set_string(inner.x, y, &truncated, style);

        // Fill rest of the line with background for selected row.
        if is_selected {
            let remaining = max_w.saturating_sub(truncated.len());
            if remaining > 0 {
                buf.set_string(
                    inner.x + truncated.len() as u16,
                    y,
                    &" ".repeat(remaining),
                    style,
                );
            }
        }
    }
}

/// Render the preview pane (metadata table + waveform).
pub fn render_preview_pane(app: &App, area: Rect, buf: &mut Buffer) {
    let theme = &app.theme;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border)
        .title(" Preview ");

    let inner = block.inner(area);
    block.render(area, buf);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let preview = match &app.preview {
        Some(p) => p,
        None => {
            if !app.results.is_empty() {
                buf.set_string(
                    inner.x + 1,
                    inner.y + inner.height / 2,
                    "Loading...",
                    theme.status_text,
                );
            }
            return;
        }
    };

    let meta = &preview.metadata;
    let max_w = inner.width as usize;
    let mut y = inner.y;

    // Metadata fields.
    let fields: &[(&str, &str)] = &[
        ("Path", &meta.path.to_string_lossy()),
        ("Vendor", &meta.vendor),
        ("Library", &meta.library),
        ("Category", &meta.category),
        ("Sound ID", &meta.sound_id),
        ("Description", &meta.description),
        ("Comment", &meta.comment),
        ("Key", &meta.key),
        ("Rating", &meta.rating),
    ];

    for &(key, value) in fields {
        if y >= inner.y + inner.height {
            break;
        }
        // Always show Path; skip empty fields otherwise.
        if key != "Path" && value.is_empty() {
            continue;
        }
        let label = format!("{key}: ");
        buf.set_string(inner.x, y, &label, theme.metadata_key);

        let value_x = inner.x + label.len() as u16;
        let value_max = max_w.saturating_sub(label.len());
        let truncated: String = value.chars().take(value_max).collect();
        buf.set_string(value_x, y, &truncated, theme.metadata_value);
        y += 1;
    }

    // BPM (special: Option<u16>).
    if let Some(bpm) = meta.bpm {
        if y < inner.y + inner.height {
            let label = "BPM: ";
            buf.set_string(inner.x, y, label, theme.metadata_key);
            buf.set_string(
                inner.x + label.len() as u16,
                y,
                &bpm.to_string(),
                theme.metadata_value,
            );
            y += 1;
        }
    }

    // Audio info (T11): duration, sample rate, format.
    if let Some(ref info) = preview.audio_info {
        if y < inner.y + inner.height {
            let label = "Duration: ";
            let value = format_duration(info.duration_secs);
            buf.set_string(inner.x, y, label, theme.metadata_key);
            buf.set_string(inner.x + label.len() as u16, y, &value, theme.metadata_value);
            y += 1;
        }
        if y < inner.y + inner.height {
            let label = "Sample Rate: ";
            let value = format!("{} Hz", info.sample_rate);
            buf.set_string(inner.x, y, label, theme.metadata_key);
            buf.set_string(inner.x + label.len() as u16, y, &value, theme.metadata_value);
            y += 1;
        }
        if y < inner.y + inner.height {
            let label = "Format: ";
            let value = info.format_display();
            buf.set_string(inner.x, y, label, theme.metadata_key);
            buf.set_string(inner.x + label.len() as u16, y, &value, theme.metadata_value);
            y += 1;
        }
    }

    // Separator.
    if y < inner.y + inner.height {
        let sep: String = "\u{2500}".repeat(max_w);
        buf.set_string(inner.x, y, &sep, theme.border);
        y += 1;
    }

    // Waveform (4-row in preview pane; full-width 8-row in two-panel mode).
    let waveform_y_start = y;
    let waveform_height = (inner.y + inner.height).saturating_sub(y) as usize;
    if waveform_height >= 4 && inner.width >= 4 {
        let wave_width = inner.width as usize;
        let wave_rows = waveform_height.min(16);
        let lines = if preview.peaks.is_empty() {
            let blank = "\u{2800}".repeat(wave_width);
            vec![blank; wave_rows]
        } else {
            render_braille_waveform_height(&preview.peaks, wave_width, wave_rows)
        };

        let positive_style = Style::default().fg(theme.waveform_positive);
        let negative_style = Style::default().fg(theme.waveform_negative);
        let half = lines.len() / 2;

        for (i, line) in lines.iter().enumerate().take(waveform_height) {
            let style = if i < half { positive_style } else { negative_style };
            buf.set_string(inner.x, y, line, style);
            y += 1;
        }

        // Playback cursor overlay.
        let waveform_rows_drawn = lines.len().min(waveform_height);
        if app.playback_position() > 0.0 && wave_width > 0 {
            render_playback_cursor(
                app.playback_position(),
                inner.x,
                waveform_y_start,
                wave_width as u16,
                waveform_rows_drawn as u16,
                buf,
                theme,
            );
        }
    }
}

/// Render a vertical playback cursor on the waveform area.
fn render_playback_cursor(
    position: f32,
    x_start: u16,
    y_start: u16,
    wave_width: u16,
    wave_rows: u16,
    buf: &mut Buffer,
    theme: &Theme,
) {
    let col = ((position * wave_width as f32) as u16).min(wave_width.saturating_sub(1));
    let cursor_x = x_start + col;

    for row in 0..wave_rows {
        let cursor_y = y_start + row;
        if let Some(cell) = buf.cell_mut((cursor_x, cursor_y)) {
            cell.set_bg(theme.playback_cursor);
        }
    }
}

/// Format seconds as "M:SS" or "H:MM:SS".
fn format_duration(secs: f64) -> String {
    let total = secs.round() as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Render the status bar at the bottom of the TUI.
pub fn render_status_bar(app: &App, area: Rect, buf: &mut Buffer) {
    let theme = &app.theme;

    if area.width == 0 || area.height == 0 {
        return;
    }

    // Fill background.
    for x in area.x..area.x + area.width {
        buf.set_string(x, area.y, " ", theme.status_text);
    }

    let width = area.width as usize;

    // Left side: playback state with progress bar.
    let left = match app.playback_state() {
        PlaybackState::Playing | PlaybackState::Paused => {
            let is_playing = app.playback_state() == PlaybackState::Playing;
            let icon = if is_playing { "\u{25B6}" } else { "\u{23F8}" };
            let name = app.playback_filename().unwrap_or_default();
            let elapsed_secs = app
                .playback
                .as_ref()
                .map(|e| e.elapsed().as_secs_f64())
                .unwrap_or(0.0);
            let duration_secs = app
                .playback
                .as_ref()
                .and_then(|e| e.duration())
                .map(|d| d.as_secs_f64());
            let time = match duration_secs {
                Some(dur) if dur > 0.0 => {
                    if app.show_remaining {
                        let remaining = (dur - elapsed_secs).max(0.0);
                        format!("-{}/{}", format_duration(remaining), format_duration(dur))
                    } else {
                        format!("{}/{}", format_duration(elapsed_secs), format_duration(dur))
                    }
                }
                _ => format_duration(elapsed_secs),
            };

            let auto = if app.auto_advance { " [AUTO]" } else { "" };
            format!(" {icon} {name} {time}{auto}")
        }
        PlaybackState::Stopped => {
            if app.auto_advance {
                " [AUTO]".to_string()
            } else {
                String::new()
            }
        }
    };

    // Middle: mark count (if any).
    let mark_count = app.mark_count();
    let marks_text = if mark_count > 0 {
        format!(" {mark_count} marked ")
    } else {
        String::new()
    };

    // Right side: result count.
    let right = if app.search_in_progress {
        "Searching... ".to_string()
    } else if app.results.is_empty() && app.total_matches == 0 {
        "No results ".to_string()
    } else {
        let displayed = app.results.len();
        let total = if app.total_matches > 0 {
            app.total_matches
        } else {
            displayed
        };
        let filter_suffix = if app.show_marked_only { " [marked] " } else { "" };
        if displayed < total {
            format!("{displayed} of {total} results{filter_suffix} ")
        } else {
            format!("{total} results{filter_suffix} ")
        }
    };

    // Render left (playback) with accent style.
    if !left.is_empty() {
        let left_max = width.saturating_sub(right.len() + marks_text.len());
        let truncated: String = left.chars().take(left_max).collect();
        buf.set_string(area.x, area.y, &truncated, theme.playback_accent);
    }

    // Render marks count (right of playback, left of results count).
    if !marks_text.is_empty() {
        let marks_x = area.x + area.width.saturating_sub(right.len() as u16 + marks_text.len() as u16);
        buf.set_string(marks_x, area.y, &marks_text, theme.table_marked);
    }

    // Render right (count) aligned right.
    let right_len = right.len() as u16;
    if right_len <= area.width {
        let right_x = area.x + area.width - right_len;
        let style = if app.search_in_progress {
            theme.status_text
        } else {
            theme.match_count
        };
        buf.set_string(right_x, area.y, &right, style);
    }
}

/// Render a spreadsheet-style metadata table.
///
/// Displays column headers + rows of metadata from `app.results`.
/// Column list comes from config or defaults.
pub fn render_metadata_table(
    app: &App,
    area: Rect,
    buf: &mut Buffer,
    columns: &[String],
) {
    let theme = &app.theme;

    if area.width == 0 || area.height == 0 {
        return;
    }

    if app.results.is_empty() {
        let msg = if app.search_in_progress {
            "Searching..."
        } else {
            "No results"
        };
        let x = area.x + area.width.saturating_sub(msg.len() as u16) / 2;
        let y = area.y + area.height / 2;
        buf.set_string(x, y, msg, theme.status_text);
        return;
    }

    let total_width = area.width as usize;

    // Resolve column defs and compute widths.
    let defs: Vec<_> = columns
        .iter()
        .filter_map(|key| crate::engine::config::column_def(key))
        .collect();

    if defs.is_empty() {
        return;
    }

    // Proportional column sizing: distribute available width by weight.
    let total_weight: u16 = defs.iter().map(|d| d.weight).sum();
    let col_widths: Vec<usize> = if total_weight == 0 {
        vec![total_width / defs.len(); defs.len()]
    } else {
        defs.iter()
            .map(|d| {
                let w = (total_width as u32 * d.weight as u32 / total_weight as u32) as usize;
                w.max(d.min_width as usize)
            })
            .collect()
    };

    // Render header row.
    let mut x = area.x;
    for (i, def) in defs.iter().enumerate() {
        let w = col_widths[i];
        // Append sort indicator if this column is sorted.
        let sort_indicator = if app.sort_column.as_deref() == Some(def.key) {
            if app.sort_ascending { " \u{25B2}" } else { " \u{25BC}" }
        } else {
            ""
        };
        let full_label = format!("{}{}", def.label, sort_indicator);
        let label: String = full_label.chars().take(w).collect();
        let style = if i == app.selected_column {
            theme.table_column_highlight
        } else {
            theme.table_header
        };
        buf.set_string(x, area.y, &label, style);
        x += w as u16;
        if x >= area.x + area.width {
            break;
        }
    }

    // Render data rows (respecting marked-only filter).
    let data_start_y = area.y + 1;
    let visible_rows = (area.height as usize).saturating_sub(1); // -1 for header

    // Build index list: either all results or only marked.
    let indices: Vec<usize> = if app.show_marked_only {
        app.results.iter().enumerate()
            .filter(|(_, r)| r.marked)
            .map(|(i, _)| i)
            .collect()
    } else {
        (0..app.results.len()).collect()
    };

    let start = app.scroll_offset.min(indices.len());
    let end = (start + visible_rows).min(indices.len());

    for (i, &idx) in indices[start..end].iter().enumerate() {
        let row = &app.results[idx];
        let is_selected = idx == app.selected;

        // Style precedence: selected > marked > played > normal.
        let is_played = app.played.contains(&row.meta.path);
        let style = if is_selected {
            theme.results_selected
        } else if row.marked {
            theme.table_marked
        } else if is_played {
            theme.table_played
        } else {
            theme.results_normal
        };

        let y = data_start_y + i as u16;
        let mut cx = area.x;

        for (j, key) in columns.iter().enumerate() {
            if j >= col_widths.len() {
                break;
            }
            let w = col_widths[j];
            let value = column_value(row, key);
            let display: String = value.chars().take(w).collect();

            let is_col_selected = j == app.selected_column;

            if is_selected || is_col_selected {
                // Fill entire cell width for background highlight.
                let cell_style = if is_selected {
                    style
                } else {
                    style.bg(theme.table_column_bg)
                };
                let padded = format!("{:<width$}", display, width = w);
                buf.set_string(cx, y, &padded, cell_style);
            } else {
                buf.set_string(cx, y, &display, style);
            }

            cx += w as u16;
            if cx >= area.x + area.width {
                break;
            }
        }

        // Fill rest of line for selected row.
        if is_selected && cx < area.x + area.width {
            let remaining = (area.x + area.width - cx) as usize;
            buf.set_string(cx, y, &" ".repeat(remaining), style);
        }
    }
}

/// Extract a display value from a TableRow for a given column key.
pub fn column_value(row: &crate::engine::TableRow, key: &str) -> String {
    match key {
        "name" => row
            .meta
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        "vendor" => row.meta.vendor.clone(),
        "library" => row.meta.library.clone(),
        "category" => row.meta.category.clone(),
        "sound_id" => row.meta.sound_id.clone(),
        "description" => row.meta.description.clone(),
        "comment" => row.meta.comment.clone(),
        "key" => row.meta.key.clone(),
        "bpm" => row
            .meta
            .bpm
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".to_string()),
        "rating" => rating_to_asterisks(&row.meta.rating),
        "subcategory" => row.meta.subcategory.clone(),
        "genre_id" => row.meta.genre_id.clone(),
        "usage_id" => row.meta.usage_id.clone(),
        "duration" => row
            .audio_info
            .as_ref()
            .map(|i| format_duration(i.duration_secs))
            .unwrap_or_else(|| "-".to_string()),
        "sample_rate" => row
            .audio_info
            .as_ref()
            .map(|i| format!("{}k", i.sample_rate / 1000))
            .unwrap_or_else(|| "-".to_string()),
        "bit_depth" => row
            .audio_info
            .as_ref()
            .map(|i| i.bit_depth.to_string())
            .unwrap_or_else(|| "-".to_string()),
        "channels" => row
            .audio_info
            .as_ref()
            .map(|i| i.channels.to_string())
            .unwrap_or_else(|| "-".to_string()),
        "date" => row.meta.date.clone(),
        "take" => row.meta.take.clone(),
        "track" => row.meta.track.clone(),
        "item" => row.meta.item.clone(),
        "format" => row
            .audio_info
            .as_ref()
            .map(|i| i.format_display())
            .unwrap_or_else(|| "-".to_string()),
        "parent_folder" => row
            .meta
            .path
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Convert a rating string to asterisks display.
///
/// If the string is already asterisks (e.g. "****"), return as-is.
/// If it's a numeric string (e.g. "3"), return that many '*' characters.
/// Empty or unparseable values return empty string.
fn rating_to_asterisks(rating: &str) -> String {
    if rating.is_empty() {
        return String::new();
    }
    // Already asterisks.
    if rating.chars().all(|c| c == '*') {
        return rating.to_string();
    }
    // Try numeric.
    if let Ok(n) = rating.trim().parse::<u8>() {
        return "*".repeat(n.min(5) as usize);
    }
    rating.to_string()
}

/// Render the full-width waveform panel below the table.
///
/// Shows 8-row Braille waveform + transport info line.
pub fn render_waveform_panel(app: &App, area: Rect, buf: &mut Buffer) {
    let theme = &app.theme;

    if area.width < 4 || area.height < 2 {
        return;
    }

    let wave_width = area.width as usize;
    // Reserve 1 row for transport info.
    let wave_rows = (area.height as usize).saturating_sub(1).min(16);

    let preview = match &app.preview {
        Some(p) => p,
        None => {
            if !app.results.is_empty() {
                // Show blank waveform area.
                let blank = "\u{2800}".repeat(wave_width);
                for row in 0..wave_rows {
                    buf.set_string(area.x, area.y + row as u16, &blank, Style::default());
                }
            }
            return;
        }
    };

    let lines = if preview.peaks.is_empty() {
        let blank = "\u{2800}".repeat(wave_width);
        vec![blank; wave_rows]
    } else {
        render_braille_waveform_height(&preview.peaks, wave_width, wave_rows)
    };

    let positive_style = Style::default().fg(theme.waveform_positive);
    let negative_style = Style::default().fg(theme.waveform_negative);
    let half = lines.len() / 2;

    for (i, line) in lines.iter().enumerate().take(wave_rows) {
        let style = if i < half { positive_style } else { negative_style };
        buf.set_string(area.x, area.y + i as u16, line, style);
    }

    // Playback cursor overlay.
    if app.playback_position() > 0.0 && wave_width > 0 {
        render_playback_cursor(
            app.playback_position(),
            area.x,
            area.y,
            wave_width as u16,
            wave_rows as u16,
            buf,
            theme,
        );
    }

    // Transport info line below waveform.
    let info_y = area.y + wave_rows as u16;
    if info_y < area.y + area.height {
        let info = match &preview.audio_info {
            Some(ai) => {
                let name = preview.metadata.path.display();
                let dur = format_duration(ai.duration_secs);
                let rate = format!("{}Hz", ai.sample_rate);
                let fmt = ai.format_display();
                format!(" {name}  {dur}  {rate}  {fmt}")
            }
            None => {
                let name = preview.metadata.path.display();
                format!(" {name}")
            }
        };
        let truncated: String = info.chars().take(area.width as usize).collect();
        buf.set_string(area.x, info_y, &truncated, theme.metadata_value);
    }
}

/// Format a path for display: show parent_folder/filename.
fn format_result_path(path: &std::path::Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let parent = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

/// Renders peak data as an 8-row bipolar Braille waveform (default).
///
/// Each Unicode Braille character (U+2800-U+28FF) is a 2×4 dot grid.
/// The renderer maps peak amplitudes (u8 0-255) to dot patterns across rows:
/// - Top half: positive amplitude
/// - Bottom half: mirrored negative amplitude
///
/// Returns 8 strings, each `width` characters wide.
/// Peaks are resampled via linear interpolation to fit any terminal width.
pub fn render_braille_waveform(peaks: &[u8], width: usize) -> Vec<String> {
    render_braille_waveform_height(peaks, width, 16)
}

/// Renders peak data as a bipolar Braille waveform with configurable height.
///
/// `height` is the number of text rows (must be even, >= 2). Each text row
/// contains 4 vertical dots, so total dot rows = height * 4.
///
/// **Stereo detection:** When `peaks.len() >= 360`, the first 180 values are
/// left channel and the next 180 are right channel. The top half renders left
/// amplitude growing upward from center, and the bottom half renders right
/// amplitude growing downward from center (asymmetric rendering). For mono
/// files the top/bottom halves are symmetric mirrors.
pub fn render_braille_waveform_height(peaks: &[u8], width: usize, height: usize) -> Vec<String> {
    if peaks.is_empty() || width == 0 || height == 0 {
        return Vec::new();
    }

    // Each Braille character is 2 columns wide, so we need width*2 samples.
    let sample_count = width * 2;

    // Detect stereo: 360+ bytes = 180L + 180R.
    let is_stereo = peaks.len() >= crate::engine::wav::STEREO_PEAK_COUNT;
    let (left_peaks, right_peaks) = if is_stereo {
        (&peaks[..crate::engine::wav::PEAK_COUNT], &peaks[crate::engine::wav::PEAK_COUNT..crate::engine::wav::STEREO_PEAK_COUNT])
    } else {
        (peaks, peaks) // mono: both halves use same data
    };

    let resampled_left = resample(left_peaks, sample_count);
    let resampled_right = resample(right_peaks, sample_count);

    // Normalize to 0.0..1.0.
    let norm_left: Vec<f64> = resampled_left.iter().map(|&v| v as f64 / 255.0).collect();
    let norm_right: Vec<f64> = resampled_right.iter().map(|&v| v as f64 / 255.0).collect();

    // Total dot rows = height * 4 (each Braille char = 4 dots tall).
    // Top half: left channel amplitude, bottom half: right channel amplitude.
    let dot_rows = height * 4;
    let half = dot_rows / 2;

    // Build dot grid: dot_grid[row][col] = true if dot is set.
    let mut dot_grid = vec![vec![false; sample_count]; dot_rows];

    for col in 0..sample_count {
        // Top half: left channel, fills from baseline (row half-1) upward.
        let left_amp = norm_left[col];
        let left_fill = (left_amp * half as f64).round() as usize;
        let left_fill = left_fill.min(half);
        for i in 0..left_fill {
            dot_grid[half - 1 - i][col] = true;
        }

        // Bottom half: right channel, fills from baseline (row half) downward.
        let right_amp = norm_right[col];
        let right_fill = (right_amp * half as f64).round() as usize;
        let right_fill = right_fill.min(half);
        for i in 0..right_fill {
            dot_grid[half + i][col] = true;
        }
    }

    // Convert dot grid to Braille characters.
    // Each Braille character covers 2 columns and 4 dot rows.
    let mut rows = Vec::with_capacity(height);
    for text_row in 0..height {
        let mut line = String::with_capacity(width);
        for col_pair in 0..width {
            let col_left = col_pair * 2;
            let col_right = col_left + 1;
            let dot_row_base = text_row * 4;

            let mut pattern: u8 = 0;
            // Braille dot numbering (Unicode standard):
            //   Left col: dots 1,2,3,7 (bits 0,1,2,6)
            //   Right col: dots 4,5,6,8 (bits 3,4,5,7)
            // Rows within character: 0,1,2,3

            // Left column dots.
            if dot_grid[dot_row_base][col_left] {
                pattern |= 1 << 0; // dot 1
            }
            if dot_row_base + 1 < dot_rows && dot_grid[dot_row_base + 1][col_left] {
                pattern |= 1 << 1; // dot 2
            }
            if dot_row_base + 2 < dot_rows && dot_grid[dot_row_base + 2][col_left] {
                pattern |= 1 << 2; // dot 3
            }
            if dot_row_base + 3 < dot_rows && dot_grid[dot_row_base + 3][col_left] {
                pattern |= 1 << 6; // dot 7
            }

            // Right column dots.
            if col_right < sample_count {
                if dot_grid[dot_row_base][col_right] {
                    pattern |= 1 << 3; // dot 4
                }
                if dot_row_base + 1 < dot_rows && dot_grid[dot_row_base + 1][col_right] {
                    pattern |= 1 << 4; // dot 5
                }
                if dot_row_base + 2 < dot_rows && dot_grid[dot_row_base + 2][col_right] {
                    pattern |= 1 << 5; // dot 6
                }
                if dot_row_base + 3 < dot_rows && dot_grid[dot_row_base + 3][col_right] {
                    pattern |= 1 << 7; // dot 8
                }
            }

            line.push(char::from_u32(0x2800 + pattern as u32).unwrap_or(' '));
        }
        rows.push(line);
    }

    rows
}

/// Linearly resample a slice of u8 values to a target length.
fn resample(data: &[u8], target_len: usize) -> Vec<u8> {
    if target_len == 0 || data.is_empty() {
        return vec![0; target_len];
    }
    if data.len() == target_len {
        return data.to_vec();
    }

    let mut result = Vec::with_capacity(target_len);
    let scale = (data.len() - 1) as f64 / (target_len - 1).max(1) as f64;

    for i in 0..target_len {
        let pos = i as f64 * scale;
        let low = pos.floor() as usize;
        let high = (low + 1).min(data.len() - 1);
        let frac = pos - low as f64;
        let val = data[low] as f64 * (1.0 - frac) + data[high] as f64 * frac;
        result.push(val.round() as u8);
    }

    result
}

/// Render the keybinding help overlay centered on the screen.
pub fn render_help_overlay(app: &App, area: Rect, buf: &mut Buffer) {
    use ratatui::style::Modifier;

    let theme = &app.theme;

    // Calculate centered overlay area (80% width, 80% height, min 40x20).
    let overlay_w = (area.width * 4 / 5).max(40).min(area.width);
    let overlay_h = (area.height * 4 / 5).max(20).min(area.height);
    let x = area.x + (area.width.saturating_sub(overlay_w)) / 2;
    let y = area.y + (area.height.saturating_sub(overlay_h)) / 2;
    let overlay_area = Rect::new(x, y, overlay_w, overlay_h);

    // Clear the overlay area.
    for row in overlay_area.y..overlay_area.y + overlay_area.height {
        for col in overlay_area.x..overlay_area.x + overlay_area.width {
            if let Some(cell) = buf.cell_mut((col, row)) {
                cell.reset();
            }
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border)
        .title(" Keybindings (? to close) ");

    let inner = block.inner(overlay_area);
    block.render(overlay_area, buf);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    // Build help lines from keymap.
    let entries = app.keymap.help_entries();
    let mut lines: Vec<Line<'_>> = Vec::new();

    for (category, bindings) in &entries {
        // Category header.
        lines.push(Line::from(Span::styled(
            format!("  {category}"),
            Style::default()
                .fg(theme.border.fg.unwrap_or(ratatui::style::Color::White))
                .add_modifier(Modifier::BOLD),
        )));

        // Deduplicate: group keys by action.
        let mut action_keys: Vec<(super::actions::Action, Vec<&str>)> = Vec::new();
        for (key_str, action) in bindings {
            if let Some(entry) = action_keys.iter_mut().find(|(a, _)| a == action) {
                entry.1.push(key_str);
            } else {
                action_keys.push((*action, vec![key_str]));
            }
        }

        for (action, keys) in &action_keys {
            let keys_str = keys.join(", ");
            lines.push(Line::from(vec![
                Span::styled(
                    format!("    {keys_str:<16}"),
                    theme.metadata_key,
                ),
                Span::styled(
                    action.description().to_string(),
                    theme.metadata_value,
                ),
            ]));
        }

        lines.push(Line::from(""));
    }

    let paragraph = Paragraph::new(lines);
    paragraph.render(inner, buf);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use crate::engine::{TableRow, UnifiedMetadata};
    use super::super::PreviewData;

    /// Helper to create a default TableRow for tests.
    fn default_table_row() -> TableRow {
        TableRow {
            meta: UnifiedMetadata::default(),
            audio_info: None,
            marked: false,
        }
    }

    fn buffer_to_string(buf: &Buffer) -> String {
        let mut s = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                s.push_str(buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "));
            }
            s.push('\n');
        }
        s
    }

    // --- T7 tests: Search prompt widget ---

    #[test]
    fn test_prompt_renders_query() {
        let mut app = App::new(Theme::default());
        app.query = "foo".to_string();
        app.cursor_pos = 3;

        let backend = TestBackend::new(40, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_search_prompt(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        let out = buffer_to_string(terminal.backend().buffer());
        assert!(out.contains("foo"), "should contain query text: {out}");
    }

    #[test]
    fn test_prompt_shows_match_count() {
        let mut app = App::new(Theme::default());
        app.total_matches = 42;
        app.results = vec![default_table_row()]; // non-empty

        let backend = TestBackend::new(40, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_search_prompt(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        let out = buffer_to_string(terminal.backend().buffer());
        assert!(out.contains("42 matches"), "should show match count: {out}");
    }

    #[test]
    fn test_prompt_shows_searching() {
        let mut app = App::new(Theme::default());
        app.search_in_progress = true;

        let backend = TestBackend::new(40, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_search_prompt(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        let out = buffer_to_string(terminal.backend().buffer());
        assert!(out.contains("searching..."), "should show searching: {out}");
    }

    #[test]
    fn test_prompt_empty_query() {
        let app = App::new(Theme::default());
        let backend = TestBackend::new(40, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_search_prompt(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        // Should render without panic.
        let out = buffer_to_string(terminal.backend().buffer());
        assert!(out.contains("Search"), "should have Search title: {out}");
    }

    // --- T8 tests: Results list widget ---

    #[test]
    fn test_results_renders_filenames() {
        let mut app = App::new(Theme::default());
        app.results = vec![
            TableRow {
                meta: UnifiedMetadata {
                    path: std::path::PathBuf::from("/a/b/kick.wav"),
                    ..Default::default()
                },
                audio_info: None,
                marked: false,
            },
            TableRow {
                meta: UnifiedMetadata {
                    path: std::path::PathBuf::from("/c/d/snare.wav"),
                    ..Default::default()
                },
                audio_info: None,
                marked: false,
            },
        ];

        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_results_list(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        let out = buffer_to_string(terminal.backend().buffer());
        assert!(out.contains("kick.wav"), "should show kick.wav: {out}");
        assert!(out.contains("snare.wav"), "should show snare.wav: {out}");
    }

    #[test]
    fn test_results_empty_shows_message() {
        let app = App::new(Theme::default());
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_results_list(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        let out = buffer_to_string(terminal.backend().buffer());
        assert!(out.contains("No results"), "should show 'No results': {out}");
    }

    #[test]
    fn test_results_path_display_format() {
        let path = std::path::Path::new("/a/b/c/file.wav");
        let display = format_result_path(path);
        assert_eq!(display, "c/file.wav");
    }

    // --- T9 tests: Preview pane ---

    #[test]
    fn test_preview_shows_populated_fields() {
        let mut app = App::new(Theme::default());
        app.preview = Some(PreviewData {
            metadata: UnifiedMetadata {
                path: std::path::PathBuf::from("/test/file.wav"),
                vendor: "Mars".to_string(),
                library: "DX100".to_string(),
                category: "LOOP".to_string(),
                ..Default::default()
            },
            peaks: vec![],
            audio_info: None,
        });
        app.results = vec![default_table_row()]; // non-empty so preview renders

        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_preview_pane(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        let out = buffer_to_string(terminal.backend().buffer());
        assert!(out.contains("Mars"), "should show vendor: {out}");
        assert!(out.contains("DX100"), "should show library: {out}");
    }

    #[test]
    fn test_preview_hides_empty_fields() {
        let mut app = App::new(Theme::default());
        app.preview = Some(PreviewData {
            metadata: UnifiedMetadata {
                path: std::path::PathBuf::from("/test/file.wav"),
                vendor: "Mars".to_string(),
                // library is empty
                ..Default::default()
            },
            peaks: vec![],
            audio_info: None,
        });
        app.results = vec![default_table_row()];

        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_preview_pane(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        let out = buffer_to_string(terminal.backend().buffer());
        assert!(!out.contains("Library:"), "should hide empty Library: {out}");
    }

    #[test]
    fn test_preview_path_always_shown() {
        let mut app = App::new(Theme::default());
        app.preview = Some(PreviewData {
            metadata: UnifiedMetadata {
                path: std::path::PathBuf::from("/test/file.wav"),
                ..Default::default()
            },
            peaks: vec![],
            audio_info: None,
        });
        app.results = vec![default_table_row()];

        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_preview_pane(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        let out = buffer_to_string(terminal.backend().buffer());
        assert!(out.contains("Path:"), "should always show Path: {out}");
    }

    #[test]
    fn test_preview_loading_placeholder() {
        let mut app = App::new(Theme::default());
        app.results = vec![default_table_row()]; // non-empty
        app.preview = None;

        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_preview_pane(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        let out = buffer_to_string(terminal.backend().buffer());
        assert!(out.contains("Loading"), "should show Loading: {out}");
    }

    #[test]
    fn test_preview_waveform_renders() {
        let mut app = App::new(Theme::default());
        app.preview = Some(PreviewData {
            metadata: UnifiedMetadata {
                path: std::path::PathBuf::from("/test/file.wav"),
                ..Default::default()
            },
            peaks: vec![128u8; 180],
            audio_info: None,
        });
        app.results = vec![default_table_row()];

        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_preview_pane(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        let out = buffer_to_string(terminal.backend().buffer());
        // Braille characters should appear.
        let has_braille = out.chars().any(|c| ('\u{2801}'..='\u{28FF}').contains(&c));
        assert!(has_braille, "should contain Braille waveform chars: {out}");
    }

    // --- Braille renderer tests (8-row default) ---

    #[test]
    fn test_braille_16row_all_zeros() {
        let peaks = vec![0u8; 180];
        let rows = render_braille_waveform(&peaks, 90);
        assert_eq!(rows.len(), 16);
        for row in &rows {
            assert_eq!(row.chars().count(), 90);
            for ch in row.chars() {
                assert_eq!(ch, '\u{2800}', "all-zero peaks should produce blank Braille");
            }
        }
    }

    #[test]
    fn test_braille_16row_all_max() {
        let peaks = vec![255u8; 180];
        let rows = render_braille_waveform(&peaks, 90);
        assert_eq!(rows.len(), 16);
        for row in &rows {
            assert_eq!(row.chars().count(), 90);
            for ch in row.chars() {
                assert_ne!(ch, '\u{2800}', "all-max peaks should produce non-blank Braille");
            }
        }
    }

    #[test]
    fn test_braille_16row_dimensions() {
        let peaks: Vec<u8> = (0..180).map(|i| (i * 3 % 256) as u8).collect();
        let rows = render_braille_waveform(&peaks, 90);
        assert_eq!(rows.len(), 16);
        for row in &rows {
            assert_eq!(row.chars().count(), 90);
        }
    }

    #[test]
    fn test_braille_16row_symmetry() {
        let peaks = vec![128u8; 180];
        let rows = render_braille_waveform(&peaks, 90);
        assert_eq!(rows.len(), 16);
        // Symmetric: row i should mirror row (15-i).
        for i in 0..8 {
            assert_eq!(rows[i], rows[15 - i], "row {i} should mirror row {}", 15 - i);
        }
    }

    // --- Braille renderer tests (4-row backward compat) ---

    #[test]
    fn test_braille_4row_backward_compat() {
        let peaks: Vec<u8> = (0..180).map(|i| (i * 3 % 256) as u8).collect();
        let rows = render_braille_waveform_height(&peaks, 90, 4);
        assert_eq!(rows.len(), 4);
        for row in &rows {
            assert_eq!(row.chars().count(), 90);
        }
    }

    #[test]
    fn test_braille_4row_all_zeros() {
        let peaks = vec![0u8; 180];
        let rows = render_braille_waveform_height(&peaks, 90, 4);
        assert_eq!(rows.len(), 4);
        for row in &rows {
            for ch in row.chars() {
                assert_eq!(ch, '\u{2800}');
            }
        }
    }

    #[test]
    fn test_braille_4row_all_max() {
        let peaks = vec![255u8; 180];
        let rows = render_braille_waveform_height(&peaks, 90, 4);
        assert_eq!(rows.len(), 4);
        for row in &rows {
            for ch in row.chars() {
                assert_ne!(ch, '\u{2800}');
            }
        }
    }

    #[test]
    fn test_braille_4row_symmetry() {
        let peaks = vec![128u8; 180];
        let rows = render_braille_waveform_height(&peaks, 90, 4);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0], rows[3], "row 0 should mirror row 3");
        assert_eq!(rows[1], rows[2], "row 1 should mirror row 2");
    }

    #[test]
    fn test_braille_resample_narrow() {
        let peaks: Vec<u8> = (0..180).map(|i| (i * 3 % 256) as u8).collect();
        let rows = render_braille_waveform(&peaks, 45);
        assert_eq!(rows.len(), 16);
        for row in &rows {
            assert_eq!(row.chars().count(), 45);
        }
    }

    #[test]
    fn test_braille_resample_wide() {
        let peaks: Vec<u8> = (0..180).map(|i| (i * 3 % 256) as u8).collect();
        let rows = render_braille_waveform(&peaks, 180);
        assert_eq!(rows.len(), 16);
        for row in &rows {
            assert_eq!(row.chars().count(), 180);
        }
    }

    #[test]
    fn test_braille_single_spike() {
        let mut peaks = vec![0u8; 180];
        peaks[90] = 255;
        let rows = render_braille_waveform(&peaks, 90);
        assert_eq!(rows.len(), 16);
        let total_blank: usize = rows
            .iter()
            .flat_map(|r| r.chars())
            .filter(|&c| c == '\u{2800}')
            .count();
        let total_chars = 90 * 8;
        assert!(
            total_blank > total_chars * 3 / 4,
            "single spike should leave most chars blank, got {total_blank}/{total_chars} blank"
        );
    }

    #[test]
    fn test_braille_empty_peaks() {
        let rows = render_braille_waveform(&[], 90);
        assert!(rows.is_empty());
    }

    #[test]
    fn test_braille_short_peaks() {
        let peaks = vec![128u8; 10];
        let rows = render_braille_waveform(&peaks, 20);
        assert_eq!(rows.len(), 16);
        for row in &rows {
            assert_eq!(row.chars().count(), 20);
        }
    }

    // --- Stereo waveform tests ---

    #[test]
    fn test_braille_stereo_asymmetric() {
        // Stereo: L=max, R=0 → top half should have dots, bottom should be blank.
        let mut peaks = vec![255u8; 180]; // Left = max
        peaks.extend_from_slice(&vec![0u8; 180]); // Right = silent
        assert_eq!(peaks.len(), 360);

        let rows = render_braille_waveform_height(&peaks, 90, 16);
        assert_eq!(rows.len(), 16);

        // Top 8 rows (left channel) should have non-blank characters.
        let top_nonblank: usize = rows[..8].iter()
            .flat_map(|r| r.chars())
            .filter(|&c| c != '\u{2800}')
            .count();
        assert!(top_nonblank > 0, "top half (left) should have waveform dots");

        // Bottom 8 rows (right channel) should be all blank.
        for (i, row) in rows[8..].iter().enumerate() {
            for ch in row.chars() {
                assert_eq!(ch, '\u{2800}', "bottom row {i} should be blank (silent right channel)");
            }
        }
    }

    #[test]
    fn test_braille_stereo_mono_compat() {
        // Mono-compat: 360 bytes with identical L/R → should look symmetric.
        let data = vec![128u8; 180];
        let mut peaks = data.clone();
        peaks.extend_from_slice(&data);
        assert_eq!(peaks.len(), 360);

        let rows = render_braille_waveform_height(&peaks, 90, 16);
        assert_eq!(rows.len(), 16);
        // Should be symmetric: row i mirrors row (15-i).
        for i in 0..8 {
            assert_eq!(rows[i], rows[15 - i], "stereo mono-compat: row {i} should mirror row {}", 15 - i);
        }
    }

    #[test]
    fn test_braille_stereo_180_still_works() {
        // Mono peaks (180 bytes) should still render symmetrically.
        let peaks = vec![128u8; 180];
        let rows = render_braille_waveform_height(&peaks, 90, 16);
        assert_eq!(rows.len(), 16);
        for i in 0..8 {
            assert_eq!(rows[i], rows[15 - i], "mono 180: row {i} should mirror row {}", 15 - i);
        }
    }

    // --- T10 tests: Playback cursor ---

    #[test]
    fn test_playback_cursor_at_start() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 4));
        // Fill with Braille characters.
        for y in 0..4 {
            for x in 0..20 {
                buf.cell_mut((x, y)).unwrap().set_symbol("\u{2800}");
            }
        }
        let theme = Theme::default();
        render_playback_cursor(0.01, 0, 0, 20, 4, &mut buf, &theme);
        // Column 0 should have cursor background.
        let cell = buf.cell((0, 0)).unwrap();
        assert_eq!(cell.bg, theme.playback_cursor, "cursor should be at column 0");
    }

    #[test]
    fn test_playback_cursor_at_middle() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 4));
        let theme = Theme::default();
        render_playback_cursor(0.5, 0, 0, 20, 4, &mut buf, &theme);
        // Column 10 should have cursor background.
        let cell = buf.cell((10, 0)).unwrap();
        assert_eq!(cell.bg, theme.playback_cursor, "cursor should be at column 10");
    }

    #[test]
    fn test_playback_cursor_at_end() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 4));
        let theme = Theme::default();
        render_playback_cursor(1.0, 0, 0, 20, 4, &mut buf, &theme);
        // Column 19 (width-1) should have cursor background.
        let cell = buf.cell((19, 0)).unwrap();
        assert_eq!(cell.bg, theme.playback_cursor, "cursor should be at column 19");
    }

    #[test]
    fn test_playback_cursor_hidden_when_zero() {
        let mut app = App::new(Theme::default());
        // playback_position() returns 0.0 by default (no engine playing).
        app.preview = Some(PreviewData {
            metadata: UnifiedMetadata {
                path: std::path::PathBuf::from("/test/file.wav"),
                ..Default::default()
            },
            peaks: vec![128u8; 180],
            audio_info: None,
        });
        app.results = vec![default_table_row()];

        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_preview_pane(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        // No cursor should be rendered when position is 0.
        // Just verify no panic and rendering succeeds.
    }

    #[test]
    fn test_playback_cursor_theme_color() {
        let theme = Theme::default();
        assert_eq!(
            theme.playback_cursor,
            ratatui::style::Color::White,
            "default cursor should be White"
        );
    }

    // --- T11 tests: Audio info in preview ---

    #[test]
    fn test_preview_shows_audio_info() {
        let mut app = App::new(Theme::default());
        app.preview = Some(PreviewData {
            metadata: UnifiedMetadata {
                path: std::path::PathBuf::from("/test/file.wav"),
                ..Default::default()
            },
            peaks: vec![],
            audio_info: Some(crate::engine::wav::AudioInfo {
                duration_secs: 3.2,
                sample_rate: 44100,
                bit_depth: 16,
                channels: 2,
            }),
        });
        app.results = vec![default_table_row()];

        let backend = TestBackend::new(50, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_preview_pane(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        let out = buffer_to_string(terminal.backend().buffer());
        assert!(out.contains("Duration:"), "should show Duration: {out}");
        assert!(out.contains("44100"), "should show sample rate: {out}");
        assert!(out.contains("16-bit stereo"), "should show format: {out}");
    }

    #[test]
    fn test_preview_no_audio_info() {
        let mut app = App::new(Theme::default());
        app.preview = Some(PreviewData {
            metadata: UnifiedMetadata {
                path: std::path::PathBuf::from("/test/file.wav"),
                ..Default::default()
            },
            peaks: vec![],
            audio_info: None,
        });
        app.results = vec![default_table_row()];

        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_preview_pane(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        let out = buffer_to_string(terminal.backend().buffer());
        // No Duration line when audio_info is None.
        assert!(!out.contains("Duration:"), "should not show Duration when None: {out}");
    }

    #[test]
    fn test_format_duration_short() {
        assert_eq!(format_duration(3.2), "0:03");
        assert_eq!(format_duration(62.0), "1:02");
        assert_eq!(format_duration(0.0), "0:00");
    }

    #[test]
    fn test_format_duration_long() {
        assert_eq!(format_duration(3661.0), "1:01:01");
    }

    // --- T12 tests: Status bar ---

    #[test]
    fn test_status_bar_playing() {
        let mut app = App::new(Theme::default());
        app.results = vec![default_table_row()];
        // We can't actually make PlaybackEngine play in tests without audio device,
        // so test the stopped case.
        let backend = TestBackend::new(60, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_status_bar(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        // Stopped: no playback text on left.
        let out = buffer_to_string(terminal.backend().buffer());
        assert!(!out.contains("\u{25B6}"), "stopped should not show play icon: {out}");
    }

    #[test]
    fn test_status_bar_result_count() {
        let mut app = App::new(Theme::default());
        app.results = (0..42).map(|_| default_table_row()).collect();
        app.total_matches = 1234;

        let backend = TestBackend::new(60, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_status_bar(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        let out = buffer_to_string(terminal.backend().buffer());
        assert!(
            out.contains("42 of 1234 results"),
            "should show count: {out}"
        );
    }

    #[test]
    fn test_status_bar_searching() {
        let mut app = App::new(Theme::default());
        app.search_in_progress = true;

        let backend = TestBackend::new(60, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_status_bar(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        let out = buffer_to_string(terminal.backend().buffer());
        assert!(out.contains("Searching..."), "should show Searching: {out}");
    }

    #[test]
    fn test_status_bar_no_results() {
        let app = App::new(Theme::default());

        let backend = TestBackend::new(60, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_status_bar(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        let out = buffer_to_string(terminal.backend().buffer());
        assert!(out.contains("No results"), "should show No results: {out}");
    }

    // --- S5-T3 tests: Two-panel layout (table + waveform) ---

    #[test]
    fn test_table_renders_column_headers() {
        let app = App::new(Theme::default());
        let columns = vec!["name".to_string(), "vendor".to_string(), "category".to_string()];

        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_metadata_table(&app, f.area(), f.buffer_mut(), &columns);
            })
            .unwrap();
        // No results → should show "No results" message.
        let out = buffer_to_string(terminal.backend().buffer());
        assert!(out.contains("No results"), "empty table should show No results: {out}");
    }

    #[test]
    fn test_table_renders_metadata_columns() {
        let mut app = App::new(Theme::default());
        app.results = vec![TableRow {
            meta: UnifiedMetadata {
                path: std::path::PathBuf::from("/test/kick.wav"),
                vendor: "Mars".to_string(),
                category: "LOOP".to_string(),
                ..Default::default()
            },
            audio_info: None,
            marked: false,
        }];
        let columns = vec!["name".to_string(), "vendor".to_string(), "category".to_string()];

        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_metadata_table(&app, f.area(), f.buffer_mut(), &columns);
            })
            .unwrap();
        let out = buffer_to_string(terminal.backend().buffer());
        assert!(out.contains("Name"), "should show Name header: {out}");
        assert!(out.contains("Vendor"), "should show Vendor header: {out}");
        assert!(out.contains("kick"), "should show filename: {out}");
        assert!(out.contains("Mars"), "should show vendor value: {out}");
    }

    #[test]
    fn test_table_selected_row_highlighted() {
        let mut app = App::new(Theme::default());
        app.results = vec![
            TableRow {
                meta: UnifiedMetadata {
                    path: std::path::PathBuf::from("/test/kick.wav"),
                    ..Default::default()
                },
                audio_info: None,
                marked: false,
            },
            TableRow {
                meta: UnifiedMetadata {
                    path: std::path::PathBuf::from("/test/snare.wav"),
                    ..Default::default()
                },
                audio_info: None,
                marked: false,
            },
        ];
        app.selected = 1;
        let columns = vec!["name".to_string()];

        let backend = TestBackend::new(40, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_metadata_table(&app, f.area(), f.buffer_mut(), &columns);
            })
            .unwrap();
        let out = buffer_to_string(terminal.backend().buffer());
        assert!(out.contains("snare"), "selected row should be visible: {out}");
    }

    #[test]
    fn test_waveform_full_width() {
        let mut app = App::new(Theme::default());
        app.preview = Some(PreviewData {
            metadata: UnifiedMetadata {
                path: std::path::PathBuf::from("/test/file.wav"),
                ..Default::default()
            },
            peaks: vec![128u8; 180],
            audio_info: None,
        });
        app.results = vec![default_table_row()];

        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_waveform_panel(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        let out = buffer_to_string(terminal.backend().buffer());
        // Should contain Braille characters (waveform rendered).
        assert!(out.contains('\u{2800}') || out.chars().any(|c| c >= '\u{2800}' && c <= '\u{28FF}'),
            "waveform panel should contain Braille chars: {out}");
    }

    #[test]
    fn test_narrow_terminal_graceful() {
        let mut app = App::new(Theme::default());
        app.results = vec![default_table_row()];
        let columns = vec!["name".to_string(), "vendor".to_string(), "category".to_string()];

        // Very narrow terminal — should not panic.
        let backend = TestBackend::new(20, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_metadata_table(&app, f.area(), f.buffer_mut(), &columns);
            })
            .unwrap();
    }

    #[test]
    fn test_column_config_from_config() {
        let cols = crate::engine::config::default_columns();
        assert!(cols.contains(&"vendor".to_string()));
        assert!(cols.contains(&"duration".to_string()));
        assert!(cols.contains(&"date".to_string()));
        // All default columns should have valid defs.
        for col in &cols {
            assert!(
                crate::engine::config::column_def(col).is_some(),
                "default column '{}' should have a ColumnDef",
                col
            );
        }
    }

    #[test]
    fn test_column_value_extracts_fields() {
        let row = TableRow {
            meta: UnifiedMetadata {
                path: std::path::PathBuf::from("/test/kick.wav"),
                vendor: "Mars".to_string(),
                bpm: Some(120),
                ..Default::default()
            },
            audio_info: Some(crate::engine::wav::AudioInfo {
                duration_secs: 2.5,
                sample_rate: 48000,
                bit_depth: 24,
                channels: 2,
            }),
            marked: false,
        };
        assert_eq!(column_value(&row, "name"), "kick");
        assert_eq!(column_value(&row, "vendor"), "Mars");
        assert_eq!(column_value(&row, "bpm"), "120");
        assert_eq!(column_value(&row, "duration"), "0:03");
        assert_eq!(column_value(&row, "sample_rate"), "48k");
    }

    #[test]
    fn test_column_value_no_audio_info() {
        let row = TableRow {
            meta: UnifiedMetadata {
                path: std::path::PathBuf::from("/test/kick.wav"),
                ..Default::default()
            },
            audio_info: None,
            marked: false,
        };
        assert_eq!(column_value(&row, "duration"), "-");
        assert_eq!(column_value(&row, "sample_rate"), "-");
    }

    // --- S5-T7 tests: Mark count in status bar ---

    #[test]
    fn test_status_bar_mark_count() {
        let mut app = App::new(Theme::default());
        app.results = (0..5).map(|_| default_table_row()).collect();
        app.results[0].marked = true;
        app.results[2].marked = true;
        app.total_matches = 5;

        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_status_bar(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        let out = buffer_to_string(terminal.backend().buffer());
        assert!(out.contains("2 marked"), "should show mark count: {out}");
    }

    #[test]
    fn test_status_bar_no_mark_count_when_zero() {
        let mut app = App::new(Theme::default());
        app.results = (0..5).map(|_| default_table_row()).collect();
        app.total_matches = 5;

        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_status_bar(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        let out = buffer_to_string(terminal.backend().buffer());
        assert!(!out.contains("marked"), "should not show mark count when zero: {out}");
    }

    #[test]
    fn test_marked_only_view_filters() {
        let mut app = App::new(Theme::default());
        app.results = vec![
            TableRow {
                meta: UnifiedMetadata {
                    path: std::path::PathBuf::from("/test/kick.wav"),
                    ..Default::default()
                },
                audio_info: None,
                marked: true,
            },
            TableRow {
                meta: UnifiedMetadata {
                    path: std::path::PathBuf::from("/test/snare.wav"),
                    ..Default::default()
                },
                audio_info: None,
                marked: false,
            },
            TableRow {
                meta: UnifiedMetadata {
                    path: std::path::PathBuf::from("/test/hat.wav"),
                    ..Default::default()
                },
                audio_info: None,
                marked: true,
            },
        ];
        app.show_marked_only = true;
        let columns = vec!["name".to_string()];

        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_metadata_table(&app, f.area(), f.buffer_mut(), &columns);
            })
            .unwrap();
        let out = buffer_to_string(terminal.backend().buffer());
        assert!(out.contains("kick"), "marked file should be visible: {out}");
        assert!(out.contains("hat"), "marked file should be visible: {out}");
        assert!(!out.contains("snare"), "unmarked file should be hidden: {out}");
    }

    // --- Rating asterisks tests ---

    #[test]
    fn test_rating_asterisks_passthrough() {
        assert_eq!(rating_to_asterisks("****"), "****");
        assert_eq!(rating_to_asterisks("*"), "*");
        assert_eq!(rating_to_asterisks("*****"), "*****");
    }

    #[test]
    fn test_rating_asterisks_numeric() {
        assert_eq!(rating_to_asterisks("3"), "***");
        assert_eq!(rating_to_asterisks("5"), "*****");
        assert_eq!(rating_to_asterisks("0"), "");
        assert_eq!(rating_to_asterisks("1"), "*");
    }

    #[test]
    fn test_rating_asterisks_empty() {
        assert_eq!(rating_to_asterisks(""), "");
    }

    #[test]
    fn test_rating_asterisks_clamps_to_5() {
        assert_eq!(rating_to_asterisks("9"), "*****");
    }

    // --- New column_value tests ---

    #[test]
    fn test_column_value_new_fields() {
        let row = TableRow {
            meta: UnifiedMetadata {
                path: std::path::PathBuf::from("/test/kick.wav"),
                date: "2024-01-15".to_string(),
                take: "67".to_string(),
                track: "1".to_string(),
                item: "12345678".to_string(),
                rating: "3".to_string(),
                ..Default::default()
            },
            audio_info: Some(crate::engine::wav::AudioInfo {
                duration_secs: 2.5,
                sample_rate: 48000,
                bit_depth: 24,
                channels: 2,
            }),
            marked: false,
        };
        assert_eq!(column_value(&row, "date"), "2024-01-15");
        assert_eq!(column_value(&row, "take"), "67");
        assert_eq!(column_value(&row, "track"), "1");
        assert_eq!(column_value(&row, "item"), "12345678");
        assert_eq!(column_value(&row, "bit_depth"), "24");
        assert_eq!(column_value(&row, "channels"), "2");
        assert_eq!(column_value(&row, "rating"), "***");
    }

    // --- S7-T4 tests: Mode indicator in search prompt ---

    #[test]
    fn test_mode_indicator_normal() {
        let app = App::new(Theme::default());
        assert_eq!(app.input_mode, super::super::InputMode::Normal);
        let backend = TestBackend::new(60, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_search_prompt(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        let out = buffer_to_string(terminal.backend().buffer());
        assert!(out.contains("[NORMAL]"), "should show [NORMAL] indicator: {out}");
    }

    #[test]
    fn test_mode_indicator_insert() {
        let mut app = App::new(Theme::default());
        app.input_mode = super::super::InputMode::Insert;
        let backend = TestBackend::new(60, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_search_prompt(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        let out = buffer_to_string(terminal.backend().buffer());
        assert!(out.contains("[SEARCH]"), "should show [SEARCH] indicator: {out}");
    }

    #[test]
    fn test_cursor_visible_in_insert_only() {
        // Normal mode: no block cursor.
        let app = App::new(Theme::default());
        let backend = TestBackend::new(60, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_search_prompt(&app, f.area(), f.buffer_mut());
            })
            .unwrap();
        let normal_out = buffer_to_string(terminal.backend().buffer());
        assert!(!normal_out.contains('\u{2588}'), "Normal mode should not show block cursor");

        // Insert mode: block cursor visible.
        let mut app2 = App::new(Theme::default());
        app2.input_mode = super::super::InputMode::Insert;
        let backend2 = TestBackend::new(60, 3);
        let mut terminal2 = Terminal::new(backend2).unwrap();
        terminal2
            .draw(|f| {
                render_search_prompt(&app2, f.area(), f.buffer_mut());
            })
            .unwrap();
        let insert_out = buffer_to_string(terminal2.backend().buffer());
        assert!(insert_out.contains('\u{2588}'), "Insert mode should show block cursor");
    }

    #[test]
    fn test_all_themes_have_mode_styles() {
        for theme in [Theme::telescope(), Theme::ableton(), Theme::soundminer()] {
            assert_ne!(
                theme.mode_normal,
                Style::default(),
                "{} mode_normal should be styled",
                theme.name,
            );
            assert_ne!(
                theme.mode_insert,
                Style::default(),
                "{} mode_insert should be styled",
                theme.name,
            );
            // Normal and insert should be visually distinct.
            assert_ne!(
                theme.mode_normal, theme.mode_insert,
                "{} mode_normal and mode_insert should differ",
                theme.name,
            );
        }
    }

}
