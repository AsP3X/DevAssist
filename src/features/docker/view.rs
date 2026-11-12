//! Drawing the Docker tool: the tabs, the lists and the dialogs.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::actions::{Choice, Confirm, Verb, count};
use super::state::{Action, COMPOSE, CONTAINERS, Dialog, Docker, Engine, IMAGES, Row, TABS};
use crate::atoms::ui::dialog::{self, centered};
use crate::atoms::ui::hits::contains;
use crate::atoms::ui::list::align_scroll;
use crate::atoms::ui::menu;
use crate::atoms::ui::progress;
use crate::atoms::ui::tabs;
use crate::atoms::ui::text::{fit, padded_row};

/// Draws the tool and its open dialog into `area`.
pub fn draw(frame: &mut Frame, docker: &mut Docker, area: Rect) {
    docker.tick = docker.tick.wrapping_add(1);
    docker.hits.clear();
    docker.tab_rects.clear();
    docker.ensure_selection();

    if area.width < 48 || area.height < 12 {
        frame.render_widget(Paragraph::new("Make the terminal a bit larger."), area);
        return;
    }
    if let Some(log) = docker.log.as_mut() {
        log.draw(frame, area);
        return;
    }

    let pieces = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(2),
            Constraint::Length(2),
        ])
        .split(area);

    draw_summary(frame, docker, pieces[0]);
    draw_tabs(frame, docker, pieces[1]);
    if docker.engine == Engine::Ready {
        draw_columns(frame, docker, pieces[2]);
        draw_list(frame, docker, pieces[3]);
    } else {
        draw_unreachable(frame, docker, pieces[3]);
    }
    draw_activity(frame, docker, pieces[4]);
    draw_footer(frame, docker, pieces[5]);

    match docker.dialog.clone() {
        Dialog::None => {}
        Dialog::Menu { verbs, selected } => draw_menu(frame, docker, area, &verbs, selected),
        Dialog::Confirm(confirm) => draw_confirm(frame, docker, area, &confirm),
        Dialog::Error { title, text } => draw_error(frame, docker, area, &title, &text),
    }
}

fn draw_summary(frame: &mut Frame, docker: &Docker, area: Rect) {
    let text = match &docker.engine {
        Engine::Idle | Engine::Loading => "  Docker".to_string(),
        Engine::Missing => "  Docker is not installed".to_string(),
        Engine::Down(_) => "  Docker is not running".to_string(),
        Engine::Ready => {
            let lists = &docker.lists;
            let running = lists.containers.iter().filter(|c| c.running()).count();
            let mut text = format!(
                "  {running} running · {} stopped · {} · {} · {}",
                lists.containers.len() - running,
                count(lists.projects.len(), "project"),
                count(lists.images.len(), "image"),
                count(lists.volumes.len(), "volume"),
            );
            let ticked = docker.view().marked.len();
            if ticked > 0 {
                text.push_str(&format!(" · {ticked} selected"));
            }
            text
        }
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            fit(&text, area.width as usize),
            Style::default().fg(Color::Gray),
        )),
        area,
    );
}

fn draw_tabs(frame: &mut Frame, docker: &mut Docker, area: Rect) {
    let bar = Rect {
        x: area.x + 1,
        width: area.width.saturating_sub(1),
        ..area
    };
    let mouse = if docker.modal() { None } else { docker.mouse };
    docker.tab_rects = tabs::draw(frame, bar, &TABS, docker.tab, mouse);
    let used = docker
        .tab_rects
        .iter()
        .map(|rect| rect.x + rect.width)
        .max()
        .unwrap_or(bar.x);
    let room = Rect {
        x: used + 2,
        width: (area.x + area.width).saturating_sub(used + 2),
        ..area
    };
    let filter = &docker.view().filter;
    let line = if docker.filtering {
        Line::from(vec![
            Span::styled("filter ", Style::default().fg(Color::DarkGray)),
            Span::styled(filter.clone(), Style::default().fg(Color::White)),
            Span::styled("▌", Style::default().fg(Color::Cyan)),
            Span::styled("   esc clears", Style::default().fg(Color::DarkGray)),
        ])
    } else if !filter.is_empty() {
        Line::from(vec![
            Span::styled("filter ", Style::default().fg(Color::DarkGray)),
            Span::styled(filter.clone(), Style::default().fg(Color::White)),
            Span::styled("   esc clears", Style::default().fg(Color::DarkGray)),
        ])
    } else {
        Line::from("")
    };
    frame.render_widget(Paragraph::new(line), room);
}

