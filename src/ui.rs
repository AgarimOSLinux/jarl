use unicode_width::UnicodeWidthStr;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::app::{App, AppMode};
use crate::logger::log_path;
use crate::player::{PlayerStatus, MAX_RECONNECT_ATTEMPTS};

// ── Entry point ───────────────────────────────────────────────────────────────

/// Pad `s` to exactly `width` display columns, truncating with '…' if needed.
fn pad_display(s: &str, width: usize) -> String {
    let w = UnicodeWidthStr::width(s);
    if w == width { return s.to_string(); }
    if w > width {
        // Truncate to width-1 and add ellipsis
        let mut result = String::new();
        let mut cur = 0;
        for ch in s.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
            if cur + cw > width - 1 { break; }
            result.push(ch);
            cur += cw;
        }
        result.push('…');
        // Pad remaining if needed
        let final_w = UnicodeWidthStr::width(result.as_str());
        if final_w < width { result.push_str(&" ".repeat(width - final_w)); }
        result
    } else {
        // Pad with spaces
        format!("{}{}", s, " ".repeat(width - w))
    }
}

pub fn draw(f: &mut Frame, app: &App) {
    let t = &app.theme;
    let bg = if app.transparent { ratatui::style::Color::Reset } else { t.bg };
    f.render_widget(Block::default().style(Style::default().bg(bg).fg(t.fg)), f.area());

    let has_error = matches!(app.player.status(), PlayerStatus::Error(_));

    // Zen mode: hide header and help bar
    let mut constraints: Vec<Constraint> = vec![];
    if !app.zen_mode { constraints.push(Constraint::Length(5)); }
    if has_error     { constraints.push(Constraint::Length(3)); }
    constraints.push(Constraint::Min(3));
    if app.status_msg.is_some() { constraints.push(Constraint::Length(1)); }
    if !app.zen_mode && (!app.hide_help || app.chiquito_mode) { constraints.push(Constraint::Length(3)); }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(f.area());

    let mut ci = 0;
    if !app.zen_mode { draw_header(f, app, chunks[ci]); ci += 1; }
    if has_error     { draw_error_bar(f, app, chunks[ci]); ci += 1; }
    // Content area: optionally split for visualizer
    let content_area = chunks[ci]; ci += 1;
    let list_area;
    if app.show_vis {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(10), Constraint::Length(26)])
            .split(content_area);
        list_area = cols[0];
        draw_station_list(f, app, cols[0]);
        draw_visualizer(f, app, cols[1]);
    } else {
        list_area = content_area;
        draw_station_list(f, app, content_area);
    }
    if app.status_msg.is_some() { draw_status_line(f, app, chunks[ci]); ci += 1; }
    if !app.zen_mode && (!app.hide_help || app.chiquito_mode) { draw_help_bar(f, app, chunks[ci]); }
    match &app.mode {
        AppMode::ThemePicker   { selected } => draw_theme_picker(f, app, *selected),
        AppMode::ConfirmDelete { index }    => draw_confirm_delete(f, app, *index),
        AppMode::Search                     => draw_search_bar(f, app, list_area),
        AppMode::History       { selected } => draw_history(f, app, *selected),
        AppMode::Normal => {}
    }
}

