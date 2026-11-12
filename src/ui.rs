use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::app::{Action, App, MenuItem, Mode, RemoteState, Row, UpdateState, group_key};
use crate::atoms::sys::paths;
use crate::atoms::task::job::{ItemState, clock, split_progress};
use crate::gitops::LocalRepo;
use crate::pathprompt::Purpose;
use crate::settings::{Field, PROTOCOLS};

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn draw(frame: &mut Frame, app: &mut App) {
    app.tick = app.tick.wrapping_add(1);
    app.hits.rows.clear();
    app.hits.buttons.clear();
    app.ensure_selection();

    let area = frame.area();
    if area.width < 48 || area.height < 12 {
        frame.render_widget(Paragraph::new("Make the terminal a bit larger."), area);
        finish_selection(frame, app);
        return;
    }

    let pieces = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(2),
            Constraint::Length(2),
        ])
        .split(area);

    draw_header(frame, app, pieces[0]);
    draw_columns(frame, pieces[1]);
    draw_installed(frame, app, pieces[2]);
    draw_activity(frame, app, pieces[3]);
    draw_footer(frame, app, pieces[4]);

    match &app.mode {
        Mode::Add => draw_add(frame, app, area),
        Mode::ConfirmRemove { .. } => draw_confirm(frame, app, area),
        Mode::Path => draw_path(frame, app, area),
        Mode::Group => draw_group(frame, app, area),
        Mode::Menu => draw_menu(frame, app, area),
        Mode::Update => draw_update(frame, app, area),
        Mode::Help => draw_help(frame, app, area),
        Mode::Settings => draw_settings(frame, app, area),
        Mode::Progress => draw_progress(frame, app, area),
        Mode::List | Mode::Filter => {}
    }
    finish_selection(frame, app);
}

fn finish_selection(frame: &mut Frame, app: &mut App) {
    app.screen = capture_screen(frame.buffer_mut());
    if let Some(sel) = app.pointer.selection {
        paint_selection(frame.buffer_mut(), sel);
    }
}

fn capture_screen(buf: &Buffer) -> Vec<Vec<String>> {
    let area = buf.area;
    let rows_n = area.y.saturating_add(area.height) as usize;
    let cols_n = area.x.saturating_add(area.width) as usize;
    let mut rows = vec![Vec::new(); rows_n];
    for y in area.y..area.y.saturating_add(area.height) {
        let mut row = vec![String::new(); cols_n];
        for x in area.x..area.x.saturating_add(area.width) {
            if let Some(cell) = buf.cell(Position::new(x, y)) {
                row[x as usize] = cell.symbol().to_string();
            }
        }
        rows[y as usize] = row;
    }
    rows
}