/// The width of each column for this tab; the first is the marker.
fn column_widths(tab: usize, total: u16) -> Vec<usize> {
    let total = total as usize;
    let marker = 2;
    let rest = total.saturating_sub(marker);
    let fixed: &[usize] = match tab {
        CONTAINERS => &[30, 28, 28],
        COMPOSE => &[26, 22],
        IMAGES => &[44, 14, 10, 18],
        _ => &[44, 10],
    };
    // On a narrow screen every column shrinks by the same share.
    let wanted: usize = fixed.iter().sum::<usize>() + 12;
    let mut widths = vec![marker];
    let mut used = 0;
    for width in fixed {
        let width = if wanted > rest {
            width * rest / wanted
        } else {
            *width
        };
        widths.push(width);
        used += width;
    }
    widths.push(rest.saturating_sub(used));
    widths
}

fn headings(tab: usize) -> &'static [&'static str] {
    match tab {
        CONTAINERS => &["", "container", "image", "status", "ports"],
        COMPOSE => &["", "project", "state", "compose files"],
        IMAGES => &["", "image", "id", "size", "created", "used by"],
        _ => &["", "volume", "driver", "used by"],
    }
}

fn draw_columns(frame: &mut Frame, docker: &Docker, area: Rect) {
    let widths = column_widths(docker.tab, area.width);
    let header = padded_row(headings(docker.tab), &widths, area.width as usize);
    frame.render_widget(
        Paragraph::new(Span::styled(header, Style::default().fg(Color::DarkGray))),
        area,
    );
}

/// A line of the list, ready to draw.
struct Shown {
    key: String,
    /// The cells after the marker. A header has one.
    cells: Vec<String>,
    style: Style,
    header: bool,
    ticked: bool,
}

fn shown_rows(docker: &Docker) -> Vec<Shown> {
    let containers = &docker.lists.containers;
    let marked = &docker.view().marked;
    let grouped = docker
        .rows()
        .iter()
        .any(|row| matches!(row, Row::Group { .. }));
    docker
        .rows()
        .into_iter()
        .map(|row| {
            let key = row.key();
            let ticked = marked.contains(&key);
            let plain = |cells: Vec<String>, style: Style| Shown {
                key: key.clone(),
                cells,
                style,
                header: false,
                ticked,
            };
            match row {
                Row::Group {
                    name,
                    running,
                    total,
                    collapsed,
                } => {
                    let label = if name.is_empty() { "No project" } else { name };
                    let arrow = if collapsed { "▸" } else { "▾" };
                    let members: Vec<&String> = containers
                        .iter()
                        .filter(|c| c.project == name)
                        .map(|c| &c.id)
                        .collect();
                    Shown {
                        key: key.clone(),
                        cells: vec![format!("{arrow} {label}  ·  {running} of {total} running")],
                        style: Style::default()
                            .fg(Color::Gray)
                            .add_modifier(Modifier::BOLD),
                        header: true,
                        ticked: !members.is_empty() && members.iter().all(|id| marked.contains(id)),
                    }
                }
                Row::Container(container) => {
                    let dot = if container.running() { "●" } else { "○" };
                    let indent = if grouped { "  " } else { "" };
                    let color = match container.state.as_str() {
                        "running" => Color::White,
                        "paused" | "restarting" => Color::Yellow,
                        "dead" => Color::Red,
                        _ => Color::DarkGray,
                    };
                    plain(
                        vec![
                            format!("{indent}{dot} {}", container.name),
                            container.image.clone(),
                            container.status.clone(),
                            container.ports.clone(),
                        ],
                        Style::default().fg(color),
                    )
                }
                Row::Project(project) => {
                    let color = if project.running == 0 {
                        Color::DarkGray
                    } else if project.running < project.total {
                        Color::Yellow
                    } else {
                        Color::White
                    };
                    plain(
                        vec![
                            project.name.clone(),
                            format!("{} of {} running", project.running, project.total),
                            project.files.join(", "),
                        ],
                        Style::default().fg(color),
                    )
                }
                Row::Image(image) => {
                    let users = image.used_by(containers);
                    let used = if !users.is_empty() {
                        users.join(", ")
                    } else if image.in_use(containers) {
                        "a container".to_string()
                    } else {
                        String::new()
                    };
                    let color = if image.dangling() {
                        Color::DarkGray
                    } else if used.is_empty() {
                        Color::Gray
                    } else {
                        Color::White
                    };
                    plain(
                        vec![
                            image.name(),
                            image.short_id(),
                            image.size.clone(),
                            image.age.clone(),
                            used,
                        ],
                        Style::default().fg(color),
                    )
                }
                Row::Volume(volume) => {
                    let users = volume.used_by(containers);
                    let color = if users.is_empty() {
                        Color::Gray
                    } else {
                        Color::White
                    };
                    plain(
                        vec![volume.label(), volume.driver.clone(), users.join(", ")],
                        Style::default().fg(color),
                    )
                }
            }
        })
        .collect()
}