// ── Header ────────────────────────────────────────────────────────────────────

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let t      = &app.theme;
    let bg     = if app.transparent { ratatui::style::Color::Reset } else { t.bg };
    let status = app.player.status();
    let vol    = app.player.volume();

    let (sname, sgenre) = app
        .current
        .map(|i| (app.stations[i].name.as_str(), app.stations[i].genre.as_str()))
        .unwrap_or(("No station selected", "—"));

    let is_fav = app.current
        .map(|i| app.favorites.contains(&app.stations[i].name))
        .unwrap_or(false);

    let (badge, badge_col) = match &status {
        PlayerStatus::Idle       => ("◌ idle".to_string(),       t.muted),
        PlayerStatus::Connecting => ("◌ connecting…".to_string(), t.warning),
        PlayerStatus::Playing    => {
            let s = if app.tick % 8 < 4 { "● LIVE" } else { "○ LIVE" };
            (s.to_string(), t.success)
        }
        PlayerStatus::Paused => ("⏸ paused".to_string(), t.warning),
        PlayerStatus::Reconnecting(n) => {
            let dot = if app.tick % 8 < 4 { "◌" } else { "○" };
            (format!("{dot} reconnecting… ({n}/{MAX_RECONNECT_ATTEMPTS})"), t.warning)
        }
        PlayerStatus::Error(_) => ("✗ error".to_string(), t.error),
    };

    let filled  = (vol * 12.0).round() as usize;
    let vol_bar = format!(
        "{}{} {:3}%",
        "█".repeat(filled.min(12)),
        "░".repeat(12usize.saturating_sub(filled)),
        (vol * 100.0).round() as u8,
    );
    let wave_frames = ["▁▂▄▅▆▅▄▂", "▂▄▅▆▅▄▂▁", "▄▅▆▅▄▂▁▂", "▅▆▅▄▂▁▂▄"];
    let wave = if matches!(status, PlayerStatus::Playing) {
        wave_frames[app.tick as usize % wave_frames.len()]
    } else {
        "▁▁▁▁▁▁▁▁"
    };

    let fav_star = if is_fav {
        Span::styled("★ ", Style::default().fg(t.warning).add_modifier(Modifier::BOLD))
    } else {
        Span::raw("")
    };

    let filter_badge = if app.fav_filter {
        Span::styled(" [★ favs] ", Style::default().fg(t.warning))
    } else {
        Span::raw("")
    };

    let inner_w = area.width.saturating_sub(2) as usize;

    let track_line = {
        let title_opt = app.track_title.lock().ok()
            .and_then(|g| g.clone());
        let available_w = area.width.saturating_sub(7) as usize;
        let has_title = title_opt.as_ref().map(|t| !t.is_empty()).unwrap_or(false);
        let left = match &title_opt {
            Some(title) if !title.is_empty() => {
                if title.chars().count() > available_w {
                    // Pad with spaces so the scroll looks continuous
                    let padded = format!("{title}   ");
                    let chars: Vec<char> = padded.chars().collect();
                    let len = chars.len();
                    let offset = app.ticker_offset % len;
                    chars[offset..]
                        .iter()
                        .chain(chars[..offset].iter())
                        .take(available_w)
                        .collect::<String>()
                } else {
                    title.clone()
                }
            }
            _ => sgenre.to_string(),
        };
        let left_styled = if has_title {
            Style::default().fg(t.fg)
        } else {
            Style::default().fg(t.muted)
        };

        // Right-pad the left content to fill the available width.
        let left_w = inner_w.saturating_sub(5);
        let left_padded = if left_w == 0 { String::new() } else { pad_display(&left, left_w) };
        Line::from(vec![
            Span::raw("     "),
            Span::styled(left_padded, left_styled),
        ])
    };

    let wave_vol_line = {
        let left_text_w: usize = format!("  {wave}  {badge}   vol: {vol_bar}").chars().count();
        let left = vec![
            Span::styled(format!("  {wave}  "), Style::default().fg(t.accent)),
            Span::styled(badge, Style::default().fg(badge_col).add_modifier(Modifier::BOLD)),
            Span::raw("   "),
            Span::styled("vol: ", Style::default().fg(t.muted)),
            Span::styled(vol_bar, Style::default().fg(t.accent)),
        ];
        let pad_w = inner_w.saturating_sub(left_text_w);
        let mut spans = left;
        spans.push(Span::raw(" ".repeat(pad_w)));
        Line::from(spans)
    };

    let lines = vec![
        Line::from(vec![
            Span::styled("  ♪  ", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
            fav_star,
            Span::styled(sname, Style::default().fg(t.fg).add_modifier(Modifier::BOLD)),
            filter_badge,
        ]),
        track_line,
        wave_vol_line,
    ];

    let title = Span::styled(
        " jarl ", Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
    );
    let block = Block::default()
        .title(title).title_alignment(Alignment::Center)
        .borders(Borders::ALL).border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.border))
        .style(Style::default().bg(bg));

    f.render_widget(Paragraph::new(lines).block(block).style(Style::default().bg(bg)), area);
}

// ── Error bar ─────────────────────────────────────────────────────────────────

