use chrono::{DateTime, Local};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::prelude::Stylize;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Row, Table, TableState, Wrap};

use crate::app::{App, CreateField, Mode};
use crate::model::{GithubStatus, Task, TaskStatus};

const ACCENT: Color = Color::Rgb(120, 180, 255);

pub fn render(frame: &mut Frame, app: &mut App) {
    let areas = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(8),
        Constraint::Length(3),
    ])
    .split(frame.area());

    render_header(frame, app, areas[0]);
    if app.mode == Mode::Setup {
        render_setup(frame, app, areas[1]);
    } else {
        render_tasks(frame, app, areas[1]);
    }
    render_footer(frame, app, areas[2]);

    match app.mode {
        Mode::CreateTask => render_create_dialog(frame, app),
        Mode::EditDescription => render_description_dialog(frame, app),
        Mode::Details => render_details_dialog(frame, app),
        Mode::Setup | Mode::Normal => {}
    }
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let repository = app
        .repository
        .as_deref()
        .unwrap_or("repository not configured");
    let sync = if app.is_syncing() {
        "  refreshing..."
    } else {
        ""
    };
    let title = Line::from(vec![
        Span::styled(
            " TASKER ",
            Style::default().fg(Color::Black).bg(ACCENT).bold(),
        ),
        Span::raw("  "),
        Span::styled(repository, Style::default().fg(Color::Gray)),
        Span::styled(sync, Style::default().fg(Color::Yellow)),
    ]);
    frame.render_widget(
        Paragraph::new(title).block(Block::default().borders(Borders::BOTTOM)),
        area,
    );
}

fn render_setup(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" First-time setup ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let content = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(3),
        Constraint::Min(1),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new("Enter the single GitHub repository this task list should track."),
        content[0],
    );
    let input = Paragraph::new(app.input.as_str())
        .block(Block::default().title(" owner/repo ").borders(Borders::ALL));
    frame.render_widget(input, content[1]);
    let cursor_x = content[1].x + 1 + app.input.chars().count() as u16;
    frame.set_cursor_position((
        cursor_x.min(content[1].right().saturating_sub(2)),
        content[1].y + 1,
    ));
}

fn render_tasks(frame: &mut Frame, app: &App, area: Rect) {
    let columns =
        Layout::horizontal([Constraint::Percentage(68), Constraint::Percentage(32)]).split(area);

    let header = Row::new(["ID", "Task", "Branch", "GitHub"])
        .style(Style::default().fg(Color::Gray).bold())
        .bottom_margin(1);
    let rows = app.tasks.iter().map(|task| {
        let style = if task.status == TaskStatus::Complete {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default()
        };
        Row::new([
            task.id.to_string(),
            task.title.clone(),
            task.branch_name.clone(),
            task.github_status.to_string(),
        ])
        .style(style)
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(5),
            Constraint::Fill(2),
            Constraint::Fill(2),
            Constraint::Length(14),
        ],
    )
    .header(header)
    .row_highlight_style(Style::default().bg(Color::Rgb(35, 48, 65)).fg(Color::White))
    .highlight_symbol("> ")
    .block(
        Block::default()
            .title(format!(" Tasks ({}) ", app.tasks.len()))
            .borders(Borders::ALL),
    );
    let mut state =
        TableState::default().with_selected((!app.tasks.is_empty()).then_some(app.selected));
    frame.render_stateful_widget(table, columns[0], &mut state);

    render_task_summary(frame, app.selected_task(), columns[1]);
}

fn render_task_summary(frame: &mut Frame, task: Option<&Task>, area: Rect) {
    let lines = match task {
        Some(task) => task_lines(task),
        None => vec![
            Line::from("No tasks yet."),
            Line::from(""),
            Line::from(vec![
                Span::raw("Press "),
                Span::styled("c", key_style()),
                Span::raw(" to create one."),
            ]),
        ],
    };
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::default().title(" Selected ").borders(Borders::ALL)),
        area,
    );
}

fn task_lines(task: &Task) -> Vec<Line<'static>> {
    let github_style = github_style(task.github_status);
    let mut lines = vec![
        Line::styled(task.title.clone(), Style::default().bold().fg(Color::White)),
        Line::from(""),
        labelled("Branch", task.branch_name.clone()),
        labelled("Task", task.status.to_string()),
        Line::from(vec![
            Span::styled("GitHub: ", Style::default().fg(Color::Gray)),
            Span::styled(task.github_status.to_string(), github_style),
        ]),
        labelled("Created", format_timestamp(task.created_at)),
    ];
    if !task.description.is_empty() {
        lines.splice(
            1..1,
            [
                Line::from(""),
                Line::styled(task.description.clone(), Style::default().fg(Color::Gray)),
            ],
        );
    }
    if let Some(number) = task.pr_number {
        lines.push(labelled("PR", format!("#{number}")));
    }
    if let Some(url) = &task.pr_url {
        lines.push(labelled("URL", url.clone()));
    }
    if let Some(checked_at) = task.last_checked_at {
        lines.push(labelled("Checked", format_timestamp(checked_at)));
    }
    if let Some(completed_at) = task.completed_at {
        lines.push(labelled("Completed", format_timestamp(completed_at)));
    }
    lines
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let shortcuts = if app.mode == Mode::Setup {
        Line::from(vec![
            Span::styled("Enter", key_style()),
            Span::raw(" save  "),
            Span::styled("Esc", key_style()),
            Span::raw(" quit"),
        ])
    } else {
        Line::from(vec![
            Span::styled("c", key_style()),
            Span::raw(" create  "),
            Span::styled("d", key_style()),
            Span::raw(" done  "),
            Span::styled("e", key_style()),
            Span::raw(" describe  "),
            Span::styled("y", key_style()),
            Span::raw(" copy  "),
            Span::styled("r", key_style()),
            Span::raw(" refresh  "),
            Span::styled("Enter", key_style()),
            Span::raw(" details  "),
            Span::styled("q", key_style()),
            Span::raw(" quit"),
        ])
    };
    let message = if let Some(error) = &app.error {
        Line::styled(error.clone(), Style::default().fg(Color::Red))
    } else if let Some(notice) = &app.notice {
        Line::styled(notice.clone(), Style::default().fg(Color::Green))
    } else {
        Line::from("")
    };
    frame.render_widget(
        Paragraph::new(vec![shortcuts, message])
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::TOP)),
        area,
    );
}