fn draw_list(frame: &mut Frame, docker: &mut Docker, area: Rect) {
    let rows = shown_rows(docker);
    if rows.is_empty() {
        let what = match docker.tab {
            CONTAINERS => "containers",
            COMPOSE => "Compose projects",
            IMAGES => "images",
            _ => "volumes",
        };
        let text = if docker.view().filter.is_empty() {
            format!("  There are no {what}.")
        } else {
            format!("  No {what} match that filter.")
        };
        frame.render_widget(
            Paragraph::new(Span::styled(text, Style::default().fg(Color::DarkGray))),
            area,
        );
        return;
    }
    let tab = docker.tab;
    let selected = docker.views[tab]
        .selected
        .as_ref()
        .and_then(|key| rows.iter().position(|row| &row.key == key))
        .unwrap_or(0);
    let height = area.height as usize;
    align_scroll(&mut docker.views[tab].scroll, selected, rows.len(), height);
    let start = docker.views[tab].scroll;
    let widths = column_widths(tab, area.width);
    let clickable = !docker.modal() || docker.filtering;
    for (offset, row) in rows.iter().enumerate().skip(start).take(height) {
        let rect = Rect {
            x: area.x,
            y: area.y + (offset - start) as u16,
            width: area.width,
            height: 1,
        };
        let chosen = offset == selected;
        let marker = match (chosen, row.ticked) {
            (true, true) => "▌●",
            (true, false) => "▌ ",
            (false, true) => " ●",
            (false, false) => "  ",
        };
        let line = if row.header {
            fit(&format!("{marker}{}", row.cells[0]), area.width as usize)
        } else {
            let mut cells: Vec<&str> = vec![marker];
            cells.extend(row.cells.iter().map(String::as_str));
            padded_row(&cells, &widths, area.width as usize)
        };
        let style = if chosen {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            row.style
        };
        frame.render_widget(Paragraph::new(Span::styled(line, style)), rect);
        if clickable {
            docker.hits.rows.push((rect, row.key.clone()));
        }
    }
}