fn draw_error_bar(f: &mut Frame, app: &App, area: Rect) {
    let t  = &app.theme;
    let bg = if app.transparent { ratatui::style::Color::Reset } else { t.bg };
    let msg = if let PlayerStatus::Error(ref e) = app.player.status() { e.clone() } else { return; };
    let log = log_path();
    let text = format!("  ✗ {msg}  ·  log: {}", log.display());
    f.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(t.error).bg(bg))
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded)
                .border_style(Style::default().fg(t.error))),
        area,
    );
}

// ── Station list ──────────────────────────────────────────────────────────────

fn draw_station_list(f: &mut Frame, app: &App, area: Rect) {
    let t       = &app.theme;
    let bg      = if app.transparent { ratatui::style::Color::Reset } else { t.bg };
    let visible = app.visible_stations();

    // ── Dynamic column widths ─────────────────────────────────────────────────
    // Fixed overhead per row:
    //   borders: 2  │  play indicator: 3  │  fav indicator: 2
    //   bitrate col: 12 (3 padding + 9 "NNNN kbps")  │  trailing space: 1
    // The remaining width is split 60% name / 40% genre (minimum 10 / 6).
    let fixed    = 2 + 3 + 2 + 12 + 1;
    let flexible = (area.width as usize).saturating_sub(fixed);
    let name_w   = (flexible * 60 / 100).max(10);
    let genre_w  = flexible.saturating_sub(name_w).max(6);

    let items: Vec<ListItem> = visible.iter().map(|(real_idx, s)| {
        let playing  = app.current == Some(*real_idx);
        let is_fav   = app.favorites.contains(&s.name);

        let play_ind = if playing { "▶ " } else { "  " };
        let play_col = if playing { t.success } else { t.bg };
        let fav_ind  = if is_fav  { "★ " } else { "  " };
        let fav_col  = if is_fav  { t.warning } else { t.muted };
        let name_sty = if playing {
            Style::default().fg(t.success).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.fg)
        };

        ListItem::new(Line::from(vec![
            Span::styled(format!(" {play_ind}"), Style::default().fg(play_col)),
            Span::styled(fav_ind,                Style::default().fg(fav_col)),
            Span::styled(pad_display(&s.name,  name_w),  name_sty),
            Span::styled(pad_display(&s.genre, genre_w), Style::default().fg(t.muted)),
            Span::styled(format!("   {}", s.bitrate.display()), Style::default().fg(t.accent)),
            Span::raw(" "),
        ]))
    }).collect();

    let title = if !app.search_query.is_empty() {
        format!(" Stations  «{}» ({}/{}) ", app.search_query, visible.len(), app.stations.len())
    } else if app.fav_filter {
        format!(" Stations ★ ({}/{}) ", visible.len(), app.stations.len())
    } else {
        format!(" Stations ({}) ", app.stations.len())
    };

    let mut state = ListState::default();
    state.select(Some(app.sel));

    let list = List::new(items)
        .block(Block::default()
            .title(Span::styled(title, Style::default().fg(t.accent).add_modifier(Modifier::BOLD)))
            .borders(Borders::ALL).border_type(BorderType::Rounded)
            .border_style(Style::default().fg(t.border))
            .style(Style::default().bg(bg)))
        .highlight_style(Style::default().bg(t.highlight).add_modifier(Modifier::BOLD))
        .highlight_symbol("");

    f.render_stateful_widget(list, area, &mut state);
}

// ── Status line ───────────────────────────────────────────────────────────────

fn draw_status_line(f: &mut Frame, app: &App, area: Rect) {
    let t  = &app.theme;
    let bg = if app.transparent { ratatui::style::Color::Reset } else { t.bg };
    if let Some((ref msg, _)) = app.status_msg {
        f.render_widget(
            Paragraph::new(format!("  {msg}"))
                .style(Style::default().fg(t.success).bg(bg)),
            area,
        );
    }
}

// ── Help bar ──────────────────────────────────────────────────────────────────