fn render_create_dialog(frame: &mut Frame, app: &App) {
    let area = centered_rect(64, 11, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(" Create task ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let sections = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(5),
        Constraint::Length(1),
    ])
    .split(inner);
    let fields = [
        (
            CreateField::Title,
            " Title ",
            app.input.as_str(),
            sections[0],
        ),
        (
            CreateField::Description,
            " Description (optional) ",
            app.description_input.as_str(),
            sections[1],
        ),
    ];
    for (field, title, value, area) in fields {
        let focused = app.create_field == field;
        let border = if focused { ACCENT } else { Color::DarkGray };
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border));
        render_input(frame, value, block, area, focused);
    }
    frame.render_widget(
        Paragraph::new("Tab switch field  |  Enter create  |  Esc cancel")
            .alignment(Alignment::Right),
        sections[2],
    );
}

fn render_description_dialog(frame: &mut Frame, app: &App) {
    let area = centered_rect(64, 9, frame.area());
    frame.render_widget(Clear, area);
    let title = app
        .selected_task()
        .map(|task| format!(" Describe: {} ", task.title))
        .unwrap_or_else(|| " Describe task ".into());
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let sections = Layout::vertical([Constraint::Length(5), Constraint::Length(1)]).split(inner);
    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Gray));
    render_input(frame, &app.input, input_block, sections[0], true);
    frame.render_widget(
        Paragraph::new("Enter save  |  Esc cancel").alignment(Alignment::Right),
        sections[1],
    );
}

/// Renders a text input wrapped at character boundaries, scrolled so the end of
/// `value` stays visible, with the cursor placed after the last character.
fn render_input(frame: &mut Frame, value: &str, block: Block, area: Rect, focused: bool) {
    let inner = block.inner(area);
    let lines = wrap_chars(value, inner.width.max(1) as usize);
    let cursor_row = lines.len().saturating_sub(1) as u16;
    let cursor_column = lines.last().map_or(0, |line| line.chars().count()) as u16;
    let scroll = (cursor_row + 1).saturating_sub(inner.height);
    let lines = lines.into_iter().map(Line::from).collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)).block(block), area);
    if focused && inner.height > 0 {
        frame.set_cursor_position((inner.x + cursor_column, inner.y + cursor_row - scroll));
    }
}

/// Splits `value` into rows of at most `width` characters. A full last row is
/// followed by an empty one, which is where the cursor goes next.
fn wrap_chars(value: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    for character in value.chars() {
        if lines
            .last()
            .is_some_and(|line| line.chars().count() == width)
        {
            lines.push(String::new());
        }
        lines.last_mut().unwrap().push(character);
    }
    if lines
        .last()
        .is_some_and(|line| line.chars().count() == width)
    {
        lines.push(String::new());
    }
    lines
}

fn render_details_dialog(frame: &mut Frame, app: &App) {
    let area = centered_rect(72, 18, frame.area());
    frame.render_widget(Clear, area);
    let lines = app.selected_task().map(task_lines).unwrap_or_default();
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .title(" Task details - d done, e describe, y copy, Enter/Esc close ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(ACCENT)),
        ),
        area,
    );
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::new(
        Direction::Vertical,
        [
            Constraint::Fill(1),
            Constraint::Length(height.min(area.height)),
            Constraint::Fill(1),
        ],
    )
    .split(area);
    Layout::new(
        Direction::Horizontal,
        [
            Constraint::Fill(1),
            Constraint::Length(width.min(vertical[1].width)),
            Constraint::Fill(1),
        ],
    )
    .split(vertical[1])[1]
}

fn labelled(label: &'static str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::default().fg(Color::Gray)),
        Span::raw(value),
    ])
}

fn key_style() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

fn github_style(status: GithubStatus) -> Style {
    let color = match status {
        GithubStatus::Unknown => Color::Gray,
        GithubStatus::NotPushed => Color::DarkGray,
        GithubStatus::BranchPushed => Color::Cyan,
        GithubStatus::DraftPr => Color::Yellow,
        GithubStatus::OpenPr => Color::Green,
        GithubStatus::ClosedPr => Color::Red,
        GithubStatus::Merged => Color::Magenta,
    };
    Style::default().fg(color)
}

fn format_timestamp(timestamp: i64) -> String {
    DateTime::from_timestamp(timestamp, 0)
        .map(|date_time| {
            date_time
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "Unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_input_at_character_boundaries() {
        assert_eq!(wrap_chars("", 4), [""]);
        assert_eq!(wrap_chars("abc de", 4), ["abc ", "de"]);
        assert_eq!(wrap_chars("abcd", 4), ["abcd", ""]);
    }
}
