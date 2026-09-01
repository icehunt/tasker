use chrono::{DateTime, Local};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::prelude::Stylize;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Row, Table, TableState, Wrap};

use crate::app::{App, Mode};
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
    let area = centered_rect(64, 7, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(" Create task ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let sections = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .split(inner);
    frame.render_widget(Paragraph::new("Task title"), sections[0]);
    frame.render_widget(
        Paragraph::new(app.input.as_str()).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Gray)),
        ),
        sections[1],
    );
    frame.render_widget(
        Paragraph::new("Enter create  |  Esc cancel").alignment(Alignment::Right),
        sections[2],
    );
    let cursor_x = sections[1].x + 1 + app.input.chars().count() as u16;
    frame.set_cursor_position((
        cursor_x.min(sections[1].right().saturating_sub(2)),
        sections[1].y + 1,
    ));
}

fn render_details_dialog(frame: &mut Frame, app: &App) {
    let area = centered_rect(72, 18, frame.area());
    frame.render_widget(Clear, area);
    let lines = app.selected_task().map(task_lines).unwrap_or_default();
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .title(" Task details - y copy, Enter/Esc close ")
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