fn draw_help_bar(f: &mut Frame, app: &App, area: Rect) {
    use crate::config::Keybindings;
    let t  = &app.theme;
    let bg = if app.transparent { ratatui::style::Color::Reset } else { t.bg };
    let kb = &app.config.keybindings;

    if app.chiquito_mode {
        let trumpet = if app.tick % 8 < 4 { "🎺" } else { "🎷" };
        let quote_text = crate::quotes::QUOTES[app.chiquito_bar_idx];

        let inner_w = area.width.saturating_sub(6) as usize;
        let padded  = format!("{quote_text}     ");
        let chars: Vec<char> = padded.chars().collect();
        let len = chars.len();
        let scrolled: String = if quote_text.chars().count() > inner_w {
            let offset = app.ticker_offset % len;
            chars[offset..].iter().chain(chars[..offset].iter()).take(inner_w).collect()
        } else {
            quote_text.to_string()
        };

        let label = format!(" {trumpet} Chiquito  ");
        let spans = vec![
            Span::styled(label, Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
            Span::styled(scrolled, Style::default().fg(t.fg)),
        ];
        f.render_widget(
            Paragraph::new(Line::from(spans)).block(
                Block::default().borders(Borders::ALL).border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(t.accent))
                    .style(Style::default().bg(bg)),
            ),
            area,
        );
    } else {
        // ── Normal mode: keyboard shortcuts ───────────────────────────────────
        let d = |s: &str| Keybindings::display(s);
        let pairs: Vec<(String, &str)> = vec![
            (format!("↑↓/{}/{}", d(&kb.nav_up), d(&kb.nav_down)), "nav"),
            (d(&kb.play),        "play"),
            (d(&kb.pause),       "pause"),
            (format!("{}/{}", d(&kb.prev_station), d(&kb.next_station)), "zap"),
            (format!("{}/{}", d(&kb.volume_up), d(&kb.volume_down)), "vol"),
            ("/".into(),         "search"),
            (d(&kb.favourite),   "fav"),
            (d(&kb.fav_filter),  "filter"),
            (d(&kb.delete),      "del"),
            (d(&kb.theme),       "theme"),
            (d(&kb.history),     "history"),
            (d(&kb.toggle_notify), "notify"),
            (d(&kb.zen),         "zen"),
            (d(&kb.hide_help),   "hide"),
            (d(&kb.visualizer),  "vis"),
            (d(&kb.transparent), "transp"),
            (d(&kb.reload),      "reload"),
            (d(&kb.reload_themes), "themes"),
            (d(&kb.chiquito),    "chiquito"),
            (d(&kb.quit),        "quit"),
        ];

        let mut spans: Vec<Span> = vec![Span::raw(" ")];
        for (i, (key, desc)) in pairs.iter().enumerate() {
            if i > 0 { spans.push(Span::styled(" · ", Style::default().fg(t.border))); }
            spans.push(Span::styled(key.clone(), Style::default().fg(t.accent).add_modifier(Modifier::BOLD)));
            spans.push(Span::styled(format!(" {desc}"), Style::default().fg(t.muted)));
        }
        f.render_widget(
            Paragraph::new(Line::from(spans)).block(
                Block::default().borders(Borders::ALL).border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(t.border))
                    .style(Style::default().bg(bg)),
            ),
            area,
        );
    }
}

// ── Theme picker modal ────────────────────────────────────────────────────────

fn draw_theme_picker(f: &mut Frame, app: &App, selected: usize) {
    let t  = &app.theme;
    let bg = if app.transparent { ratatui::style::Color::Reset } else { t.bg };

    // Layout: list modal on top, hint bar below it (outside the box).
    // This avoids the hint overlapping the last list item.
    let total_h  = (f.area().height as f32 * 0.70) as u16;
    let modal_h  = total_h.saturating_sub(3).max(6);
    let modal_w  = (f.area().width as f32 * 0.36) as u16;

    let x = (f.area().width.saturating_sub(modal_w) / 2) + 2;
    let y = (f.area().height.saturating_sub(modal_h) / 2) + 2;

    let modal_area = Rect { x, y, width: modal_w, height: modal_h };

    f.render_widget(Clear, modal_area);

    // ── Items ─────────────────────────────────────────────────────────────────
    let items: Vec<ListItem> = app.themes.iter().map(|th| {
        let active = th.name == app.theme.name;
        let pfx    = if active { "● " } else { "  " };
        let sty    = if active {
            Style::default().fg(t.success).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.fg)
        };
        ListItem::new(Span::styled(format!("{pfx}{}", th.name), sty))
    }).collect();

    // ── Scroll offset ─────────────────────────────────────────────────────────
    // ListState is created fresh each frame, so we must compute the offset
    // ourselves to keep the selected item in the visible window.
    let inner_h = modal_h.saturating_sub(2) as usize; // subtract top+bottom border
    let offset  = if selected >= inner_h {
        selected + 1 - inner_h
    } else {
        0
    };

    let mut state = ListState::default();
    state.select(Some(selected));
    *state.offset_mut() = offset;

    let list = List::new(items)
        .block(Block::default()
            .title(Span::styled(
                format!(" Themes  {}/{} ", selected + 1, app.themes.len()),
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            ))
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL).border_type(BorderType::Double)
            .border_style(Style::default().fg(t.accent))
            .style(Style::default().bg(bg)))
        .highlight_style(Style::default().bg(t.highlight).add_modifier(Modifier::BOLD))
        .highlight_symbol("");

    f.render_stateful_widget(list, modal_area, &mut state);
}

