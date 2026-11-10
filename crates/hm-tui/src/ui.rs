//! Rendering. One function, called once per tick; it never mutates
//! [`App`], it only reads it -- key handling and background refresh are
//! [`crate::app::App`]'s job, not this one's.

use crate::app::{App, Tab};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs},
    Frame,
};

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // tab bar
            Constraint::Min(3),    // table
            Constraint::Length(1), // status line
            Constraint::Length(1), // help line
        ])
        .split(area);

    draw_tabs(frame, chunks[0], app);
    match app.tab {
        Tab::Vms => draw_vms(frame, chunks[1], app),
        Tab::Sandboxes => draw_sandboxes(frame, chunks[1], app),
    }
    draw_status(frame, chunks[2], app);
    draw_help(frame, chunks[3]);
}

fn draw_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let titles = [Tab::Vms, Tab::Sandboxes].map(|t| t.title());
    let selected = match app.tab {
        Tab::Vms => 0,
        Tab::Sandboxes => 1,
    };
    let tabs = Tabs::new(titles.to_vec())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" HyperMachine "),
        )
        .select(selected)
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(tabs, area);
}

fn state_color(state: &str) -> Color {
    match state {
        "running" => Color::Green,
        "paused" => Color::Yellow,
        "error" => Color::Red,
        "stopped" => Color::DarkGray,
        _ => Color::White,
    }
}

fn draw_vms(frame: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(["NAME", "STATE", "CPUS", "MEMORY", "IP"])
        .style(Style::default().add_modifier(Modifier::BOLD));
    let rows = app.vms.iter().enumerate().map(|(i, vm)| {
        let selected = i == app.selected;
        let state = vm.state.to_string();
        let cells = [
            Cell::from(vm.name.clone()),
            Cell::from(state.clone()).style(Style::default().fg(state_color(&state))),
            Cell::from(vm.cpus.to_string()),
            Cell::from(format!("{} MB", vm.memory_mb)),
            Cell::from(vm.ip_address.clone().unwrap_or_default()),
        ];
        let row = Row::new(cells);
        if selected {
            row.style(Style::default().add_modifier(Modifier::REVERSED))
        } else {
            row
        }
    });

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(30),
            Constraint::Length(10),
            Constraint::Length(6),
            Constraint::Length(10),
            Constraint::Percentage(30),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Type 2 VMs ({}) ", app.vms.len())),
    );
    frame.render_widget(table, area);

    if app.vms.is_empty() {
        let empty = Paragraph::new("no VMs, or the backend at this URL is unreachable");
        frame.render_widget(empty, inset(area));
    }
}

fn draw_sandboxes(frame: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(["SANDBOX ID", "TEMPLATE", "STATE", "CPUS", "MEMORY", "NODE"])
        .style(Style::default().add_modifier(Modifier::BOLD));
    let rows = app.sandboxes.iter().enumerate().map(|(i, sb)| {
        let selected = i == app.selected;
        let cells = [
            Cell::from(sb.sandbox_id.clone()),
            Cell::from(sb.template_id.clone()),
            Cell::from(sb.state.clone()).style(Style::default().fg(state_color(&sb.state))),
            Cell::from(sb.cpu_count.to_string()),
            Cell::from(format!("{} MB", sb.memory_mb)),
            Cell::from(sb.node_id.clone()),
        ];
        let row = Row::new(cells);
        if selected {
            row.style(Style::default().add_modifier(Modifier::REVERSED))
        } else {
            row
        }
    });

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(25),
            Constraint::Percentage(20),
            Constraint::Length(10),
            Constraint::Length(6),
            Constraint::Length(10),
            Constraint::Percentage(20),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Sandbox VMs ({}) ", app.sandboxes.len())),
    );
    frame.render_widget(table, area);

    if app.sandboxes.is_empty() {
        let empty = Paragraph::new("no sandboxes, or no sandboxd node at this URL");
        frame.render_widget(empty, inset(area));
    }
}

fn inset(area: Rect) -> Rect {
    Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

fn draw_status(frame: &mut Frame, area: Rect, app: &App) {
    let style = if app.status.contains("error") || app.confirm_delete.is_some() {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Gray)
    };
    let line = Line::from(Span::styled(app.status.clone(), style));
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_help(frame: &mut Frame, area: Rect) {
    let help = "Tab/←→ switch  ↑↓ select  s start/stop  d delete (press twice)  r refresh  q quit";
    frame.render_widget(
        Paragraph::new(help).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}
