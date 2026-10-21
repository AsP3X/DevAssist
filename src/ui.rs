use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::app::{Action, App, Mode, RemoteState};
use crate::gitops::LocalRepo;

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn draw(frame: &mut Frame, app: &mut App) {
    app.tick = app.tick.wrapping_add(1);
    app.hits.rows.clear();
    app.hits.buttons.clear();
    app.ensure_selection();

    let area = frame.area();
    if area.width < 48 || area.height < 12 {
        frame.render_widget(Paragraph::new("Make the terminal a bit larger."), area);
        return;
    }

    let log_height = if app.log.is_empty() && !app.busy {
        0
    } else {
        7.min(area.height.saturating_sub(12) as usize) as u16
    };
    let pieces = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(2),
            Constraint::Length(log_height),
            Constraint::Length(2),
        ])
        .split(area);

    draw_header(frame, app, pieces[0]);
    draw_columns(frame, app, pieces[1]);
    draw_installed(frame, app, pieces[2]);
    draw_activity(frame, app, pieces[3]);
    if log_height > 0 {
        draw_log(frame, app, pieces[4]);
    }
    draw_footer(frame, app, pieces[5]);

    match &app.mode {
        Mode::Add => draw_add(frame, app, area),
        Mode::ConfirmRemove { .. } => draw_confirm(frame, app, area),
        Mode::Help => draw_help(frame, app, area),
        Mode::List | Mode::Filter => {}
    }
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let github = match &app.remotes_state {
        RemoteState::Loading if app.remotes.is_empty() => "loading GitHub…".to_string(),
        RemoteState::Loading => format!("{} on GitHub · refreshing", app.remotes.len()),
        RemoteState::Ready if app.owners.is_empty() && app.pinned.is_empty() => {
            "type owner/repo to add".to_string()
        }
        RemoteState::Ready => format!(
            "{} on GitHub · {} not installed",
            app.remotes.len(),
            app.missing_count()
        ),
        RemoteState::Failed(_) if app.remotes.is_empty() => "GitHub unavailable".to_string(),
        RemoteState::Failed(_) => format!("{} on GitHub · refresh failed", app.remotes.len()),
    };
    let width = area.width as usize;
    let (badge, source, path) = header_parts(app, width);
    let title = Line::from(vec![
        Span::styled(
            badge,
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(source, Style::default().fg(Color::Cyan)),
        Span::styled(format!("  {path}"), Style::default().fg(Color::DarkGray)),
    ]);
    let behind = app
        .installed
        .iter()
        .filter(|repo| repo.behind.unwrap_or(0) > 0)
        .count();
    let dirty = app.installed.iter().filter(|repo| repo.dirty).count();
    let mut meta_text = format!("  {} installed", app.installed.len());
    if behind > 0 {
        meta_text.push_str(&format!(" · {behind} behind"));
    }
    if dirty > 0 {
        meta_text.push_str(&format!(" · {dirty} dirty"));
    }
    meta_text.push_str(&format!(" · {github}"));
    let meta = Line::from(Span::styled(
        fit(&meta_text, width),
        Style::default().fg(Color::Gray),
    ));
    let third = if app.mode == Mode::Filter {
        Line::from(vec![
            Span::styled("  filter ", Style::default().fg(Color::DarkGray)),
            Span::styled(&app.list_filter, Style::default().fg(Color::White)),
            Span::styled("▌", Style::default().fg(Color::Cyan)),
            Span::styled("   esc clears", Style::default().fg(Color::DarkGray)),
        ])
    } else {
        let mut hint =
            "  arrows move · click a row or button · / filter · s sort · o open · ? help"
                .to_string();
        if app.sort == crate::app::SortMode::NeedsUpdate {
            hint.push_str(" · sorted by attention");
        }
        Line::from(Span::styled(
            fit(&hint, width),
            Style::default().fg(Color::DarkGray),
        ))
    };
    frame.render_widget(Paragraph::new(vec![title, meta, third]), area);
}