// ── Spectrum visualizer ───────────────────────────────────────────────────────

fn draw_visualizer(f: &mut Frame, app: &App, area: Rect) {
    let t  = &app.theme;
    let bg = if app.transparent { ratatui::style::Color::Reset } else { t.bg };

    let inner_h = area.height.saturating_sub(2) as usize;
    let inner_w = area.width.saturating_sub(2) as usize;

    if inner_h == 0 || inner_w == 0 || app.spectrum.is_empty() {
        f.render_widget(
            Block::default()
                .title(Span::styled(" ♫ ", Style::default().fg(t.muted)))
                .borders(Borders::ALL).border_type(BorderType::Rounded)
                .border_style(Style::default().fg(t.border))
                .style(Style::default().bg(bg)),
            area,
        );
        return;
    }

    let bar_w: usize = 2;
    let gap_w: usize = 1;
    let stride  = bar_w + gap_w;
    let n_bars  = (inner_w / stride).max(1);

    let spectrum = &app.spectrum;
    let bars: Vec<f32> = (0..n_bars).map(|i| {
        let t0 = i as f32 / n_bars as f32;
        let t1 = (i + 1) as f32 / n_bars as f32;
        let lo = (t0 * spectrum.len() as f32) as usize;
        let hi = ((t1 * spectrum.len() as f32) as usize + 1).min(spectrum.len());
        if lo < hi { spectrum[lo..hi].iter().sum::<f32>() / (hi - lo) as f32 } else { 0.0 }
    }).collect();

    fn to_rgb(c: ratatui::style::Color) -> (u8, u8, u8) {
        match c { ratatui::style::Color::Rgb(r, g, b) => (r, g, b), _ => (128, 128, 128) }
    }
    let (ar, ag, ab) = to_rgb(t.accent);
    let (sr, sg, sb) = to_rgb(t.success);
    let (mr, mg, mb) = to_rgb(t.muted);
    let lerp = |a: u8, b: u8, f: f32| -> u8 {
        (a as f32 + (b as f32 - a as f32) * f).round().clamp(0.0, 255.0) as u8
    };

    let mut lines: Vec<Line> = (0..inner_h).map(|row| {
        let row_from_bottom = inner_h - 1 - row;
        let height_f = row_from_bottom as f32 / inner_h.max(1) as f32;
        let mut spans: Vec<Span> = Vec::with_capacity(inner_w);
        for (bi, &mag) in bars.iter().enumerate() {
            let bar_eighths = (mag * inner_h as f32 * 8.0) as usize;
            let full_rows   = bar_eighths / 8;
            let partial     = bar_eighths % 8;
            let freq_f      = bi as f32 / n_bars.max(1) as f32;
            let hr = lerp(ar, mr, freq_f);
            let hg = lerp(ag, mg, freq_f);
            let hb = lerp(ab, mb, freq_f);
            let vr = lerp(hr, sr, height_f * 0.6);
            let vg = lerp(hg, sg, height_f * 0.6);
            let vb = lerp(hb, sb, height_f * 0.6);
            let bar_color = ratatui::style::Color::Rgb(vr, vg, vb);
            let ch = if row_from_bottom < full_rows { "█" }
                     else if row_from_bottom == full_rows && partial > 0 {
                         ["▁","▂","▃","▄","▅","▆","▇","█"][partial - 1]
                     } else { " " };
            let is_active = row_from_bottom < full_rows
                || (row_from_bottom == full_rows && partial > 0);
            for _ in 0..bar_w {
                let style = if is_active {
                    Style::default().fg(bar_color).bg(bg)
                } else {
                    Style::default().fg(bg).bg(bg)
                };
                spans.push(Span::styled(if is_active { ch } else { " " }, style));
            }
            if bi < n_bars - 1 {
                for _ in 0..gap_w {
                    spans.push(Span::styled(" ", Style::default().bg(bg)));
                }
            }
        }
        Line::from(spans)
    }).collect();

    // Idle pulse animation
    if app.spectrum.iter().all(|&s| s < 0.01) {
        let mid = inner_h / 2;
        if mid < lines.len() {
            let phase = (app.tick as usize) % (n_bars * 2);
            let spans: Vec<Span> = (0..n_bars).flat_map(|i| {
                let dist = if i < phase { phase - i } else { i - phase };
                let intensity = 1.0_f32 - (dist as f32 / n_bars as f32).min(1.0);
                let r = lerp(0, ar, intensity * 0.4);
                let g = lerp(0, ag, intensity * 0.4);
                let b = lerp(0, ab, intensity * 0.4);
                let col = ratatui::style::Color::Rgb(r, g, b);
                let ch  = if intensity > 0.1 { "▄" } else { " " };
                let mut v: Vec<Span> = (0..bar_w)
                    .map(|_| Span::styled(ch, Style::default().fg(col).bg(bg)))
                    .collect();
                if i < n_bars - 1 {
                    v.push(Span::styled(" ", Style::default().bg(bg)));
                }
                v
            }).collect();
            lines[mid] = Line::from(spans);
        }
    }

    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title(Span::styled(" ♫ ", Style::default().fg(t.muted)))
                .borders(Borders::ALL).border_type(BorderType::Rounded)
                .border_style(Style::default().fg(t.border))
                .style(Style::default().bg(bg)),
        ),
        area,
    );
}