/// What the list's place says while Docker cannot be asked.
fn draw_unreachable(frame: &mut Frame, docker: &Docker, area: Rect) {
    let dim = Style::default().fg(Color::DarkGray);
    let lines = match &docker.engine {
        Engine::Idle | Engine::Loading | Engine::Ready => vec![Line::from(vec![
            Span::styled(
                format!("  {} ", progress::spinner(docker.tick)),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw("Asking Docker…"),
        ])],
        Engine::Missing => vec![
            Line::from(Span::styled(
                "  Docker is not installed.",
                Style::default().fg(Color::Yellow),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  DevAssist uses the docker command. Install Docker Desktop from",
                dim,
            )),
            Line::from(Span::styled(
                "  https://www.docker.com/products/docker-desktop/ and press r.",
                dim,
            )),
        ],
        Engine::Down(text) => vec![
            Line::from(Span::styled(
                "  Docker is not running.",
                Style::default().fg(Color::Yellow),
            )),
            Line::from(""),
            Line::from(Span::styled("  Start Docker Desktop, then press r.", dim)),
            Line::from(""),
            Line::from(Span::styled(format!("  {text}"), dim)),
        ],
    };
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn draw_activity(frame: &mut Frame, docker: &Docker, area: Rect) {
    let busy = docker.running.is_some();
    let spin = if busy {
        progress::spinner(docker.tick)
    } else {
        " "
    };
    let style = if docker.status_error {
        Style::default().fg(Color::Red)
    } else if busy {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::Gray)
    };
    let waiting = match (docker.queue.front(), docker.queue.len()) {
        (None, _) => String::new(),
        (Some(next), 1) => format!("  ·  next: {}", next.doing),
        (Some(next), more) => format!("  ·  next: {} +{} more", next.doing, more - 1),
    };
    let message = fit(
        &format!(" {spin} {}{waiting}", docker.status),
        area.width as usize,
    );
    let detail = fit(&format!("   {}", detail(docker)), area.width as usize);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(message, style)),
            Line::from(Span::styled(detail, Style::default().fg(Color::DarkGray))),
        ]),
        area,
    );
}

/// More about the highlighted row than its line has room for.
fn detail(docker: &Docker) -> String {
    if docker.engine != Engine::Ready {
        return String::new();
    }
    if let Some(group) = docker.selected_group() {
        return if group.is_empty() {
            "Containers in no Compose project  ·  enter folds  ·  space selects them".to_string()
        } else {
            format!(
                "Compose project {group}  ·  enter folds  ·  s starts or stops it  ·  space selects it"
            )
        };
    }
    let Some(key) = docker.view().selected.as_ref() else {
        return String::new();
    };
    let lists = &docker.lists;
    match docker.tab {
        CONTAINERS => lists
            .containers
            .iter()
            .find(|c| &c.id == key)
            .map(|c| {
                let id: String = c.id.chars().take(12).collect();
                let mut text = format!("{}  ·  {id}  ·  {}", c.name, c.image);
                if !c.project.is_empty() {
                    text.push_str(&format!("  ·  project {}", c.project));
                }
                text
            })
            .unwrap_or_default(),
        COMPOSE => lists
            .projects
            .iter()
            .find(|p| &p.name == key)
            .map(|p| format!("{}  ·  {}  ·  {}", p.name, p.status, p.files.join(", ")))
            .unwrap_or_default(),
        IMAGES => lists
            .images
            .iter()
            .find(|i| &i.key() == key)
            .map(|i| format!("{}  ·  {}", i.name(), i.id))
            .unwrap_or_default(),
        _ => lists
            .volumes
            .iter()
            .find(|v| &v.name == key)
            .map(|v| v.name.clone())
            .unwrap_or_default(),
    }
}

fn draw_footer(frame: &mut Frame, docker: &mut Docker, area: Rect) {
    let ticked = docker.view().marked.len();
    let counted = |label: &str| match ticked {
        0 => label.to_string(),
        _ => format!("{label} {ticked}"),
    };
    let buttons: Vec<(String, &str, Action)> = match docker.tab {
        CONTAINERS => vec![
            (counted("Start/stop"), "s", Action::Toggle),
            (counted("Restart"), "R", Action::Restart),
            ("Logs".to_string(), "l", Action::Logs),
            (counted("Remove"), "d", Action::Remove),
            ("Prune".to_string(), "p", Action::Prune),
            ("Reload".to_string(), "r", Action::Reload),
            ("Quit".to_string(), "q", Action::Quit),
        ],
        COMPOSE => vec![
            (counted("Up/stop"), "s", Action::Toggle),
            ("Logs".to_string(), "l", Action::Logs),
            ("Down".to_string(), "d", Action::Remove),
            ("Reload".to_string(), "r", Action::Reload),
            ("Quit".to_string(), "q", Action::Quit),
        ],
        _ => vec![
            (counted("Remove"), "d", Action::Remove),
            ("Prune".to_string(), "p", Action::Prune),
            ("Reload".to_string(), "r", Action::Reload),
            ("Quit".to_string(), "q", Action::Quit),
        ],
    };
    let modal = docker.modal();
    let mut x = area.x + 1;
    for (label, key, action) in buttons {
        let text = format!(" {label} {key} ");
        let width = text.len() as u16;
        if x + width >= area.x + area.width {
            break;
        }
        let rect = Rect {
            x,
            y: area.y,
            width,
            height: 1,
        };
        let hovered = docker.mouse.is_some_and(|(mx, my)| contains(rect, mx, my));
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
            docker.hits.buttons.push((rect, action));
        }
        x += width + 1;
    }
    let hint = Rect {
        x: area.x,
        y: area.y + 1,
        width: area.width,
        height: 1,
    };
    let text = fit(
        "  tab next list · enter actions · space selects · / filter · drag copies text",
        hint.width as usize,
    );
    frame.render_widget(
        Paragraph::new(Span::styled(text, Style::default().fg(Color::DarkGray))),
        hint,
    );
}