fn header_parts(app: &App, width: usize) -> (String, String, String) {
    let badge = format!(" {} ", app.title);
    let mut source = app.source_label();
    let mut path = app.root.display().to_string();
    let badge_w = UnicodeWidthStr::width(badge.as_str());
    let gaps = 4;
    if badge_w
        + gaps
        + UnicodeWidthStr::width(source.as_str())
        + UnicodeWidthStr::width(path.as_str())
        > width
    {
        path = fit_end(&path, 24.min(width / 3).max(8));
    }
    let used = badge_w + gaps + UnicodeWidthStr::width(path.as_str());
    if used + UnicodeWidthStr::width(source.as_str()) > width {
        source = clip(&source, width.saturating_sub(used));
    }
    (badge, source, path)
}

fn draw_columns(frame: &mut Frame, app: &App, area: Rect) {
    let widths = column_widths(area.width, app.mixed_owners());
    let header = padded_row(
        &["", "name", "branch", "state", "sync", "about"],
        &widths,
        area.width as usize,
    );
    frame.render_widget(
        Paragraph::new(Span::styled(header, Style::default().fg(Color::DarkGray))),
        area,
    );
}

fn draw_installed(frame: &mut Frame, app: &mut App, area: Rect) {
    let repos: Vec<LocalRepo> = app.visible_installed().into_iter().cloned().collect();
    if repos.is_empty() {
        let text = if app.installed.is_empty() {
            "  No GitHub repositories here yet. Press a or click Add."
        } else {
            "  No repository matches that filter."
        };
        frame.render_widget(
            Paragraph::new(Span::styled(text, Style::default().fg(Color::DarkGray))),
            area,
        );
        return;
    }
    let selected = app
        .selected
        .as_ref()
        .and_then(|name| repos.iter().position(|repo| &repo.folder == name))
        .unwrap_or(0);
    let height = area.height as usize;
    align_scroll(&mut app.list_scroll, selected, repos.len(), height);
    let start = app.list_scroll;
    let mixed = app.mixed_owners();
    let widths = column_widths(area.width, mixed);
    for (offset, repo) in repos.iter().enumerate().skip(start).take(height) {
        let y = area.y + (offset - start) as u16;
        let rect = Rect {
            x: area.x,
            y,
            width: area.width,
            height: 1,
        };
        let chosen = offset == selected;
        let marker = if chosen { "▌" } else { " " };
        let about = app.about(repo);
        let name = if mixed {
            format!("{}/{}", repo.owner, repo.folder)
        } else {
            repo.folder.clone()
        };
        let cells = [
            marker.to_string(),
            name,
            repo.branch.clone(),
            repo.state_label().to_string(),
            repo.sync_label(),
            about,
        ];
        let line = padded_row(
            &[
                cells[0].as_str(),
                cells[1].as_str(),
                cells[2].as_str(),
                cells[3].as_str(),
                cells[4].as_str(),
                cells[5].as_str(),
            ],
            &widths,
            area.width as usize,
        );
        let style = row_style(repo, chosen);
        frame.render_widget(Paragraph::new(Span::styled(line, style)), rect);
        if matches!(app.mode, Mode::List | Mode::Filter) {
            app.hits.rows.push((rect, repo.folder.clone()));
        }
    }
}