// ── Delete confirmation modal ─────────────────────────────────────────────────

fn draw_confirm_delete(f: &mut Frame, app: &App, index: usize) {
    let t    = &app.theme;
    let bg   = if app.transparent { ratatui::style::Color::Reset } else { t.bg };
    let name = &app.stations[index].name;
    let area = centered_rect(60, 40, f.area());
    f.render_widget(Clear, area);

    let lines = vec![
        Line::from(vec![]),
        Line::from(vec![
            Span::styled("  Delete station: ", Style::default().fg(t.muted)),
            Span::styled(name.as_str(), Style::default().fg(t.error).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![]),
        Line::from(vec![
            Span::styled("  This will remove it from stations.toml.", Style::default().fg(t.muted)),
        ]),
        Line::from(vec![]),
        Line::from(vec![
            Span::styled("  Press ", Style::default().fg(t.muted)),
            Span::styled("y", Style::default().fg(t.error).add_modifier(Modifier::BOLD)),
            Span::styled(" to confirm, any other key to cancel.", Style::default().fg(t.muted)),
        ]),
    ];

    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title(Span::styled(" ⚠  Confirm Delete ", Style::default().fg(t.error).add_modifier(Modifier::BOLD)))
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL).border_type(BorderType::Double)
                .border_style(Style::default().fg(t.error))
                .style(Style::default().bg(bg)),
        ),
        area,
    );
}

// ── Search bar overlay ────────────────────────────────────────────────────────
//
// Displayed as a small box anchored to the bottom-left of the station list.
// The station list filters live as the user types.