fn draw_menu(
    frame: &mut Frame,
    docker: &mut Docker,
    screen: Rect,
    verbs: &[Verb],
    selected: usize,
) {
    let chosen = docker.chosen_count();
    let what = match docker.tab {
        CONTAINERS => "container",
        COMPOSE => "project",
        IMAGES => "image",
        _ => "volume",
    };
    let popup = centered(
        screen,
        46.min(screen.width.saturating_sub(2)),
        (verbs.len() as u16 + 4).min(screen.height.saturating_sub(2)),
    );
    let inner = dialog::window(
        frame,
        popup,
        &format!(" {} ", count(chosen, what)),
        Color::Cyan,
    );
    let entries: Vec<menu::Entry> = verbs
        .iter()
        .map(|verb| menu::Entry {
            label: verb.label(),
            key: verb.key(),
            warn: verb.warns(),
        })
        .collect();
    let list = Rect {
        height: inner.height.saturating_sub(1),
        ..inner
    };
    menu::draw(
        frame,
        list,
        &entries,
        selected,
        docker.mouse,
        &mut docker.hits,
    );
    let hint = Rect {
        y: inner.y + inner.height.saturating_sub(1),
        height: 1.min(inner.height),
        ..inner
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            " enter runs it · or press its key · esc closes",
            Style::default().fg(Color::DarkGray),
        )),
        hint,
    );
}

fn draw_confirm(frame: &mut Frame, docker: &mut Docker, screen: Rect, confirm: &Confirm) {
    let told = confirm.lines();
    let tick = confirm.tick();
    let extra = if tick.is_some() { 2 } else { 0 };
    let wanted = told.len() as u16 + extra + 5;
    let popup = centered(
        screen,
        76.min(screen.width.saturating_sub(2)),
        wanted.min(screen.height.saturating_sub(2)),
    );
    let inner = dialog::window(
        frame,
        popup,
        &format!(" {} ", confirm.title()),
        Color::Yellow,
    );
    let width = inner.width.saturating_sub(2) as usize;
    // What does not fit is counted, not silently cut.
    let room = (inner.height as usize).saturating_sub(extra as usize + 3);
    let mut lines: Vec<Line> = vec![Line::from("")];
    for line in told.iter().take(if told.len() > room {
        room.saturating_sub(1)
    } else {
        room
    }) {
        let style = if line.warn {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::from(Span::styled(
            fit(&format!(" {}", line.text), width),
            style,
        )));
    }
    if told.len() > room {
        lines.push(Line::from(Span::styled(
            format!("   … and {} more lines", told.len() + 1 - room),
            Style::default().fg(Color::DarkGray),
        )));
    }
    let text = Rect {
        height: inner.height.saturating_sub(extra + 1),
        ..inner
    };
    frame.render_widget(Paragraph::new(lines), text);

    if let Some((label, on)) = tick {
        let rect = Rect {
            x: inner.x + 1,
            y: inner.y + inner.height.saturating_sub(3),
            width: inner.width.saturating_sub(2),
            height: 1,
        };
        let mark = if on { "[x]" } else { "[ ]" };
        let hovered = docker.mouse.is_some_and(|(mx, my)| contains(rect, mx, my));
        let style = if hovered {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if on {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Gray)
        };
        frame.render_widget(
            Paragraph::new(Span::styled(
                fit(&format!("{mark} {label}   (space)"), rect.width as usize),
                style,
            )),
            rect,
        );
        docker.hits.buttons.push((rect, Action::Tick));
    }

    let buttons: Vec<(String, Action)> = confirm
        .buttons()
        .into_iter()
        .map(|(label, choice)| {
            let label = match choice {
                Choice::Force => format!("{label} f"),
                _ => label.to_string(),
            };
            (label, Action::Answer(choice))
        })
        .collect();
    let named: Vec<(&str, Action)> = buttons
        .iter()
        .map(|(label, action)| (label.as_str(), *action))
        .collect();
    let row = Rect {
        x: inner.x + 1,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width.saturating_sub(1),
        height: 1,
    };
    dialog::buttons(
        frame,
        row,
        &named,
        Action::Answer(confirm.default_choice()),
        docker.mouse,
        &mut docker.hits,
    );
}