fn draw_activity(frame: &mut Frame, app: &App, area: Rect) {
    let spin = if app.busy || app.fetching {
        SPINNER[(app.tick as usize / 2) % SPINNER.len()]
    } else {
        " "
    };
    let style = if app.status_error {
        Style::default().fg(Color::Red)
    } else if app.busy || app.fetching {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::Gray)
    };
    let message = fit(&format!(" {spin} {}", app.status), area.width as usize);
    let detail = if !app.progress.is_empty() {
        app.progress.clone()
    } else if let Some(folder) = &app.selected {
        app.installed
            .iter()
            .find(|repo| &repo.folder == folder)
            .map(|repo| {
                if let Some(warning) = &repo.warning {
                    warning.clone()
                } else if let Some(err) = &repo.error {
                    err.clone()
                } else {
                    repo.path.display().to_string()
                }
            })
            .unwrap_or_default()
    } else if let Some(warning) = &app.catalog_warning {
        warning.clone()
    } else if let RemoteState::Failed(err) = &app.remotes_state {
        err.clone()
    } else {
        String::new()
    };
    let detail = fit(&format!("   {detail}"), area.width as usize);
    let lines = vec![
        Line::from(Span::styled(message, style)),
        Line::from(Span::styled(detail, Style::default().fg(Color::DarkGray))),
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_log(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(
            " activity ",
            Style::default().fg(Color::DarkGray),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let height = inner.height as usize;
    let lines: Vec<Line> = app
        .log
        .iter()
        .rev()
        .take(height)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|line| {
            Line::from(Span::styled(
                fit(line, inner.width as usize),
                Style::default().fg(Color::Gray),
            ))
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_footer(frame: &mut Frame, app: &mut App, area: Rect) {
    let buttons = [
        ("Add", "a", Action::Add),
        ("Update", "u", Action::UpdateOne),
        ("Update all", "U", Action::UpdateAll),
        ("Remove", "d", Action::Remove),
        ("Refresh", "r", Action::Refresh),
        ("Quit", "q", Action::Quit),
    ];
    let y = area.y;
    let mut x = area.x + 1;
    let modal = !matches!(app.mode, Mode::List | Mode::Filter);
    for (label, key, action) in buttons {
        let text = format!(" {label} {key} ");
        let width = text.len() as u16;
        if x + width >= area.x + area.width {
            break;
        }
        let rect = Rect {
            x,
            y,
            width,
            height: 1,
        };
        let hovered = app.mouse.is_some_and(|(mx, my)| contains(rect, mx, my));
        let style = if hovered && !modal {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Cyan)
        };
        frame.render_widget(Paragraph::new(text).style(style), rect);
        if !modal {
            app.hits.buttons.push((rect, action));
        }
        x += width + 1;
    }
    let hint = Rect {
        x: area.x,
        y: area.y + 1,
        width: area.width,
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            "  enter or double-click updates · r fetches · s sorts · esc cancels · d deletes the folder",
            Style::default().fg(Color::DarkGray),
        )),
        hint,
    );
}

fn draw_add(frame: &mut Frame, app: &mut App, screen: Rect) {
    let popup = centered(
        screen,
        screen.width.saturating_mul(4) / 5,
        screen.height.saturating_mul(3) / 4,
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            format!(" Add from {} ", add_source(app)),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    if inner.height < 4 {
        return;
    }
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);
    let filter = Line::from(vec![
        Span::styled(" search ", Style::default().fg(Color::DarkGray)),
        Span::styled(&app.add_filter, Style::default().fg(Color::White)),
        Span::styled("▌", Style::default().fg(Color::Cyan)),
    ]);
    frame.render_widget(Paragraph::new(filter), parts[0]);

    let remotes = app.add_list();
    if remotes.is_empty() {
        frame.render_widget(
            Paragraph::new(format!(" {}", app.add_empty_message()))
                .style(Style::default().fg(Color::DarkGray)),
            parts[1],
        );
    } else {
        let selected = app
            .add_selected
            .as_ref()
            .and_then(|name| {
                remotes
                    .iter()
                    .position(|repo| repo.id().eq_ignore_ascii_case(name))
            })
            .unwrap_or(0);
        let height = parts[1].height as usize;
        align_scroll(&mut app.add_scroll, selected, remotes.len(), height);
        for (offset, repo) in remotes.iter().enumerate().skip(app.add_scroll).take(height) {
            let y = parts[1].y + (offset - app.add_scroll) as u16;
            let rect = Rect {
                x: parts[1].x,
                y,
                width: parts[1].width,
                height: 1,
            };
            let chosen = offset == selected;
            let wide = repo.direct
                || remotes
                    .iter()
                    .any(|other| !other.owner.eq_ignore_ascii_case(&repo.owner));
            let tags = match (repo.private, repo.archived) {
                (true, true) => " private archived",
                (true, false) => " private",
                (false, true) => " archived",
                (false, false) => "",
            };
            let language = if repo.language.is_empty() {
                "        ".to_string()
            } else {
                fit(&repo.language, 8)
            };
            let pushed = if repo.pushed.is_empty() {
                String::new()
            } else {
                format!("  {}", repo.pushed)
            };
            let label = if wide {
                format!("{}/{}", repo.owner, repo.name)
            } else {
                repo.name.clone()
            };
            let text = format!(
                "{}{}{} {}{}{}",
                if chosen { "▌ " } else { "  " },
                fit(&label, if wide { 36 } else { 22 }),
                language,
                repo.description,
                pushed,
                tags
            );
            let style = if chosen {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if repo.archived {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::White)
            };
            frame.render_widget(
                Paragraph::new(Span::styled(fit(&text, rect.width as usize), style)),
                rect,
            );
            app.hits.rows.push((rect, repo.id()));
        }
    }

    draw_dialog_buttons(
        frame,
        app,
        parts[2],
        &[
            ("Clone", Action::Clone),
            ("Retry", Action::Retry),
            ("Cancel", Action::Cancel),
        ],
    );
}

fn draw_confirm(frame: &mut Frame, app: &mut App, screen: Rect) {
    let (folder, dirty) = match &app.mode {
        Mode::ConfirmRemove { folder, dirty } => (folder.clone(), *dirty),
        _ => return,
    };
    let path = app
        .installed
        .iter()
        .find(|repo| repo.folder == folder)
        .map(|repo| repo.path.display().to_string())
        .unwrap_or_default();
    let popup = centered(
        screen,
        68.min(screen.width.saturating_sub(2)),
        11.min(screen.height.saturating_sub(2)),
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(
            " Remove repository ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    let warning = if dirty {
        "This checkout has uncommitted changes. The folder and those changes will be deleted."
    } else {
        "The local folder will be deleted. The GitHub repository stays online."
    };
    let body = vec![
        Line::from(Span::styled(
            folder,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            fit(&path, inner.width as usize),
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            warning,
            Style::default().fg(if dirty { Color::Yellow } else { Color::Gray }),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Enter deletes it. Esc cancels.",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    let text_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner.height.saturating_sub(1),
    };
    frame.render_widget(Paragraph::new(body).wrap(Wrap { trim: true }), text_area);
    let buttons = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    draw_dialog_buttons(
        frame,
        app,
        buttons,
        &[("Delete", Action::Confirm), ("Cancel", Action::Cancel)],
    );
}

fn draw_help(frame: &mut Frame, app: &mut App, screen: Rect) {
    let popup = centered(
        screen,
        72.min(screen.width.saturating_sub(2)),
        20.min(screen.height.saturating_sub(2)),
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " Keys ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    let help = "\
arrows, j k    move
enter, u       update the selected repository
U              update every repository
a              add any GitHub repository
d, Delete      remove the selected folder
r, F5          fetch and refresh
s              sort by name or by attention
o              open the folder
/              filter the installed list
wheel          scroll
esc            cancel git, go back, or clear the log
q, ctrl-c      quit

In Add, type owner/repo or paste a GitHub URL.
Refresh fetches. Updates use git pull --ff-only.
Remove asks before it deletes anything.";
    let text_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner.height.saturating_sub(1),
    };
    frame.render_widget(Paragraph::new(help), text_area);
    let buttons = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    draw_dialog_buttons(frame, app, buttons, &[("Close", Action::Cancel)]);
}

fn draw_dialog_buttons(frame: &mut Frame, app: &mut App, area: Rect, buttons: &[(&str, Action)]) {
    let mut x = area.x;
    for (label, action) in buttons {
        let text = format!(" {label} ");
        let width = text.len() as u16;
        if x + width > area.x + area.width {
            break;
        }
        let rect = Rect {
            x,
            y: area.y,
            width,
            height: 1,
        };
        let hovered = app.mouse.is_some_and(|(mx, my)| contains(rect, mx, my));
        let style = if hovered {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if *action == Action::Confirm {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Cyan)
        };
        frame.render_widget(Paragraph::new(text).style(style), rect);
        app.hits.buttons.push((rect, *action));
        x += width + 1;
    }
}

fn row_style(repo: &LocalRepo, chosen: bool) -> Style {
    if chosen {
        return Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
    }
    if repo.error.is_some() {
        return Style::default().fg(Color::Red);
    }
    if !repo.ready {
        return Style::default().fg(Color::DarkGray);
    }
    if repo.dirty || repo.behind.unwrap_or(0) > 0 || repo.upstream_gone {
        Style::default().fg(Color::Yellow)
    } else if repo.ahead.unwrap_or(0) > 0 {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::White)
    }
}

fn column_widths(total: u16, wide_name: bool) -> [usize; 6] {
    let total = total as usize;
    let marker = 2;
    if total < 70 {
        let name = (if wide_name { 28 } else { 20 }).min(total.saturating_sub(marker + 10));
        let rest = total.saturating_sub(marker + name);
        return [marker, name, rest / 2, 0, rest - rest / 2, 0];
    }
    let (name, branch, sync) = if wide_name && total >= 120 {
        (36, 16, 18)
    } else if wide_name {
        (28, 14, 14)
    } else if total < 110 {
        (22, 16, 18)
    } else {
        (24, 18, 22)
    };
    let state = 8;
    let about = total.saturating_sub(marker + name + branch + state + sync);
    [marker, name, branch, state, sync, about]
}

fn add_source(app: &App) -> String {
    match app.owners.as_slice() {
        [] => "GitHub".to_string(),
        [owner] => format!("github.com/{owner}"),
        _ => "GitHub".to_string(),
    }
}

fn padded_row(cells: &[&str], widths: &[usize; 6], total: usize) -> String {
    let mut out = String::new();
    for (index, cell) in cells.iter().enumerate() {
        if widths[index] == 0 {
            continue;
        }
        out.push_str(&fit(cell, widths[index]));
    }
    fit(&out, total)
}

fn align_scroll(offset: &mut usize, selected: usize, len: usize, height: usize) {
    if height == 0 {
        *offset = 0;
        return;
    }
    let max = len.saturating_sub(height);
    if *offset > max {
        *offset = max;
    }
    if selected < *offset {
        *offset = selected;
    } else if selected >= *offset + height {
        *offset = selected + 1 - height;
    }
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.max(8).min(area.width);
    let height = height.max(4).min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

fn contains(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && y >= rect.y
        && x < rect.x.saturating_add(rect.width)
        && y < rect.y.saturating_add(rect.height)
}

fn clip(text: &str, width: usize) -> String {
    fit(text, width).trim_end().to_string()
}

fn fit_end(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    if width == 1 {
        return "…".to_string();
    }
    let mut kept = String::new();
    let mut used = 0;
    for ch in text.chars().rev() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_width > width - 1 {
            break;
        }
        kept.push(ch);
        used += ch_width;
    }
    let kept: String = kept.chars().rev().collect();
    format!("…{kept}")
}

fn fit(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let current = UnicodeWidthStr::width(text);
    if current <= width {
        return format!("{text}{}", " ".repeat(width - current));
    }
    if width == 1 {
        return "…".to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_width > width - 1 {
            break;
        }
        out.push(ch);
        used += ch_width;
    }
    out.push('…');
    let used = UnicodeWidthStr::width(out.as_str());
    if used < width {
        out.push_str(&" ".repeat(width - used));
    }
    out
}