fn draw_search_bar(f: &mut Frame, app: &App, list_area: Rect) {
    let t  = &app.theme;
    let bg = if app.transparent { ratatui::style::Color::Reset } else { t.bg };

    let bar_w  = list_area.width.min(52);
    let bar_h  = 3u16;
    let bar_x  = list_area.x + 1;
    let bar_y  = list_area.y + list_area.height.saturating_sub(bar_h + 1);
    let area   = Rect { x: bar_x, y: bar_y, width: bar_w, height: bar_h };

    // Blinking block cursor
    let cursor = if app.tick % 8 < 4 { "█" } else { " " };
    let inner_w = bar_w.saturating_sub(4) as usize;   // border + padding
    let query   = &app.search_query;

    // Scroll the query text so the cursor is always visible.
    let visible: String = if query.chars().count() + 1 > inner_w {
        let skip = query.chars().count() + 1 - inner_w;
        query.chars().skip(skip).collect()
    } else {
        query.clone()
    };

    let count = app.visible_stations().len();
    let hint  = format!("  {count} match{} ", if count == 1 { "" } else { "es" });

    f.render_widget(ratatui::widgets::Clear, area);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" /", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
            Span::styled(visible,          Style::default().fg(t.fg)),
            Span::styled(cursor,           Style::default().fg(t.accent)),
        ]))
        .block(
            Block::default()
                .title(Span::styled(
                    " Search ",
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                ))
                .title_alignment(Alignment::Left)
                .title_bottom(Span::styled(hint, Style::default().fg(t.muted)))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(t.accent))
                .style(Style::default().bg(bg)),
        ),
        area,
    );
}

// ── History modal ─────────────────────────────────────────────────────────────

fn draw_history(f: &mut Frame, app: &App, selected: usize) {
    let t  = &app.theme;
    let bg = if app.transparent { ratatui::style::Color::Reset } else { t.bg };

    let modal_w = (f.area().width as f32 * 0.70) as u16;
    let modal_h = (f.area().height as f32 * 0.75) as u16;
    let x = f.area().width.saturating_sub(modal_w) / 2;
    let y = f.area().height.saturating_sub(modal_h) / 2;
    let area = Rect { x, y, width: modal_w, height: modal_h };

    f.render_widget(Clear, area);

    if app.history.is_empty() {
        f.render_widget(
            Paragraph::new("\n  No history yet — play a station to start recording.")
                .style(Style::default().fg(t.muted))
                .block(
                    Block::default()
                        .title(Span::styled(" ⏴ History ", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)))
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL).border_type(BorderType::Double)
                        .border_style(Style::default().fg(t.accent))
                        .style(Style::default().bg(bg)),
                ),
            area,
        );
        return;
    }

    let inner_h = modal_h.saturating_sub(2) as usize;
    let offset  = if selected >= inner_h { selected + 1 - inner_h } else { 0 };

    let items: Vec<ListItem> = app.history.iter().enumerate().map(|(i, e)| {
        let is_sel = i == selected;
        let sty = if is_sel {
            Style::default().fg(t.success).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.fg)
        };
        // Fixed column widths within the modal.
        let avail  = modal_w.saturating_sub(6) as usize; // borders + padding
        let name_w = (avail * 60 / 100).max(10);
        let genre_w = avail.saturating_sub(name_w).max(6);
        ListItem::new(Line::from(vec![
            Span::styled(format!(" {:>2}. ", i + 1), Style::default().fg(t.muted)),
            Span::styled(pad_display(&e.name,  name_w),  sty),
            Span::styled(pad_display(&e.genre, genre_w), Style::default().fg(t.muted)),
        ]))
    }).collect();

    let mut state = ListState::default();
    state.select(Some(selected));
    *state.offset_mut() = offset;

    let title = format!(" ⏴ History ({}/{}) ", selected + 1, app.history.len());
    let list = List::new(items)
        .block(
            Block::default()
                .title(Span::styled(title, Style::default().fg(t.accent).add_modifier(Modifier::BOLD)))
                .title_alignment(Alignment::Center)
                .title_bottom(Span::styled(
                    "  ⏎ play · Esc close  ",
                    Style::default().fg(t.muted),
                ))
                .borders(Borders::ALL).border_type(BorderType::Double)
                .border_style(Style::default().fg(t.accent))
                .style(Style::default().bg(bg)),
        )
        .highlight_style(Style::default().bg(t.highlight).add_modifier(Modifier::BOLD))
        .highlight_symbol("");

    f.render_stateful_widget(list, area, &mut state);
}

// ── Utilities ─────────────────────────────────────────────────────────────────

pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vert[1])[1]
}