fn paint_selection(buf: &mut Buffer, sel: crate::select::TextSel) {
    let (start, end) = crate::select::range(sel.anchor, sel.head);
    let area = buf.area;
    let last_x = area.x.saturating_add(area.width).saturating_sub(1);
    let style = Style::default().fg(Color::White).bg(Color::Blue);
    for y in start.1..=end.1 {
        if y < area.y || y >= area.y.saturating_add(area.height) {
            continue;
        }
        let x0 = if y == start.1 { start.0 } else { area.x };
        let x1 = if y == end.1 { end.0 } else { last_x };
        if x1 < x0 {
            continue;
        }
        for x in x0..=x1 {
            if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                cell.set_style(style);
            }
        }
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
    let (badge, path) = header_parts(app, width);
    let title = Line::from(vec![
        Span::styled(
            badge,
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("  {path}"), Style::default().fg(Color::DarkGray)),
    ]);
    let behind = app
        .installed
        .iter()
        .filter(|repo| repo.sync_counts().1 > 0)
        .count();
    let dirty = app.installed.iter().filter(|repo| repo.dirty).count();
    let mut meta_text = format!("  {} installed", app.installed.len());
    if !app.marked.is_empty() {
        meta_text.push_str(&format!(" · {} selected", app.marked.len()));
    }
    if behind > 0 {
        meta_text.push_str(&format!(" · {behind} behind"));
    }
    let elsewhere = app
        .installed
        .iter()
        .filter(|repo| repo.remote_new > 0)
        .count();
    if elsewhere > 0 {
        meta_text.push_str(&format!(" · {elsewhere} new on other branches"));
    }
    if dirty > 0 {
        meta_text.push_str(&format!(" · {dirty} dirty"));
    }
    meta_text.push_str(&format!(" · {github}"));
    match &app.update {
        UpdateState::Available(release) => {
            meta_text.push_str(&format!(" · version {} is out, press v", release.version));
        }
        UpdateState::Installed(version) => {
            meta_text.push_str(&format!(
                " · version {version} runs after a restart, press v"
            ));
        }
        _ => {}
    }
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
    } else if !app.marked.is_empty() {
        Line::from(Span::styled(
            fit(
                &format!(
                    "  {} selected · enter actions · u update · m group · d remove · ctrl-a all · esc clears",
                    app.marked.len()
                ),
                width,
            ),
            Style::default().fg(Color::Cyan),
        ))
    } else {
        let mut hint =
            "  arrows move · enter actions · space selects · m group · / filter · s sort · ? help"
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

/// Where the checkouts are: the folder they share, or how many folders.
fn locations(app: &App) -> String {
    let mut parents: Vec<&std::path::Path> = Vec::new();
    for parent in app.checkouts.iter().filter_map(|path| path.parent()) {
        if !parents.iter().any(|known| paths::same_path(known, parent)) {
            parents.push(parent);
        }
    }
    match parents.as_slice() {
        [] => format!("new clones go to {}", app.clone_dir.display()),
        [only] => only.display().to_string(),
        many => format!("in {} folders", many.len()),
    }
}

fn header_parts(app: &App, width: usize) -> (String, String) {
    let badge = format!(" {} ", app.title);
    let mut path = locations(app);
    let badge_w = UnicodeWidthStr::width(badge.as_str());
    let room = width.saturating_sub(badge_w + 2);
    if UnicodeWidthStr::width(path.as_str()) > room {
        path = fit_end(&path, room);
    }
    (badge, path)
}

fn draw_columns(frame: &mut Frame, area: Rect) {
    let widths = column_widths(area.width, false);
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

/// "1 repository", "3 repositories".
fn count_repos(count: usize) -> String {
    match count {
        1 => "1 repository".to_string(),
        _ => format!("{count} repositories"),
    }
}

fn draw_installed(frame: &mut Frame, app: &mut App, area: Rect) {
    // A line of the list: the header of a group, or a repository.
    enum Shown {
        Header {
            key: String,
            text: String,
            ticked: bool,
        },
        Repo(Box<LocalRepo>),
    }
    let rows: Vec<Shown> = app
        .rows()
        .into_iter()
        .map(|row| match row {
            Row::Group {
                name,
                repos,
                incoming,
                collapsed,
            } => {
                let label = if name.is_empty() { "No group" } else { &name };
                let arrow = if collapsed { "▸" } else { "▾" };
                let mut text = format!("{arrow} {label}  ·  {}", count_repos(repos));
                if incoming > 0 {
                    text.push_str(&format!("  ·  {incoming} to update"));
                }
                let members: Vec<&LocalRepo> = app
                    .installed
                    .iter()
                    .filter(|repo| repo.group == name)
                    .collect();
                let ticked = !members.is_empty()
                    && members.iter().all(|repo| app.is_marked_local(&repo.folder));
                Shown::Header {
                    key: group_key(&name),
                    text,
                    ticked,
                }
            }
            Row::Repo(repo) => Shown::Repo(Box::new(repo.clone())),
        })
        .collect();
    if rows.is_empty() {
        let text = if app.installed.is_empty() {
            "  No repositories yet. Press a to add one, or i to bring in clones you already have."
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
        .and_then(|name| {
            rows.iter().position(|row| match row {
                Shown::Header { key, .. } => key == name,
                Shown::Repo(repo) => &repo.folder == name,
            })
        })
        .unwrap_or(0);
    let height = area.height as usize;
    align_scroll(&mut app.list_scroll, selected, rows.len(), height);
    let start = app.list_scroll;
    let widths = column_widths(area.width, false);
    let clickable = matches!(app.mode, Mode::List | Mode::Filter);
    for (offset, row) in rows.iter().enumerate().skip(start).take(height) {
        let y = area.y + (offset - start) as u16;
        let rect = Rect {
            x: area.x,
            y,
            width: area.width,
            height: 1,
        };
        let chosen = offset == selected;
        let marker = |ticked: bool| match (chosen, ticked) {
            (true, true) => "▌●",
            (true, false) => "▌ ",
            (false, true) => " ●",
            (false, false) => "  ",
        };
        let repo = match row {
            Shown::Header { key, text, ticked } => {
                let style = if chosen {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(Color::Gray)
                        .add_modifier(Modifier::BOLD)
                };
                let line = fit(&format!("{}{text}", marker(*ticked)), area.width as usize);
                frame.render_widget(Paragraph::new(Span::styled(line, style)), rect);
                if clickable {
                    app.hits.rows.push((rect, key.clone()));
                }
                continue;
            }
            Shown::Repo(repo) => repo,
        };
        let about = app.about(repo);
        let cells = [
            marker(app.is_marked_local(&repo.folder)).to_string(),
            repo.folder.clone(),
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
        if clickable {
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
    // What runs next, when something waits for the current task.
    let waiting = match (app.queue.front(), app.queue.len()) {
        (None, _) => String::new(),
        (Some(next), 1) => format!("  ·  next: {}", next.label()),
        (Some(next), count) => format!("  ·  next: {} +{} more", next.label(), count - 1),
    };
    let message = fit(
        &format!(" {spin} {}{waiting}", app.status),
        area.width as usize,
    );
    let detail = if let Some(job) = app.job.as_ref().filter(|job| job.running()) {
        // The progress dialog is hidden: say what the run is doing.
        let doing = match job.current() {
            Some(item) if !item.progress.is_empty() => {
                format!("{}: {}", item.name, item.progress.trim())
            }
            Some(item) => item.name.clone(),
            None => String::new(),
        };
        format!("{doing}  ·  l shows progress")
    } else if let Some(group) = app.selected_group() {
        // A group's header: what the keys do with the whole group.
        if group.is_empty() {
            "Repositories in no group  ·  enter folds  ·  u updates them  ·  space selects them"
                .to_string()
        } else {
            format!(
                "Group {group}  ·  enter folds  ·  u updates it  ·  space selects it  ·  m renames  ·  d dissolves the group, its repositories stay"
            )
        }
    } else if let Some(folder) = &app.selected {
        app.installed
            .iter()
            .find(|repo| &repo.folder == folder)
            .map(|repo| {
                let id = format!("{}/{}", repo.owner, repo.repo);
                if let Some(warning) = &repo.warning {
                    format!("{id}  {warning}")
                } else if let Some(err) = &repo.error {
                    format!("{id}  {err}")
                } else if let Some(remote) = repo.remote_detail() {
                    format!("{id}  {remote}  ·  {}", repo.path.display())
                } else {
                    format!("{id}  {}", repo.path.display())
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

fn draw_footer(frame: &mut Frame, app: &mut App, area: Rect) {
    // With repositories ticked, Update and Remove say how many they act on.
    let ticked = app.marked.len();
    let counted = |label: &str| match ticked {
        0 => label.to_string(),
        _ => format!("{label} {ticked}"),
    };
    let buttons = [
        ("Add".to_string(), "a", Action::Add),
        (counted("Update"), "u", Action::UpdateOne),
        ("Update all".to_string(), "U", Action::UpdateAll),
        (counted("Remove"), "d", Action::Remove),
        ("Refresh".to_string(), "r", Action::Refresh),
        ("Settings".to_string(), "c", Action::Settings),
        ("Quit".to_string(), "q", Action::Quit),
        ("Import".to_string(), "i", Action::Import),
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
    let (text, style) = footer_hint(app, hint.width as usize);
    frame.render_widget(Paragraph::new(Span::styled(text, style)), hint);
}

fn footer_hint(app: &App, width: usize) -> (String, Style) {
    if let Some(note) = &app.copy_note {
        let style = if app.copy_failed {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Cyan)
        };
        return (fit(&format!("  {note}"), width), style);
    }
    if app.pointer.selection.is_some() {
        return (
            fit("  release to copy", width),
            Style::default().fg(Color::Cyan),
        );
    }
    (
        fit(
            "  drag copies text · double-click updates · r fetches · l last run · esc cancels",
            width,
        ),
        Style::default().fg(Color::DarkGray),
    )
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
    if inner.height < 5 {
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);
    let parts = [rows[0], rows[2], rows[3]];
    let filter = Line::from(vec![
        Span::styled(" search ", Style::default().fg(Color::DarkGray)),
        Span::styled(&app.add_filter, Style::default().fg(Color::White)),
        Span::styled("▌", Style::default().fg(Color::Cyan)),
    ]);
    frame.render_widget(Paragraph::new(filter), parts[0]);

    let status = app.add_status();
    let spin = if status.busy {
        SPINNER[(app.tick as usize / 2) % SPINNER.len()]
    } else {
        " "
    };
    let status_style = if status.error {
        Style::default().fg(Color::Red)
    } else if status.busy {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            fit(&format!(" {spin} {}", status.text), rows[1].width as usize),
            status_style,
        )),
        rows[1],
    );

    let remotes = app.add_list();
    if remotes.is_empty() {
        frame.render_widget(
            Paragraph::new(format!("   {}", app.add_empty_message()))
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
            let cloned = app.is_installed(repo);
            let marked = app.is_marked(repo);
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
                "{}{}{}{} {}{}{}",
                if chosen { "▌" } else { " " },
                if marked {
                    "●"
                } else if cloned {
                    "✓"
                } else {
                    " "
                },
                fit(&label, if wide { 36 } else { 22 }),
                language,
                repo.description,
                pushed,
                tags
            );
            let style = if chosen && cloned {
                Style::default().fg(Color::Black).bg(Color::DarkGray)
            } else if chosen {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if marked {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if cloned || repo.archived {
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

    let marked = app.add_marked.len();
    let clone = if marked > 0 {
        format!("Clone {marked}")
    } else {
        "Clone".to_string()
    };
    draw_dialog_buttons(
        frame,
        app,
        parts[2],
        &[
            (clone.as_str(), Action::Clone),
            ("Retry", Action::Retry),
            ("Cancel", Action::Cancel),
        ],
    );
    let used = (clone.len() + 2 + 1 + " Retry ".len() + 1 + " Cancel ".len() + 2) as u16;
    if parts[2].width > used + 20 {
        let hint = Rect {
            x: parts[2].x + used,
            y: parts[2].y,
            width: parts[2].width - used,
            height: 1,
        };
        let (text, style) = if marked > 0 {
            (
                format!("{marked} selected · enter clones them · space unselects"),
                Style::default().fg(Color::Cyan),
            )
        } else {
            (
                "space selects several · enter clones".to_string(),
                Style::default().fg(Color::DarkGray),
            )
        };
        frame.render_widget(
            Paragraph::new(Span::styled(fit(&text, hint.width as usize), style)),
            hint,
        );
    }
}

fn draw_confirm(frame: &mut Frame, app: &mut App, screen: Rect) {
    const LISTED: usize = 8;
    let folders = match &app.mode {
        Mode::ConfirmRemove { folders } => folders.clone(),
        _ => return,
    };
    // Name, path, and whether it has uncommitted changes.
    let repos: Vec<(String, String, bool)> = folders
        .iter()
        .map(|folder| {
            let repo = app.installed.iter().find(|repo| &repo.folder == folder);
            (
                folder.clone(),
                repo.map(|repo| repo.path.display().to_string())
                    .unwrap_or_default(),
                repo.is_some_and(|repo| repo.dirty),
            )
        })
        .collect();
    let many = repos.len() > 1;
    let dirty = repos.iter().filter(|(_, _, dirty)| *dirty).count();
    let listed = if repos.len() > LISTED {
        LISTED - 1
    } else {
        repos.len()
    };
    let rows = if many {
        listed + usize::from(repos.len() > listed)
    } else {
        2
    };
    let popup = centered(
        screen,
        68.min(screen.width.saturating_sub(2)),
        ((rows + 10) as u16).min(screen.height.saturating_sub(2)),
    );
    let title = if many {
        format!(" Remove {} repositories ", repos.len())
    } else {
        " Remove repository ".to_string()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(
            title,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    let warning = match (many, dirty) {
        (false, 0) => {
            "The local folder will be deleted. The GitHub repository stays online.".to_string()
        }
        (false, _) => "This checkout has uncommitted changes. The folder and those changes \
                       will be deleted."
            .to_string(),
        (true, 0) => {
            "The local folders will be deleted. The GitHub repositories stay online.".to_string()
        }
        (true, 1) => "1 of them has uncommitted changes. The folders and those changes will \
                      be deleted."
            .to_string(),
        (true, count) => format!(
            "{count} of them have uncommitted changes. The folders and those changes will be \
             deleted."
        ),
    };
    let name_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let mut body: Vec<Line> = Vec::new();
    if many {
        for (folder, _, dirty) in repos.iter().take(listed) {
            let mut spans = vec![Span::styled(folder.clone(), name_style)];
            if *dirty {
                spans.push(Span::styled(
                    "  uncommitted changes",
                    Style::default().fg(Color::Yellow),
                ));
            }
            body.push(Line::from(spans));
        }
        if repos.len() > listed {
            body.push(Line::from(Span::styled(
                format!("and {} more", repos.len() - listed),
                Style::default().fg(Color::DarkGray),
            )));
        }
    } else if let Some((folder, path, _)) = repos.first() {
        body.push(Line::from(Span::styled(folder.clone(), name_style)));
        body.push(Line::from(Span::styled(
            fit(path, inner.width as usize),
            Style::default().fg(Color::DarkGray),
        )));
    }
    body.push(Line::from(""));
    body.push(Line::from(Span::styled(
        warning,
        Style::default().fg(if dirty > 0 {
            Color::Yellow
        } else {
            Color::Gray
        }),
    )));
    body.push(Line::from(""));
    body.push(Line::from(Span::styled(
        if many {
            "Enter deletes them from disk. f only takes them off the list. Esc cancels."
        } else {
            "Enter deletes it from disk. f only takes it off the list. Esc cancels."
        },
        Style::default().fg(Color::DarkGray),
    )));
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
        &[
            ("Delete", Action::Confirm),
            ("Forget", Action::Forget),
            ("Cancel", Action::Cancel),
        ],
    );
}

/// The version dialog: what runs, what is out, and installing it.
fn draw_update(frame: &mut Frame, app: &mut App, screen: Rect) {
    let tick = app.tick as usize;
    let state = app.update.clone();
    let popup = centered(
        screen,
        72.min(screen.width.saturating_sub(2)),
        16.min(screen.height.saturating_sub(2)),
    );
    let accent = match &state {
        UpdateState::Failed(_) => Color::Red,
        UpdateState::Available(_) | UpdateState::Installed(_) => Color::Green,
        _ => Color::Cyan,
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(accent))
        .title(Span::styled(
            format!(" DevAssist {} ", crate::update::VERSION),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    let text = Rect {
        x: inner.x + 1,
        y: inner.y,
        width: inner.width.saturating_sub(2),
        height: inner.height.saturating_sub(1),
    };
    let width = text.width as usize;
    let dim = Style::default().fg(Color::DarkGray);
    let bold = Style::default().fg(accent).add_modifier(Modifier::BOLD);
    let megabytes = |bytes: u64| format!("{:.1} MB", bytes as f64 / 1_048_576.0);
    let mut lines: Vec<Line> = vec![Line::from("")];
    let buttons: Vec<(&str, Action)> = match &state {
        UpdateState::Unknown | UpdateState::Checking => {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{} ", SPINNER[(tick / 2) % SPINNER.len()]),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw("Looking for a newer version…"),
            ]));
            vec![("Close", Action::Cancel)]
        }
        UpdateState::Current => {
            lines.push(Line::from(Span::styled(
                "✓ This is the latest version.",
                bold,
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("enter looks again", dim)));
            vec![("Look again", Action::Confirm), ("Close", Action::Cancel)]
        }
        UpdateState::Available(release) => {
            lines.push(Line::from(Span::styled(
                format!("Version {} is available.", release.version),
                bold,
            )));
            lines.push(Line::from(""));
            // What is new, as far as there is room for it.
            let room = (text.height as usize).saturating_sub(lines.len() + 3);
            let notes: Vec<&str> = release
                .notes
                .lines()
                .map(str::trim_end)
                .filter(|line| !line.trim().is_empty())
                .collect();
            for line in notes.iter().take(room) {
                lines.push(Line::from(Span::styled(
                    fit(line, width),
                    Style::default().fg(Color::Gray),
                )));
            }
            if notes.len() > room {
                lines.push(Line::from(Span::styled("…", dim)));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                fit("enter downloads it and replaces this program.", width),
                dim,
            )));
            vec![("Install", Action::Confirm), ("Later", Action::Cancel)]
        }
        UpdateState::Installing {
            release,
            done,
            total,
        } => {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{} ", SPINNER[(tick / 2) % SPINNER.len()]),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(format!("Downloading version {}…", release.version)),
            ]));
            lines.push(Line::from(""));
            let fraction = (*total > 0).then(|| *done as f64 / *total as f64);
            lines.push(Line::from(bar_spans(width, fraction, Color::Cyan, tick)));
            lines.push(Line::from(Span::styled(
                format!("{} of {}", megabytes(*done), megabytes(*total)),
                dim,
            )));
            vec![("Hide", Action::Cancel)]
        }
        UpdateState::Installed(version) => {
            lines.push(Line::from(Span::styled(
                format!("✓ Version {version} is installed."),
                bold,
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(
                "It runs the next time DevAssist starts. enter restarts now.",
            ));
            vec![("Restart now", Action::Confirm), ("Later", Action::Cancel)]
        }
        UpdateState::Failed(err) => {
            lines.push(Line::from(Span::styled("✗ That did not work.", bold)));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                err.clone(),
                Style::default().fg(Color::Red),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("enter tries again", dim)));
            vec![("Try again", Action::Confirm), ("Close", Action::Cancel)]
        }
    };
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), text);
    let row = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    draw_dialog_buttons(frame, app, row, &buttons);
}

/// The actions for the chosen repositories, opened with Enter.
fn draw_menu(frame: &mut Frame, app: &mut App, screen: Rect) {
    let Some(menu) = app.menu.as_ref() else {
        return;
    };
    let many = menu.repos.len() > 1;
    let picked = menu.selected;
    let title = match menu.repos.as_slice() {
        [one] => format!(" {one} "),
        several => format!(" {} ", count_repos(several.len())),
    };
    let names = {
        let shown = menu.repos[..menu.repos.len().min(4)].join(", ");
        match menu.repos.len().saturating_sub(4) {
            0 => shown,
            rest => format!("{shown} and {rest} more"),
        }
    };
    let rows = MenuItem::ALL.len() as u16;
    let extra = u16::from(many) * 2;
    let popup = centered(
        screen,
        46.min(screen.width.saturating_sub(2)),
        (rows + extra + 5).min(screen.height.saturating_sub(2)),
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            fit_end(&title, popup.width.saturating_sub(4) as usize),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    let width = inner.width as usize;
    let dim = Style::default().fg(Color::DarkGray);
    let mut y = inner.y;
    if many {
        // Which repositories the menu is about.
        frame.render_widget(
            Paragraph::new(Span::styled(fit(&format!(" {names}"), width), dim)),
            Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 1,
            },
        );
        y += 2;
    } else {
        y += 1;
    }
    for (index, item) in MenuItem::ALL.into_iter().enumerate() {
        if y + 1 >= inner.y + inner.height {
            break;
        }
        let rect = Rect {
            x: inner.x,
            y,
            width: inner.width,
            height: 1,
        };
        let chosen = index == picked;
        let hovered = app.mouse.is_some_and(|(mx, my)| contains(rect, mx, my));
        let label = item.label(many);
        let gap = width.saturating_sub(UnicodeWidthStr::width(label) + 6);
        let line = format!(
            "{} {label}{}{}  ",
            if chosen { "▌" } else { " " },
            " ".repeat(gap),
            item.key()
        );
        let style = if chosen {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if hovered {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if item == MenuItem::Remove {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        };
        frame.render_widget(Paragraph::new(Span::styled(fit(&line, width), style)), rect);
        app.hits.rows.push((rect, format!("menu:{index}")));
        y += 1;
    }
    let hint = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            fit(" ↑ ↓ choose · enter runs · esc closes", width),
            dim,
        )),
        hint,
    );
}

/// The dialog that moves repositories into a group, or renames a group.
fn draw_group(frame: &mut Frame, app: &mut App, screen: Rect) {
    let Some(prompt) = app.group_prompt.as_ref() else {
        return;
    };
    let renaming = prompt.rename.is_some();
    let (title, what, note, button) = match &prompt.rename {
        Some(old) => (
            " Rename group ",
            old.clone(),
            "the repositories stay in it".to_string(),
            "Rename",
        ),
        None => {
            let what = match prompt.repos.as_slice() {
                [one] => one.clone(),
                many => {
                    let shown = many[..many.len().min(3)].join(", ");
                    match many.len().saturating_sub(3) {
                        0 => format!("{}: {shown}", count_repos(many.len())),
                        rest => format!("{}: {shown} and {rest} more", count_repos(many.len())),
                    }
                }
            };
            (
                " Move to a group ",
                what,
                "a new name makes a new group · empty takes it out of its group".to_string(),
                "Move",
            )
        }
    };
    let input = prompt.input.clone();
    let error = prompt.error.clone();
    let groups = app.groups();
    let typed = input.trim().to_lowercase();

    let popup = centered(
        screen,
        80.min(screen.width.saturating_sub(2)),
        12.min(screen.height.saturating_sub(2)),
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    let text = Rect {
        x: inner.x + 1,
        y: inner.y,
        width: inner.width.saturating_sub(2),
        height: inner.height.saturating_sub(1),
    };
    let width = text.width as usize;
    let dim = Style::default().fg(Color::DarkGray);
    let label = "Group  ";
    let room = width.saturating_sub(label.len() + 1);
    let mut lines = vec![
        Line::from(Span::styled(
            fit(&what, width),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(label, Style::default().fg(Color::Gray)),
            Span::styled(fit_end(&input, room), Style::default().fg(Color::White)),
            Span::styled("▌", Style::default().fg(Color::Cyan)),
        ]),
        Line::from(Span::styled(
            format!("{}{}", " ".repeat(label.len()), fit(&note, room)),
            dim,
        )),
        Line::from(""),
    ];
    // The groups that exist; the one that is typed is lit.
    if let Some(err) = error {
        lines.push(Line::from(Span::styled(
            fit(&err, width),
            Style::default().fg(Color::Red),
        )));
    } else if groups.is_empty() {
        lines.push(Line::from(Span::styled("there are no groups yet", dim)));
    } else {
        let mut spans = vec![Span::styled("groups  ", dim)];
        let mut used = 8;
        for group in &groups {
            let cell = UnicodeWidthStr::width(group.as_str()) + 3;
            if used + cell > width {
                spans.push(Span::styled("…", dim));
                break;
            }
            used += cell;
            let style = if group.to_lowercase() == typed {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            spans.push(Span::styled(group.clone(), style));
            spans.push(Span::raw("   "));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(""));
    let hint = if renaming {
        "enter renames · esc cancels"
    } else {
        "↑ ↓ pick a group that exists · enter moves · esc cancels"
    };
    lines.push(Line::from(Span::styled(fit(hint, width), dim)));
    frame.render_widget(Paragraph::new(lines), text);
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
        &[(button, Action::Confirm), ("Cancel", Action::Cancel)],
    );
}

/// The dialog that asks for a folder: where new clones go, or where the
/// clones to import are.
fn draw_path(frame: &mut Frame, app: &mut App, screen: Rect) {
    let Some(prompt) = app.prompt.as_ref() else {
        return;
    };
    let (title, what, note, button, verb) = match &prompt.purpose {
        Purpose::Clone(remotes) => {
            let names: Vec<String> = remotes.iter().map(|remote| remote.id()).collect();
            let what = match names.as_slice() {
                [one] => one.clone(),
                many => {
                    let shown = many[..many.len().min(3)].join(", ");
                    match many.len().saturating_sub(3) {
                        0 => format!("{} repositories: {shown}", many.len()),
                        rest => format!("{} repositories: {shown} and {rest} more", many.len()),
                    }
                }
            };
            let note = match prompt.target() {
                Some(target) => format!("becomes {}", target.display()),
                None if names.len() > 1 => "each repository gets its own folder inside".to_string(),
                None => String::new(),
            };
            (" Clone into ", what, note, "Clone", "clones")
        }
        Purpose::Import => (
            " Import clones you already have ",
            "Adds the GitHub clones in a folder to the overview.".to_string(),
            "the folder itself counts too · nothing on disk is changed".to_string(),
            "Import",
            "imports",
        ),
    };
    let input = prompt.input.clone();
    let below = match (&prompt.error, prompt.matches.is_empty()) {
        (Some(err), _) => Some((err.clone(), Color::Red)),
        (None, false) => Some((prompt.matches.join("   "), Color::Gray)),
        (None, true) => None,
    };

    let popup = centered(
        screen,
        80.min(screen.width.saturating_sub(2)),
        12.min(screen.height.saturating_sub(2)),
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    let text = Rect {
        x: inner.x + 1,
        y: inner.y,
        width: inner.width.saturating_sub(2),
        height: inner.height.saturating_sub(1),
    };
    let width = text.width as usize;
    let dim = Style::default().fg(Color::DarkGray);
    let label = "Folder  ";
    let room = width.saturating_sub(label.len() + 1);
    let mut lines = vec![
        Line::from(Span::styled(
            fit(&what, width),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        // A long path keeps its end in view: that is where typing happens.
        Line::from(vec![
            Span::styled(label, Style::default().fg(Color::Gray)),
            Span::styled(fit_end(&input, room), Style::default().fg(Color::White)),
            Span::styled("▌", Style::default().fg(Color::Cyan)),
        ]),
        Line::from(Span::styled(
            format!("{}{}", " ".repeat(label.len()), fit_end(&note, room)),
            dim,
        )),
        Line::from(""),
    ];
    match below {
        Some((line, color)) => lines.push(Line::from(Span::styled(
            fit(&line, width),
            Style::default().fg(color),
        ))),
        None => lines.push(Line::from("")),
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        fit(
            &format!("tab completes · ~ is home · ctrl-w drops the last part · enter {verb}"),
            width,
        ),
        dim,
    )));
    frame.render_widget(Paragraph::new(lines), text);
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
        &[(button, Action::Confirm), ("Cancel", Action::Cancel)],
    );
}

fn draw_settings(frame: &mut Frame, app: &mut App, screen: Rect) {
    let Some(form) = app.form.as_ref() else {
        return;
    };
    let popup = centered(
        screen,
        84.min(screen.width.saturating_sub(2)),
        18.min(screen.height.saturating_sub(2)),
    );
    let heading = if form.welcome {
        " Welcome · first-time setup "
    } else {
        " Settings "
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            heading,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    if inner.height < 7 || inner.width < 30 {
        return;
    }
    let width = inner.width as usize;
    // Roomy: intro, then label + hint + gap per field. Tight: one line each.
    let roomy = inner.height >= 16;
    let label_width = 15;
    let value_width = width.saturating_sub(label_width + 1);
    let folder_title = crate::config::default_title();

    let mut lines: Vec<Line> = Vec::new();
    let mut field_rows: Vec<(u16, Field)> = Vec::new();
    let mut protocol_hits: Vec<(u16, u16, u16, String)> = Vec::new();
    let intro = if form.welcome {
        "Which GitHub accounts should Add list by default?"
    } else {
        "What Add lists by default and how it clones."
    };
    lines.push(Line::from(Span::styled(
        fit(&format!(" {intro}"), width),
        Style::default().fg(Color::Gray),
    )));
    if roomy {
        lines.push(Line::from(""));
    }
    for field in Field::ALL {
        let focused = form.focus == field;
        let label_style = if focused {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        let mut spans = vec![Span::styled(
            format!(
                "{}{}",
                if focused { "▌" } else { " " },
                fit(field.label(), label_width)
            ),
            label_style,
        )];
        let row = inner.y + lines.len() as u16;
        match field {
            Field::Protocol => {
                let mut x = inner.x + 1 + label_width as u16;
                for protocol in PROTOCOLS {
                    let text = format!(" {protocol} ");
                    let style = if protocol != form.protocol {
                        Style::default().fg(Color::DarkGray)
                    } else if focused {
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                    };
                    protocol_hits.push((x, row, text.len() as u16, protocol.to_string()));
                    x += text.len() as u16 + 1;
                    spans.push(Span::styled(text, style));
                    spans.push(Span::raw(" "));
                }
                if focused {
                    spans.push(Span::styled(
                        "  ← → changes",
                        Style::default().fg(Color::DarkGray),
                    ));
                }
            }
            _ => {
                let (value, empty) = match field {
                    Field::Accounts => (form.accounts.as_str(), "none".to_string()),
                    Field::Pinned => (form.pinned.as_str(), "none".to_string()),
                    _ => (form.title.as_str(), format!("{folder_title} · default")),
                };
                let room = value_width.saturating_sub(1);
                if value.is_empty() {
                    if focused {
                        spans.push(Span::styled("▌", Style::default().fg(Color::Cyan)));
                    }
                    spans.push(Span::styled(
                        fit(&empty, room).trim_end().to_string(),
                        Style::default().fg(Color::DarkGray),
                    ));
                } else {
                    // Keep the end of a long value in view: that is where typing happens.
                    spans.push(Span::styled(
                        fit_end(value, room),
                        Style::default().fg(Color::White),
                    ));
                    if focused {
                        spans.push(Span::styled("▌", Style::default().fg(Color::Cyan)));
                    }
                }
            }
        }
        field_rows.push((row, field));
        lines.push(Line::from(spans));
        if roomy {
            lines.push(Line::from(Span::styled(
                fit(
                    &format!("{}{}", " ".repeat(label_width + 1), field.hint()),
                    width,
                ),
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(""));
        }
    }
    let note = match &form.error {
        Some(err) => Span::styled(
            fit(&format!(" {err}"), width),
            Style::default().fg(Color::Red),
        ),
        None => Span::styled(
            fit(&format!(" Saves to {}", form.path.display()), width),
            Style::default().fg(Color::DarkGray),
        ),
    };
    lines.push(Line::from(note));

    let text_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner.height.saturating_sub(1),
    };
    frame.render_widget(Paragraph::new(lines), text_area);
    let last_row = text_area.y + text_area.height;
    for (x, y, w, name) in protocol_hits {
        if y < last_row {
            app.hits.rows.push((
                Rect {
                    x,
                    y,
                    width: w,
                    height: 1,
                },
                format!("protocol:{name}"),
            ));
        }
    }
    for (y, field) in field_rows {
        if y < last_row {
            app.hits.rows.push((
                Rect {
                    x: inner.x,
                    y,
                    width: inner.width,
                    height: 1,
                },
                format!("field:{}", field.key()),
            ));
        }
    }

    let buttons = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    let cancel = if app.form.as_ref().is_some_and(|form| form.welcome) {
        "Not now"
    } else {
        "Cancel"
    };
    draw_dialog_buttons(
        frame,
        app,
        buttons,
        &[("Save", Action::Save), (cancel, Action::Cancel)],
    );
    let used = (" Save ".len() + 1 + cancel.len() + 2 + 2) as u16;
    if buttons.width > used + 20 {
        let hint = Rect {
            x: buttons.x + used,
            y: buttons.y,
            width: buttons.width - used,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(Span::styled(
                fit("tab next · enter saves · esc closes", hint.width as usize),
                Style::default().fg(Color::DarkGray),
            )),
            hint,
        );
    }
}

/// The update and clone dialog. A header says how the whole run stands,
/// the left pane lists the repositories, and the right pane shows the
/// progress and git output of the one that is picked or running.
fn draw_progress(frame: &mut Frame, app: &mut App, screen: Rect) {
    let tick = app.tick as usize;
    let queued = app.queue.len();
    let Some(job) = app.job.as_mut() else {
        return;
    };
    let running = job.running();
    let accent = match &job.result {
        None => Color::Cyan,
        Some(Ok(_)) => Color::Green,
        Some(Err(_)) if job.cancelled => Color::Yellow,
        Some(Err(_)) => Color::Red,
    };
    let dim = Style::default().fg(Color::DarkGray);
    let gray = Style::default().fg(Color::Gray);

    let room = screen.height.saturating_sub(2) as usize;
    let height = (screen.height as usize * 5 / 6).clamp(8.min(room), room);
    let wide = (screen.width as usize * 9 / 10).max(96);
    let popup = centered(
        screen,
        wide.min(screen.width.saturating_sub(4) as usize) as u16,
        height as u16,
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(accent))
        .title(Span::styled(
            match queued {
                0 => format!(" {} ", job.title),
                _ => format!(" {} · {queued} queued ", job.title),
            },
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    if inner.height < 5 || inner.width < 24 {
        return;
    }
    // Roomy: air around the header and bordered panes. Tight: header, a
    // plain list and the buttons.
    let roomy = inner.height >= 14 && inner.width >= 60;
    let air = u16::from(roomy);
    let side = 2 * air + 1;
    let width = inner.width.saturating_sub(2 * side) as usize;
    let header_y = inner.y + air;
    let buttons_y = inner.y + inner.height - 1;
    let body = Rect {
        x: inner.x + air,
        y: header_y + 3 + air,
        width: inner.width.saturating_sub(2 * air),
        height: buttons_y.saturating_sub(header_y + 3 + air + air),
    };
    let row = |y: u16| Rect {
        x: inner.x + side,
        y,
        width: width as u16,
        height: 1,
    };

    let total = job.items.len();
    let many = total > 1;
    let shown = job.shown();
    let current = job.current();

    // Headline: what is happening now, or how it ended. Time on the right.
    let (phase, numbers) = current
        .map(|item| split_progress(&item.progress))
        .unwrap_or_default();
    let bold = Style::default().fg(accent).add_modifier(Modifier::BOLD);
    let headline = match &job.result {
        None if job.cancelling => vec![Span::styled(
            "■ Cancelling…",
            Style::default().fg(Color::Yellow),
        )],
        None => {
            let name = current.map(|item| item.name.as_str()).unwrap_or("Starting");
            let mut spans = vec![Span::styled(
                format!("{} ", SPINNER[(tick / 2) % SPINNER.len()]),
                Style::default().fg(Color::Cyan),
            )];
            if many {
                spans.push(Span::styled(
                    format!("{} of {total}  ", (job.settled() + 1).min(total)),
                    gray,
                ));
                spans.push(Span::styled(
                    name.to_string(),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                let doing = if phase.is_empty() { "Working" } else { &phase };
                spans.push(Span::styled(
                    doing.to_string(),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            spans
        }
        Some(Ok(message)) => vec![Span::styled(format!("✓ {message}"), bold)],
        Some(Err(_)) if job.cancelled => vec![Span::styled("■ Cancelled.", bold)],
        Some(Err(message)) => vec![Span::styled(format!("✗ {message}"), bold)],
    };
    let time = clock(job.elapsed());
    frame.render_widget(Paragraph::new(Line::from(headline)), row(header_y));
    let time_width = time.len().min(width) as u16;
    frame.render_widget(
        Paragraph::new(Span::styled(format!(" {time}"), gray)),
        Rect {
            x: inner.x + side + (width as u16).saturating_sub(time_width + 1),
            y: header_y,
            width: time_width + 1,
            height: 1,
        },
    );

    // One bar across the dialog for the whole run.
    let fraction = job.fraction();
    let mut bar = bar_spans(width.saturating_sub(6), fraction, accent, tick);
    bar.push(Span::styled(
        match fraction {
            Some(fraction) => format!("{:>5}%", (fraction * 100.0).round() as u16),
            None => String::new(),
        },
        gray,
    ));
    frame.render_widget(Paragraph::new(Line::from(bar)), row(header_y + 1));

    // Facts: how many are done, running, waiting and failed. One
    // repository shows git's numbers instead.
    let mut facts: Vec<Span> = Vec::new();
    if many {
        let chips = [
            (job.count(ItemState::Done), "done", Color::Green),
            (job.count(ItemState::Running), "running", Color::Cyan),
            (job.count(ItemState::Waiting), "waiting", Color::Gray),
            (job.count(ItemState::Failed), "failed", Color::Red),
        ];
        for (count, label, color) in chips {
            if count == 0 && label != "done" {
                continue;
            }
            if !facts.is_empty() {
                facts.push(Span::styled("  ·  ", dim));
            }
            facts.push(Span::styled(
                format!("{count} {label}"),
                Style::default().fg(color),
            ));
        }
        if job.skipped > 0 {
            facts.push(Span::styled(
                format!("  ·  {} already up to date", job.skipped),
                dim,
            ));
        }
    } else if let Some(item) = job.items.first() {
        let steps = item.steps_label().unwrap_or_default();
        if running {
            facts.push(Span::styled(join_facts(&steps, &numbers), gray));
        } else {
            facts.push(Span::styled(join_facts(&item.note, &steps), dim));
        }
    }
    frame.render_widget(Paragraph::new(Line::from(facts)), row(header_y + 2));

    // Left pane: the repositories. A long list follows the shown one.
    let name_width = job
        .items
        .iter()
        .map(|item| UnicodeWidthStr::width(item.name.as_str()))
        .max()
        .unwrap_or(0);
    let (list, detail) = if !roomy {
        (Some(body), None)
    } else if many {
        let list_width = (name_width as u16 + 19)
            .clamp(30, 48)
            .min(body.width * 45 / 100);
        (
            Some(Rect {
                width: list_width,
                ..body
            }),
            Some(Rect {
                x: body.x + list_width + 1,
                width: body.width.saturating_sub(list_width + 1),
                ..body
            }),
        )
    } else {
        (None, Some(body))
    };
    if let Some(area) = list {
        let rows_area = if roomy {
            let pane = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(dim)
                .title(Span::styled(" Repositories ", gray));
            let rows_area = pane.inner(area);
            frame.render_widget(pane, area);
            rows_area
        } else {
            Rect {
                x: inner.x + side,
                width: width as u16,
                ..area
            }
        };
        let rows = rows_area.height as usize;
        let first = shown
            .saturating_sub(rows / 2)
            .min(total.saturating_sub(rows));
        let short_width = 12.min(rows_area.width as usize / 3);
        let label_width = (rows_area.width as usize).saturating_sub(short_width + 4);
        for (offset, item) in job.items.iter().enumerate().skip(first).take(rows) {
            let (icon, icon_style) = item_icon(item.state, tick);
            let short = match item.state {
                ItemState::Waiting if item.note.is_empty() => "waiting".to_string(),
                ItemState::Waiting => item.note.clone(),
                ItemState::Running => match (item.steps, item.percent()) {
                    (Some((done, total)), _) if total > 0 => format!("{done}/{total}"),
                    (_, Some(percent)) => format!("{percent}%"),
                    _ => "working".to_string(),
                },
                ItemState::Done if item.note.is_empty() => "done".to_string(),
                ItemState::Done => item.note.clone(),
                ItemState::Failed if item.note == "cancelled" => "cancelled".to_string(),
                ItemState::Failed => "failed".to_string(),
            };
            let picked = many && offset == shown;
            let name_style = match item.state {
                ItemState::Failed => Style::default().fg(Color::Red),
                ItemState::Waiting if !picked => gray,
                _ => Style::default().fg(Color::White),
            };
            let name_style = if picked {
                name_style.add_modifier(Modifier::BOLD)
            } else {
                name_style
            };
            let short_style = match item.state {
                ItemState::Running => Style::default().fg(Color::Cyan),
                ItemState::Failed => Style::default().fg(Color::Red),
                _ => dim,
            };
            let rect = Rect {
                x: rows_area.x,
                y: rows_area.y + (offset - first) as u16,
                width: rows_area.width,
                height: 1,
            };
            let pad = short_width.saturating_sub(UnicodeWidthStr::width(short.as_str()));
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        if picked { "▌" } else { " " },
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::styled(icon, icon_style),
                    Span::raw(" "),
                    Span::styled(fit(&item.name, label_width + 1), name_style),
                    Span::styled(
                        format!("{}{}", " ".repeat(pad), fit(&short, short_width).trim_end()),
                        short_style,
                    ),
                ])),
                rect,
            );
            if many {
                app.hits.rows.push((rect, format!("job:{offset}")));
            }
        }
    }

    // Right pane: the shown repository's own progress and git's output.
    job.view = 0;
    if let (Some(area), Some(item)) = (detail, job.items.get(shown)) {
        let back = if job.scroll > 0 {
            format!(" · {} lines back", job.scroll)
        } else {
            String::new()
        };
        let title = if many {
            format!(" {}{back} ", item.name)
        } else {
            format!(" Output{back} ")
        };
        let pane = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(dim)
            .title(Span::styled(
                title,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ));
        let text = pane.inner(area);
        frame.render_widget(pane, area);
        let text = Rect {
            x: text.x + 1,
            width: text.width.saturating_sub(2),
            ..text
        };
        let text_width = text.width as usize;
        let mut lines: Vec<Line> = Vec::new();
        if many && text.height >= 8 {
            let (icon, icon_style) = item_icon(item.state, tick);
            let (item_phase, item_numbers) = split_progress(&item.progress);
            // An update counts branches: done, out of those with news.
            let steps_text = item.steps_label().unwrap_or_default();
            let steps = item.steps.filter(|(_, total)| *total > 0);
            let part = steps.map(|(done, total)| f64::from(done) / f64::from(total));
            let (status, status_style, detail_line, detail_style) = match item.state {
                ItemState::Waiting if item.note.is_empty() => {
                    ("waiting for its turn".to_string(), dim, String::new(), dim)
                }
                ItemState::Waiting => (item.note.clone(), dim, String::new(), dim),
                ItemState::Running => (
                    if item_phase.is_empty() {
                        "working".to_string()
                    } else {
                        item_phase
                    },
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                    join_facts(&steps_text, &item_numbers),
                    gray,
                ),
                ItemState::Done => (
                    if item.note.is_empty() {
                        "done".to_string()
                    } else {
                        item.note.clone()
                    },
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                    steps_text,
                    gray,
                ),
                ItemState::Failed => (
                    if item.note == "cancelled" {
                        "cancelled".to_string()
                    } else {
                        "failed".to_string()
                    },
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    item.note.clone(),
                    Style::default().fg(Color::Red),
                ),
            };
            let took = item.elapsed().map(clock).unwrap_or_default();
            let gap = text_width.saturating_sub(
                2 + UnicodeWidthStr::width(status.as_str()) + UnicodeWidthStr::width(took.as_str()),
            );
            lines.push(Line::from(vec![
                Span::styled(format!("{icon} "), icon_style),
                Span::styled(status, status_style),
                Span::raw(" ".repeat(gap)),
                Span::styled(took, dim),
            ]));
            let (fill, color) = match item.state {
                ItemState::Waiting => (Some(0.0), Color::DarkGray),
                ItemState::Running => (item.fraction(), Color::Cyan),
                ItemState::Done => (Some(part.unwrap_or(1.0)), Color::Green),
                ItemState::Failed => (Some(part.unwrap_or(1.0)), Color::Red),
            };
            // Right of the bar: branches done of branches due, else git's percentage.
            let tag = match (steps, item.percent()) {
                (Some((done, total)), _) => format!("{done}/{total}"),
                (None, Some(percent)) if item.state == ItemState::Running => {
                    format!("{percent}%")
                }
                _ => String::new(),
            };
            let tag_width = if tag.is_empty() {
                0
            } else {
                UnicodeWidthStr::width(tag.as_str()).max(4) + 2
            };
            let mut item_bar = bar_spans(text_width.saturating_sub(tag_width), fill, color, tick);
            if !tag.is_empty() {
                item_bar.push(Span::styled(format!("{tag:>tag_width$}"), gray));
            }
            lines.push(Line::from(item_bar));
            lines.push(Line::from(Span::styled(
                fit(&detail_line, text_width),
                detail_style,
            )));
            lines.push(Line::from(Span::styled("─".repeat(text_width), dim)));
        }
        let out_rows = (text.height as usize).saturating_sub(lines.len());
        let end = item.output.len().saturating_sub(job.scroll);
        let start = end.saturating_sub(out_rows);
        for line in item.output.iter().skip(start).take(end - start) {
            lines.push(Line::from(Span::styled(fit(line, text_width), gray)));
        }
        if item.output.is_empty() {
            let empty = match item.state {
                ItemState::Waiting => "nothing yet",
                ItemState::Running => "waiting for git…",
                _ => "git printed nothing",
            };
            lines.push(Line::from(Span::styled(empty, dim)));
        }
        frame.render_widget(Paragraph::new(lines), text);
        job.view = out_rows;
    }

    // Button labels carry their own leading space: start one column early
    // so the words line up with the header.
    let buttons = Rect {
        x: inner.x + side - 1,
        y: buttons_y,
        width: width as u16 + 1,
        height: 1,
    };
    let moves = match (many, roomy) {
        (true, true) => "↑ ↓ repository · pgup pgdn output",
        (true, false) => "↑ ↓ repository",
        (false, _) => "↑ ↓ scroll output",
    };
    let (next, hint) = if running {
        (
            draw_dialog_buttons(
                frame,
                app,
                buttons,
                &[("Cancel", Action::Cancel), ("Hide", Action::Hide)],
            ),
            format!("{moves} · esc cancels · h hides, the run goes on"),
        )
    } else {
        (
            draw_dialog_buttons(frame, app, buttons, &[("Close", Action::Cancel)]),
            match queued {
                0 => format!("{moves} · enter closes"),
                _ => format!("{moves} · enter closes and starts the next task"),
            },
        )
    };
    let end = buttons.x + buttons.width;
    if end > next + 22 {
        let hint_width = (end - next - 1) as usize;
        let pad = hint_width.saturating_sub(UnicodeWidthStr::width(hint.as_str()));
        let area = Rect {
            x: next + 1,
            y: buttons.y,
            width: hint_width as u16,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(Span::styled(
                format!("{}{}", " ".repeat(pad), fit(&hint, hint_width).trim_end()),
                dim,
            )),
            area,
        );
    }
}

/// Two facts on one line. An empty one leaves no separator behind.
fn join_facts(first: &str, second: &str) -> String {
    match (first.is_empty(), second.is_empty()) {
        (false, false) => format!("{first}  ·  {second}"),
        (false, true) => first.to_string(),
        _ => second.to_string(),
    }
}

fn item_icon(state: ItemState, tick: usize) -> (&'static str, Style) {
    match state {
        ItemState::Waiting => ("·", Style::default().fg(Color::DarkGray)),
        ItemState::Running => (
            SPINNER[(tick / 2) % SPINNER.len()],
            Style::default().fg(Color::Cyan),
        ),
        ItemState::Done => ("✓", Style::default().fg(Color::Green)),
        ItemState::Failed => ("✗", Style::default().fg(Color::Red)),
    }
}

/// A line of `width` cells, lit up to `fraction`. Without a fraction a
/// short block moves back and forth.
fn bar_spans(width: usize, fraction: Option<f64>, color: Color, tick: usize) -> Vec<Span<'static>> {
    let width = width.max(1);
    let (from, to) = match fraction {
        Some(fraction) => (
            0,
            (fraction.clamp(0.0, 1.0) * width as f64).round() as usize,
        ),
        None => {
            let block = (width / 5).clamp(1, 12);
            let span = width - block;
            let step = if span == 0 { 0 } else { tick % (span * 2) };
            let at = if step > span { span * 2 - step } else { step };
            (at, at + block)
        }
    };
    let dim = Style::default().fg(Color::DarkGray);
    vec![
        Span::styled("━".repeat(from), dim),
        Span::styled("━".repeat(to - from), Style::default().fg(color)),
        Span::styled("━".repeat(width - to), dim),
    ]
}

fn draw_help(frame: &mut Frame, app: &mut App, screen: Rect) {
    let popup = centered(
        screen,
        72.min(screen.width.saturating_sub(2)),
        31.min(screen.height.saturating_sub(2)),
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
space          select several · ctrl-a selects all shown
m              put the selection into a group · on a group: rename it
enter          actions for the selection · on a group: fold it
u              update the selection
U              update every repository
a              add any GitHub repository, into a folder you choose
i              import clones you already have, from any folder
d, Delete      delete the selection, or only take it off the list
r, F5          fetch and refresh
s              sort by name or by attention
o              open the selection in the file manager
c              settings: default accounts, protocol, title
l              show the last update or clone
v              version: look for an update and install it
/              filter the installed list
drag           select text and copy it
wheel          scroll
esc            clear a selection, cancel, or go back
q              quit · ctrl-c copies, or quits

In Add, type owner/repo or paste a GitHub URL.
owner/ or a link to an account lists all of its repositories.
Space selects several repositories. Enter clones them all.
Drag across any text. Releasing the mouse copies it.
Starting and Refresh fetch. Update fast-forwards every branch.
Remove asks before it deletes anything.
An action started while another runs waits and runs after it.";
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

/// Draws the buttons and returns the column after the last one.
fn draw_dialog_buttons(
    frame: &mut Frame,
    app: &mut App,
    area: Rect,
    buttons: &[(&str, Action)],
) -> u16 {
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
    x
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
    let (ahead, behind) = repo.sync_counts();
    if repo.dirty || behind > 0 || repo.upstream_gone || (!repo.all_branches_current && ahead == 0)
    {
        Style::default().fg(Color::Yellow)
    } else if repo.remote_new > 0 {
        // Nothing to pull here, but the remote has work on other branches.
        Style::default().fg(Color::Magenta)
    } else if ahead > 0 {
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
        (24, 18, 32)
    };
    let state = 8;
    let about = total.saturating_sub(marker + name + branch + state + sync);
    [marker, name, branch, state, sync, about]
}

fn add_source(app: &App) -> String {
    if let Some(owner) = app.add_account() {
        return format!("github.com/{owner}");
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    fn app(first_run: bool) -> App {
        App::for_test(Settings {
            checkouts: Vec::new(),
            placement: Vec::new(),
            collapsed: Vec::new(),
            update_check: false,
            update_check_now: false,
            clone_dir: PathBuf::from("development"),
            owners_explicit: true,
            unsaved: false,
            title: "development".into(),
            custom_title: None,
            owners: vec!["AsP3X".into()],
            pinned: Vec::new(),
            protocol: crate::gitops::Protocol::Auto,
            doctor: false,
            config_path: PathBuf::from("development").join("devassist.toml"),
            first_run,
        })
    }

    fn render(app: &mut App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push_str(buffer.cell(Position::new(x, y)).unwrap().symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn the_overview_shows_commits_waiting_on_other_remote_branches() {
        let mut app = app(false);
        let current = crate::gitops::LocalRepo {
            folder: "api".into(),
            owner: "AsP3X".into(),
            repo: "api".into(),
            path: PathBuf::from("development").join("api"),
            branch: "main".into(),
            ahead: Some(0),
            behind: Some(0),
            all_branches_current: true,
            ready: true,
            ..Default::default()
        };
        let mut branchy = current.clone();
        branchy.folder = "portal".into();
        branchy.repo = "portal".into();
        branchy.remote_new = 3;
        branchy.remote_branches = vec!["dev".into()];
        let mut both = current.clone();
        both.folder = "docs".into();
        both.repo = "docs".into();
        both.behind = Some(2);
        both.total_behind = 2;
        both.all_branches_current = false;
        both.remote_new = 12;
        both.remote_branches = vec!["dev".into(), "fix/a".into(), "fix/b".into()];
        app.installed = vec![current, both, branchy];
        app.selected = Some("portal".into());
        let screen = render(&mut app, 120, 20);
        assert!(screen.contains("up to date"));
        assert!(screen.contains("3 new on dev"));
        assert!(screen.contains("behind 2, 12 new on 3 branches"));
        assert!(screen.contains("3 installed · 1 behind · 2 new on other branches"));
        assert!(screen.contains("AsP3X/portal  3 new commits on remote branch dev"));
    }

    fn local(folder: &str, dirty: bool) -> crate::gitops::LocalRepo {
        crate::gitops::LocalRepo {
            folder: folder.into(),
            owner: "AsP3X".into(),
            repo: folder.into(),
            path: PathBuf::from("development").join(folder),
            branch: "main".into(),
            dirty,
            ahead: Some(0),
            behind: Some(0),
            all_branches_current: true,
            ready: true,
            ..Default::default()
        }
    }

    #[test]
    fn ticked_repositories_show_in_the_overview_and_in_the_remove_dialog() {
        let mut app = app(false);
        app.installed = vec![
            local("api", false),
            local("docs", true),
            local("portal", false),
        ];
        app.selected = Some("docs".into());
        app.marked = vec!["api".into(), "docs".into()];
        let screen = render(&mut app, 120, 24);
        assert!(screen.contains(" ●api"));
        assert!(screen.contains("▌●docs"));
        assert!(screen.contains("  portal"));
        assert!(screen.contains("3 installed · 2 selected"));
        assert!(screen.contains("2 selected · enter actions · u update · m group · d remove"));
        assert!(screen.contains(" Update 2 u "));
        assert!(screen.contains(" Remove 2 d "));

        app.mode = Mode::ConfirmRemove {
            folders: vec!["api".into(), "docs".into()],
        };
        let screen = render(&mut app, 120, 24);
        assert!(screen.contains(" Remove 2 repositories "));
        assert!(screen.contains("docs  uncommitted changes"));
        assert!(screen.contains("1 of them has uncommitted changes."));
        assert!(screen.contains("Enter deletes them from disk. f only takes them off the"));
        assert!(screen.contains(" Delete   Forget   Cancel "));

        // Nothing ticked: the buttons carry no count, one repository is asked about.
        app.marked.clear();
        app.mode = Mode::ConfirmRemove {
            folders: vec!["portal".into()],
        };
        let screen = render(&mut app, 120, 24);
        assert!(screen.contains(" Remove repository "));
        assert!(screen.contains("Enter deletes it from disk. f only takes it off the list."));
        assert!(screen.contains(" Update u "));
    }

    fn remote(name: &str) -> crate::github::RemoteRepo {
        crate::github::RemoteRepo::direct(crate::github::RepoSpec {
            owner: "AsP3X".into(),
            name: name.into(),
        })
    }

    #[test]
    fn adding_asks_where_the_clone_should_go() {
        use crate::pathprompt::PathPrompt;
        let mut app = app(false);
        let dir = PathBuf::from(if cfg!(windows) { r"C:\dev" } else { "/srv/dev" });
        app.prompt = Some(PathPrompt::new(
            Purpose::Clone(vec![remote("portal")]),
            &dir,
        ));
        app.mode = Mode::Path;
        let screen = render(&mut app, 120, 30);
        assert!(screen.contains("╭ Clone into "));
        assert!(screen.contains("AsP3X/portal"));
        assert!(screen.contains(&format!("Folder  {}▌", dir.display())));
        assert!(screen.contains(&format!("becomes {}", dir.join("portal").display())));
        assert!(screen.contains("tab completes"));
        assert!(screen.contains(" Clone   Cancel "));
        assert!(
            app.hits
                .buttons
                .iter()
                .any(|(_, action)| *action == Action::Confirm)
        );

        // Several repositories, and what Tab found.
        let several = vec![remote("a"), remote("b"), remote("c"), remote("d")];
        let mut prompt = PathPrompt::new(Purpose::Clone(several), &dir);
        prompt.matches = vec!["api-new".into(), "api-old".into()];
        app.prompt = Some(prompt);
        let screen = render(&mut app, 120, 30);
        assert!(screen.contains("4 repositories: AsP3X/a, AsP3X/b, AsP3X/c and 1 more"));
        assert!(screen.contains("each repository gets its own folder inside"));
        assert!(screen.contains("api-new   api-old"));

        // Import uses the same dialog, and shows what is wrong.
        let mut prompt = PathPrompt::new(Purpose::Import, &dir);
        prompt.error = Some("That is not a folder.".into());
        app.prompt = Some(prompt);
        let screen = render(&mut app, 120, 30);
        assert!(screen.contains("╭ Import clones you already have "));
        assert!(screen.contains("That is not a folder."));
        assert!(screen.contains(" Import   Cancel "));
    }

    #[test]
    fn the_header_says_where_the_checkouts_are() {
        let mut app = app(false);
        let screen = render(&mut app, 120, 24);
        assert!(screen.contains("new clones go to development"));
        assert!(screen.contains("i to bring in clones you already have"));

        let work = PathBuf::from("work");
        app.checkouts = vec![work.join("api"), work.join("docs")];
        let screen = render(&mut app, 120, 24);
        assert!(screen.contains("  work "));

        app.checkouts.push(PathBuf::from("elsewhere").join("tool"));
        let screen = render(&mut app, 120, 24);
        assert!(screen.contains("in 2 folders"));
    }

    #[test]
    fn groups_show_as_headers_with_their_repositories_under_them() {
        let mut app = app(false);
        let mut api = local("api", false);
        api.group = "Work".into();
        let mut web = local("web", false);
        web.group = "Work".into();
        web.behind = Some(2);
        web.total_behind = 2;
        let mut notes = local("notes", false);
        notes.group = "home".into();
        app.installed = vec![api, local("loose", false), notes, web];
        app.collapsed = vec!["home".into()];
        app.marked = vec!["api".into(), "web".into()];
        app.selected = Some(group_key("Work"));
        let screen = render(&mut app, 120, 24);
        // Groups by name, what is in no group last; a folded group hides its rows.
        let line = |text: &str| screen.lines().position(|line| line.contains(text)).unwrap();
        assert!(screen.contains(" ▸ home  ·  1 repository"));
        assert!(screen.contains("▌●▾ Work  ·  2 repositories  ·  1 to update"));
        assert!(screen.contains("  ▾ No group  ·  1 repository"));
        assert!(!screen.contains("notes"));
        assert!(line("▸ home") < line("▾ Work"));
        assert!(line("▾ Work") < line(" ●api"));
        assert!(line(" ●web") < line("▾ No group"));
        assert!(line("▾ No group") < line("  loose"));
        assert!(screen.contains("Group Work  ·  enter folds"));
        assert!(app.hits.rows.iter().any(|(_, name)| name == "group::home"));
    }

    #[test]
    fn the_group_dialog_moves_and_renames() {
        use crate::app::GroupPrompt;
        let mut app = app(false);
        let mut api = local("api", false);
        api.group = "Work".into();
        let mut notes = local("notes", false);
        notes.group = "home".into();
        app.installed = vec![api, local("loose", false), notes];
        app.group_prompt = Some(GroupPrompt {
            repos: vec!["loose".into(), "notes".into()],
            rename: None,
            input: "work".into(),
            error: None,
        });
        app.mode = Mode::Group;
        let screen = render(&mut app, 120, 30);
        assert!(screen.contains("╭ Move to a group "));
        assert!(screen.contains("2 repositories: loose, notes"));
        assert!(screen.contains("Group  work▌"));
        assert!(screen.contains("groups  home   Work"));
        assert!(screen.contains(" Move   Cancel "));

        app.group_prompt = Some(GroupPrompt {
            repos: Vec::new(),
            rename: Some("Work".into()),
            input: String::new(),
            error: Some("A group needs a name.".into()),
        });
        let screen = render(&mut app, 120, 30);
        assert!(screen.contains("╭ Rename group "));
        assert!(screen.contains("A group needs a name."));
        assert!(screen.contains(" Rename   Cancel "));
    }

    #[test]
    fn enter_shows_the_actions_for_the_selection() {
        use crate::app::Menu;
        let mut app = app(false);
        app.installed = vec![
            local("api", false),
            local("docs", false),
            local("web", false),
        ];
        app.selected = Some("docs".into());
        app.menu = Some(Menu {
            repos: vec!["docs".into()],
            selected: 1,
        });
        app.mode = Mode::Menu;
        let screen = render(&mut app, 120, 30);
        assert!(screen.contains("╭ docs "));
        assert!(screen.contains("  Update "));
        assert!(screen.contains("▌ Open the folder "));
        for label in [
            "Open on GitHub",
            "Move to a group…",
            "Copy the path",
            "Remove…",
        ] {
            assert!(screen.contains(label), "{label} is missing");
        }
        assert!(screen.contains("enter runs"));
        assert_eq!(
            app.hits
                .rows
                .iter()
                .filter(|(_, name)| name.starts_with("menu:"))
                .count(),
            MenuItem::ALL.len()
        );

        // Several repositories: the menu says which, and words change.
        app.menu = Some(Menu {
            repos: vec!["api".into(), "docs".into(), "web".into()],
            selected: 0,
        });
        let screen = render(&mut app, 120, 30);
        assert!(screen.contains("╭ 3 repositories "));
        assert!(screen.contains("api, docs, web"));
        assert!(screen.contains("▌ Update "));
        assert!(screen.contains("Open the folders"));
        assert!(screen.contains("Copy the paths"));
    }

    #[test]
    fn the_version_dialog_walks_through_an_update() {
        use crate::update::{Asset, Release};
        let release = Release {
            version: "9.0.0".into(),
            notes: "## What is new\n\n- Groups fold\n- Faster clones".into(),
            page: "https://github.com/AsP3X/DevAssist/releases/tag/v9.0.0".into(),
            download: Some(Asset {
                name: "devassist-9.0.0-x64.exe".into(),
                url: "https://api.github.com/assets/1".into(),
                size: 4_194_304,
            }),
            sums: None,
        };
        let mut app = app(false);

        // In the overview, the header says a version is out.
        app.update = UpdateState::Available(release.clone());
        let screen = render(&mut app, 120, 30);
        assert!(screen.contains("version 9.0.0 is out, press v"));

        app.mode = Mode::Update;
        let screen = render(&mut app, 120, 30);
        assert!(screen.contains(&format!("╭ DevAssist {} ", crate::update::VERSION)));
        assert!(screen.contains("Version 9.0.0 is available."));
        assert!(screen.contains("- Groups fold"));
        assert!(screen.contains(" Install   Later "));

        app.update = UpdateState::Installing {
            release,
            done: 1_048_576,
            total: 4_194_304,
        };
        let screen = render(&mut app, 120, 30);
        assert!(screen.contains("Downloading version 9.0.0…"));
        assert!(screen.contains("1.0 MB of 4.0 MB"));

        app.update = UpdateState::Installed("9.0.0".into());
        let screen = render(&mut app, 120, 30);
        assert!(screen.contains("✓ Version 9.0.0 is installed."));
        assert!(screen.contains(" Restart now   Later "));

        app.update = UpdateState::Failed("DevAssist cannot write to C:\\Program Files".into());
        let screen = render(&mut app, 120, 30);
        assert!(screen.contains("✗ That did not work."));
        assert!(screen.contains("DevAssist cannot write to"));
        assert!(screen.contains(" Try again   Close "));

        app.update = UpdateState::Current;
        let screen = render(&mut app, 120, 30);
        assert!(screen.contains("✓ This is the latest version."));
    }

    #[test]
    fn queued_actions_show_under_the_list_and_on_the_dialog() {
        use crate::app::Queued;
        let mut app = app(false);
        app.installed = vec![local("api", false), local("docs", false)];
        app.busy = true;
        app.status = "Updating docs…".into();
        app.queue.push_back(Queued::Update(vec!["api".into()]));
        let screen = render(&mut app, 120, 24);
        assert!(screen.contains("Updating docs…  ·  next: update api"));

        app.queue.push_back(Queued::Refresh);
        app.queue
            .push_back(Queued::Remove(vec!["api".into(), "docs".into()]));
        let screen = render(&mut app, 120, 24);
        assert!(screen.contains("next: update api +2 more"));

        app.job = Some(batch());
        app.mode = Mode::Progress;
        let screen = render(&mut app, 120, 36);
        assert!(screen.contains("╭ Cloning 3 repositories · 3 queued "));
    }

    #[test]
    fn the_first_launch_shows_the_setup_with_the_detected_account() {
        let mut app = app(true);
        let screen = render(&mut app, 100, 30);
        assert!(screen.contains("Welcome"));
        assert!(screen.contains("AsP3X"));
        assert!(screen.contains("Not now"));
        assert!(!screen.contains("…│"), "a line is cut off");
        assert!(screen.contains("devassist.toml"));
        assert!(
            app.hits
                .rows
                .iter()
                .any(|(_, name)| name == "field:accounts")
        );
        assert!(app.hits.rows.iter().any(|(_, name)| name == "protocol:ssh"));
    }

    #[test]
    fn the_settings_dialog_fits_a_small_terminal() {
        let mut app = app(false);
        app.run(Action::Settings);
        let screen = render(&mut app, 60, 14);
        for label in [
            "Settings",
            "Accounts",
            "Pinned repos",
            "Clone with",
            "Title",
            "Save",
        ] {
            assert!(screen.contains(label), "{label} is missing");
        }
    }

    #[test]
    fn the_add_dialog_shows_what_space_selected() {
        use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
        let mut app = app(false);
        for name in ["alpha", "beta", "gamma"] {
            app.remotes
                .push(crate::github::RemoteRepo::direct(crate::github::RepoSpec {
                    owner: "AsP3X".into(),
                    name: name.into(),
                }));
        }
        app.run(Action::Add);
        render(&mut app, 100, 30);
        let space = Event::Key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        app.on_event(space.clone());
        app.on_event(space);
        let screen = render(&mut app, 100, 30);
        assert!(screen.contains("●AsP3X/alpha"));
        assert!(screen.contains("●AsP3X/beta"));
        assert!(!screen.contains("●AsP3X/gamma"));
        assert!(screen.contains("Clone 2"));
        assert!(screen.contains("2 selected"));
    }

    fn job(names: &[&str], skipped: usize) -> crate::atoms::task::job::Job {
        let names = names.iter().map(|name| (*name).to_string()).collect();
        crate::atoms::task::job::Job::new("Cloning 3 repositories", names, skipped)
    }

    fn batch() -> crate::atoms::task::job::Job {
        use crate::atoms::task::job::JobEvent;
        let mut run = job(&["AsP3X/api", "AsP3X/portal", "AsP3X/docs"], 2);
        run.on(JobEvent::Start(0));
        run.on(JobEvent::Line("Cloning into 'api'...".into(), false));
        run.on(JobEvent::Done(0, "cloned".into()));
        run.on(JobEvent::Start(1));
        run.on(JobEvent::Line("Cloning into 'portal'...".into(), false));
        run.on(JobEvent::Line(
            "Receiving objects:  50% (133/266), 1.20 MiB | 2.00 MiB/s".into(),
            true,
        ));
        run
    }

    #[test]
    fn a_running_batch_shows_the_run_the_list_and_the_running_repository() {
        let mut app = app(false);
        app.job = Some(batch());
        app.mode = Mode::Progress;
        let screen = render(&mut app, 120, 36);
        assert!(screen.contains("╭ Cloning 3 repositories "));
        // Header: where the run stands.
        assert!(screen.contains("2 of 3  AsP3X/portal"));
        // Half of the download is 42% of the clone: resolving deltas follows.
        assert!(screen.contains("42%"));
        assert!(screen.contains("1 done  ·  1 running  ·  1 waiting  ·  2 already up to date"));
        // Left pane: every repository with its state.
        assert!(screen.contains("╭ Repositories "));
        assert!(screen.contains("✓ AsP3X/api"));
        assert!(screen.contains("▌⠋ AsP3X/portal") || screen.contains("▌⠙ AsP3X/portal"));
        assert!(screen.contains("· AsP3X/docs"));
        // Right pane: the running repository, not the whole log.
        assert!(screen.contains("╭ AsP3X/portal "));
        assert!(screen.contains("Receiving objects"));
        assert!(screen.contains("133/266, 1.20 MiB · 2.00 MiB/s"));
        assert!(screen.contains("Cloning into 'portal'..."));
        assert!(!screen.contains("Cloning into 'api'..."));
        assert!(screen.contains("Cancel"));
        assert!(screen.contains("Hide"));
        assert!(!screen.contains(" activity "));
        assert!(
            app.hits
                .buttons
                .iter()
                .any(|(_, action)| *action == Action::Hide)
        );
        assert!(app.hits.rows.iter().any(|(_, name)| name == "job:2"));
    }

    #[test]
    fn an_update_shows_each_repository_by_its_branches() {
        use crate::atoms::task::job::JobEvent;
        let mut app = app(false);
        let names = ["api", "portal", "docs"].map(String::from).to_vec();
        let mut run = crate::atoms::task::job::Job::new("Updating 3 repositories", names, 0);
        run.on(JobEvent::Start(0));
        run.on(JobEvent::Steps(0, 2));
        run.on(JobEvent::Steps(2, 2));
        run.on(JobEvent::Done(0, "updated".into()));
        run.on(JobEvent::Start(1));
        run.on(JobEvent::Steps(0, 4));
        run.on(JobEvent::Line(
            "Receiving objects:  50% (133/266), 1.20 MiB | 2.00 MiB/s".into(),
            true,
        ));
        run.on(JobEvent::Steps(1, 4));
        run.on(JobEvent::Line(
            "dev: fast-forwarded 3 commits".into(),
            false,
        ));
        app.job = Some(run);
        app.mode = Mode::Progress;
        let screen = render(&mut app, 120, 36);
        // The list and the bar count branches, not git's download.
        assert!(screen.contains("1/4"));
        assert!(screen.contains("1 of 4 branches updated  ·  133/266, 1.20 MiB · 2.00 MiB/s"));
        // One and a quarter of three repositories.
        assert!(screen.contains("42%"));

        // A finished repository keeps its count.
        app.on_event(ratatui::crossterm::event::Event::Key(
            ratatui::crossterm::event::KeyEvent::new(
                ratatui::crossterm::event::KeyCode::Up,
                ratatui::crossterm::event::KeyModifiers::NONE,
            ),
        ));
        let screen = render(&mut app, 120, 36);
        assert!(screen.contains("╭ api "));
        assert!(screen.contains("2 of 2 branches updated"));
        assert!(screen.contains("2/2"));
    }

    #[test]
    fn picking_a_repository_shows_its_own_output() {
        use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
        let mut app = app(false);
        app.job = Some(batch());
        app.mode = Mode::Progress;
        app.on_event(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)));
        let screen = render(&mut app, 120, 36);
        assert!(screen.contains("▌✓ AsP3X/api"));
        assert!(screen.contains("╭ AsP3X/api "));
        assert!(screen.contains("Cloning into 'api'..."));
        assert!(!screen.contains("Cloning into 'portal'..."));
    }

    #[test]
    fn a_failed_job_says_what_went_wrong() {
        use crate::atoms::task::job::JobEvent;
        let mut app = app(false);
        let mut run = job(&["AsP3X/api", "AsP3X/portal"], 0);
        run.on(JobEvent::Start(0));
        run.on(JobEvent::Done(0, "cloned".into()));
        run.on(JobEvent::Start(1));
        run.on(JobEvent::Line("fatal: repository not found".into(), false));
        run.on(JobEvent::Failed(1, "fatal: repository not found".into()));
        run.finish(Err("Cloned 1, 1 failed: AsP3X/portal".into()), false);
        app.job = Some(run);
        app.mode = Mode::Progress;
        let screen = render(&mut app, 120, 36);
        assert!(screen.contains("✗ Cloned 1, 1 failed: AsP3X/portal"));
        assert!(screen.contains("1 done  ·  1 failed"));
        assert!(screen.contains("▌✗ AsP3X/portal"));
        assert!(screen.contains("╭ AsP3X/portal "));
        assert!(screen.contains("✗ failed"));
        assert!(screen.contains("fatal: repository not found"));
        assert!(screen.contains("Close"));
        assert!(!screen.contains("Hide"));
    }

    #[test]
    fn one_repository_gets_the_whole_dialog_for_its_output() {
        use crate::atoms::task::job::JobEvent;
        let mut app = app(false);
        let mut run =
            crate::atoms::task::job::Job::new("Updating artcraft", vec!["artcraft".into()], 0);
        run.on(JobEvent::Start(0));
        run.on(JobEvent::Line(
            "From https://github.com/AsP3X/artcraft".into(),
            false,
        ));
        run.on(JobEvent::Line(
            "Receiving objects:  45% (120/266), 1.20 MiB | 2.00 MiB/s".into(),
            true,
        ));
        app.job = Some(run);
        app.mode = Mode::Progress;
        let screen = render(&mut app, 120, 36);
        assert!(screen.contains("╭ Updating artcraft "));
        assert!(screen.contains("Receiving objects"));
        assert!(screen.contains("120/266, 1.20 MiB · 2.00 MiB/s"));
        assert!(screen.contains("╭ Output "));
        assert!(screen.contains("From https://github.com/AsP3X/artcraft"));
        assert!(!screen.contains("Repositories "));
    }

    #[test]
    fn the_progress_dialog_fits_the_smallest_terminal() {
        use crate::atoms::task::job::JobEvent;
        let mut app = app(false);
        let mut run = job(&["one", "two", "three", "four", "five", "six"], 0);
        run.on(JobEvent::Start(4));
        app.job = Some(run);
        app.mode = Mode::Progress;
        let screen = render(&mut app, 48, 12);
        assert!(screen.contains("five"));
        assert!(screen.contains("Cancel"));
    }

    #[test]
    fn the_add_dialog_says_which_account_it_is_loading() {
        let mut app = app(false);
        app.run(Action::Add);
        app.on_event(ratatui::crossterm::event::Event::Paste(
            "https://github.com/UCS-Care-Group".into(),
        ));
        let screen = render(&mut app, 100, 30);
        assert!(screen.contains("Add from github.com/UCS-Care-Group"));
        assert!(screen.contains("Loading github.com/UCS-Care-Group"));
    }
}