fn draw_error(frame: &mut Frame, docker: &mut Docker, screen: Rect, title: &str, text: &str) {
    let popup = centered(
        screen,
        72.min(screen.width.saturating_sub(2)),
        10.min(screen.height.saturating_sub(2)),
    );
    let inner = dialog::window(frame, popup, &format!(" {title} "), Color::Red);
    let body = Rect {
        x: inner.x + 1,
        y: inner.y,
        width: inner.width.saturating_sub(2),
        height: inner.height.saturating_sub(1),
    };
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "✗ That did not work.",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            text.to_string(),
            Style::default().fg(Color::Red),
        )),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);
    let row = Rect {
        x: inner.x + 1,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width.saturating_sub(1),
        height: 1,
    };
    dialog::buttons(
        frame,
        row,
        &[("Close", Action::CloseDialog)],
        Action::CloseDialog,
        docker.mouse,
        &mut docker.hits,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::docker::model::samples;
    use crate::features::docker::state::{GROUP_KEY, VOLUMES};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    fn docker() -> Docker {
        Docker::for_test(samples::lists())
    }

    fn press(docker: &mut Docker, code: KeyCode) {
        docker.on_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }

    fn render(docker: &mut Docker, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| draw(frame, docker, frame.area()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn the_containers_tab_groups_by_project() {
        let mut docker = docker();
        let screen = render(&mut docker, 120, 24);
        assert!(screen.contains("2 running · 2 stopped · 2 projects · 4 images · 4 volumes"));
        assert!(screen.contains(" Containers   Compose   Images   Volumes "));
        assert!(screen.contains("▾ shop  ·  2 of 3 running"));
        assert!(screen.contains("● shop-web"));
        assert!(screen.contains("○ shop-admin"));
        assert!(screen.contains("Up 28 seconds (healthy)"));
        assert!(screen.contains("8081→80"));
        assert!(screen.contains("▾ No project  ·  0 of 1 running"));
        assert!(screen.contains("○ backups"));
        // Every row can be clicked, and the headers are among them.
        assert_eq!(docker.hits.rows.len(), 6);
        assert!(docker.hits.rows[0].1.starts_with(GROUP_KEY));
        assert_eq!(docker.tab_rects.len(), 4);
    }

    #[test]
    fn each_tab_has_its_own_columns() {
        let mut docker = docker();
        press(&mut docker, KeyCode::Tab);
        let screen = render(&mut docker, 120, 24);
        assert!(screen.contains("project"));
        assert!(screen.contains("2 of 3 running"));
        assert!(screen.contains("C:\\dev\\blog\\compose.yaml"));

        press(&mut docker, KeyCode::Tab);
        let screen = render(&mut docker, 120, 24);
        assert!(screen.contains("postgres:16-alpine"));
        assert!(screen.contains("ac838714b7d7"));
        assert!(screen.contains("394MB"));
        assert!(screen.contains("shop-postgres"));
        assert!(screen.contains("<none>"));

        press(&mut docker, KeyCode::Tab);
        assert_eq!(docker.tab, VOLUMES);
        let screen = render(&mut docker, 120, 24);
        assert!(screen.contains("shop_pg_data"));
        assert!(screen.contains("1b441f4d9016"));
        assert!(screen.contains("backups"));
    }

    #[test]
    fn a_missing_or_stopped_docker_says_what_to_do() {
        let mut docker = docker();
        docker.engine = Engine::Down("error during connect: the pipe was not found".into());
        let screen = render(&mut docker, 100, 20);
        assert!(screen.contains("Docker is not running."));
        assert!(screen.contains("Start Docker Desktop, then press r."));
        assert!(screen.contains("the pipe was not found"));

        docker.engine = Engine::Missing;
        let screen = render(&mut docker, 100, 20);
        assert!(screen.contains("Docker is not installed."));

        docker.engine = Engine::Loading;
        let screen = render(&mut docker, 100, 20);
        assert!(screen.contains("Asking Docker…"));
    }

    #[test]
    fn the_remove_dialog_lists_what_goes_and_what_stays() {
        let mut docker = docker();
        docker.on_event(Event::Key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::CONTROL,
        )));
        let screen = render(&mut docker, 120, 30);
        assert!(screen.contains("4 selected"));
        assert!(screen.contains(" Remove 4 d "));
        press(&mut docker, KeyCode::Char('d'));
        let screen = render(&mut docker, 120, 30);
        assert!(screen.contains("╭ Remove containers "));
        assert!(screen.contains("These stopped containers will be removed:"));
        assert!(screen.contains("These are running and stay unless you stop them:"));
        assert!(screen.contains(" Remove   Stop and remove all f   Cancel "));
        // The buttons can be clicked.
        assert_eq!(
            docker
                .hits
                .buttons
                .iter()
                .map(|(_, action)| *action)
                .collect::<Vec<_>>(),
            [
                Action::Answer(Choice::Go),
                Action::Answer(Choice::Force),
                Action::Answer(Choice::Cancel)
            ]
        );
    }

    #[test]
    fn the_prune_dialog_has_a_tick_box() {
        let mut docker = docker();
        docker.tab = IMAGES;
        press(&mut docker, KeyCode::Char('p'));
        let screen = render(&mut docker, 120, 30);
        assert!(screen.contains("╭ Prune unused images "));
        assert!(screen.contains("1 image will be removed, about 1.2GB:"));
        assert!(screen.contains("[ ] also the 1 named image no container uses"));
        press(&mut docker, KeyCode::Char(' '));
        let screen = render(&mut docker, 120, 30);
        assert!(screen.contains("[x] also the 1 named image no container uses"));
        assert!(screen.contains("2 images will be removed"));
    }

    #[test]
    fn the_actions_menu_and_the_log_view_draw() {
        let mut docker = docker();
        press(&mut docker, KeyCode::Down);
        press(&mut docker, KeyCode::Enter);
        let screen = render(&mut docker, 120, 30);
        assert!(screen.contains("╭ 1 container "));
        assert!(screen.contains("Show the logs"));
        assert!(screen.contains("Remove…"));
        press(&mut docker, KeyCode::Char('l'));
        let screen = render(&mut docker, 120, 30);
        assert!(screen.contains("logs · shop-admin"));
    }

    #[test]
    fn a_failed_action_shows_what_docker_said() {
        let mut docker = docker();
        docker.dialog = Dialog::Error {
            title: "Stopping shop-web".into(),
            text: "Error response from daemon: no such container".into(),
        };
        let screen = render(&mut docker, 100, 24);
        assert!(screen.contains("╭ Stopping shop-web "));
        assert!(screen.contains("no such container"));
        assert!(screen.contains(" Close "));
    }

    #[test]
    fn it_fits_the_smallest_screen() {
        let mut docker = docker();
        let screen = render(&mut docker, 48, 12);
        assert!(screen.contains("shop-web"));
        press(&mut docker, KeyCode::Char('d'));
        render(&mut docker, 48, 12);
        let screen = render(&mut docker, 47, 12);
        assert!(screen.contains("Make the terminal a bit larger."));
    }
}
