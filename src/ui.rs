//! Rendu Ratatui et boucle d'événements de la TUI.
//!
//! Charte LLAStudio : citron `#FACC15`, lime `#84CC16`, fond sombre, rendu
//! net et minimaliste. Le terminal est toujours restauré (sortie normale ou
//! panique) pour ne jamais laisser l'utilisateur dans un écran alterné cassé.

use std::io::{self, stdout};

use anyhow::Result;
use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Padding, Paragraph, Row, Table, TableState, Wrap,
};
use ratatui::{Frame, Terminal};

use crate::app::{App, ToastKind};
use crate::scanner::PortEntry;

const LEMON: Color = Color::Rgb(250, 204, 21);
const LIME: Color = Color::Rgb(132, 204, 22);
const BG: Color = Color::Rgb(12, 15, 10);
const SURFACE: Color = Color::Rgb(24, 29, 19);
const BORDER: Color = Color::Rgb(58, 68, 40);
const TEXT: Color = Color::Rgb(232, 238, 222);
const MUTED: Color = Color::Rgb(112, 122, 100);
const DANGER: Color = Color::Rgb(239, 68, 68);
const INK: Color = Color::Rgb(24, 22, 6);
const INK_DIM: Color = Color::Rgb(76, 68, 18);

type Backend = CrosstermBackend<io::Stdout>;

/// Exécute la boucle TUI jusqu'à `Q` / `Échap`, puis restaure le terminal.
pub fn run(app: &mut App) -> Result<()> {
    let mut terminal = init_terminal()?;
    let result = event_loop(&mut terminal, app);
    restore_terminal();
    result
}

fn event_loop(terminal: &mut Terminal<Backend>, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|frame| draw(frame, app))?;

        if event::poll(std::time::Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => app.handle_key(key),
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        app.tick();

        if app.should_quit() {
            break;
        }
    }
    Ok(())
}

fn init_terminal() -> Result<Terminal<Backend>> {
    enable_raw_mode()?;
    let mut out = stdout();
    if let Err(error) = execute!(out, EnterAlternateScreen, Hide) {
        let _ = disable_raw_mode();
        return Err(error.into());
    }

    let backend = CrosstermBackend::new(out);
    let terminal = match Terminal::new(backend) {
        Ok(terminal) => terminal,
        Err(error) => {
            restore_terminal();
            return Err(error.into());
        }
    };

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous_hook(info);
    }));

    Ok(terminal)
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let mut out = stdout();
    let _ = execute!(out, LeaveAlternateScreen, Show);
}

fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(Block::default().style(Style::new().bg(BG).fg(TEXT)), area);

    let [header, table, toast, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(4),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    let visible = app.visible();
    draw_header(frame, header, app, &visible);
    draw_table(frame, table, app, &visible);
    draw_toast(frame, toast, app, &visible);
    draw_footer(frame, footer);
}

fn draw_header(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, visible: &[&PortEntry]) {
    let [brand_row, info_row, rule_row] = Layout::vertical([Constraint::Length(1); 3]).areas(area);
    let [brand_area, count_area] =
        Layout::horizontal([Constraint::Min(20), Constraint::Length(40)]).areas(brand_row);
    let [info_area, filter_area] =
        Layout::horizontal([Constraint::Min(20), Constraint::Length(40)]).areas(info_row);

    let brand = Line::from(vec![
        Span::raw("🍋 "),
        Span::styled(
            "LemonPort",
            Style::new().fg(LEMON).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  by LLAStudio", Style::new().fg(MUTED)),
    ]);
    frame.render_widget(Paragraph::new(brand), brand_area);

    let count = if app.hidden_system() > 0 && !app.show_all() {
        format!(
            "{} actif{} · {} système masqué{}",
            visible.len(),
            plural(visible.len()),
            app.hidden_system(),
            plural(app.hidden_system())
        )
    } else {
        format!(
            "{} port{} actif{}",
            visible.len(),
            plural(visible.len()),
            plural(visible.len())
        )
    };
    frame.render_widget(
        Paragraph::new(count)
            .style(Style::new().fg(LIME).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Right),
        count_area,
    );

    let mut info = vec![
        Span::styled("TCP", Style::new().fg(LIME).add_modifier(Modifier::BOLD)),
        Span::styled(" · LISTEN", Style::new().fg(MUTED)),
        Span::styled(" · scan auto 2 s", Style::new().fg(MUTED)),
    ];
    if app.hidden_system() > 0 && !app.show_all() {
        info.push(Span::styled(
            " · --all pour tout voir",
            Style::new().fg(MUTED),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(info)), info_area);

    let filter_line = if app.search_mode() {
        let shown = if app.filter().is_empty() {
            "tapez pour filtrer…".to_string()
        } else {
            app.filter().to_string()
        };
        Line::from(vec![
            Span::styled("/", Style::new().fg(LEMON).add_modifier(Modifier::BOLD)),
            Span::styled(shown, Style::new().fg(TEXT)),
            Span::styled("▏", Style::new().fg(LEMON)),
        ])
    } else if app.filter().is_empty() {
        Line::from(vec![
            Span::styled("/", Style::new().fg(LEMON).add_modifier(Modifier::BOLD)),
            Span::styled(" filtrer", Style::new().fg(MUTED)),
        ])
    } else {
        Line::from(vec![
            Span::styled("filtre : ", Style::new().fg(MUTED)),
            Span::styled(app.filter().to_string(), Style::new().fg(LEMON)),
            Span::styled("  (/ modifier · Échap effacer)", Style::new().fg(MUTED)),
        ])
    };
    frame.render_widget(
        Paragraph::new(filter_line).alignment(Alignment::Right),
        filter_area,
    );

    frame.render_widget(
        Paragraph::new("─".repeat(area.width as usize)).style(Style::new().fg(BORDER)),
        rule_row,
    );
}

fn draw_table(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, visible: &[&PortEntry]) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(BORDER))
        .padding(Padding::horizontal(1))
        .title(Line::from(vec![
            Span::styled(
                " PORTS EN ÉCOUTE ",
                Style::new().fg(LEMON).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("{} ", visible.len()), Style::new().fg(MUTED)),
        ]));

    if visible.is_empty() {
        let (title, hint) = if !app.filter().is_empty() {
            (
                format!("Aucun résultat pour « {} »", app.filter()),
                "Échap pour effacer le filtre".to_string(),
            )
        } else if app.hidden_system() > 0 && !app.show_all() {
            (
                "Aucun port de développement en écoute".to_string(),
                format!(
                    "{} port{} système masqué{} — relancez avec --all",
                    app.hidden_system(),
                    plural(app.hidden_system()),
                    plural(app.hidden_system())
                ),
            )
        } else {
            (
                "Aucun port TCP en écoute".to_string(),
                "Lancez un serveur, puis appuyez sur R".to_string(),
            )
        };

        let inner = block.inner(area);
        frame.render_widget(block, area);
        let message_area = if inner.height >= 4 {
            let [_, middle, _] = Layout::vertical([
                Constraint::Fill(1),
                Constraint::Length(2),
                Constraint::Fill(1),
            ])
            .areas(inner);
            middle
        } else {
            inner
        };
        let message = Paragraph::new(vec![
            Line::from(Span::styled(
                title,
                Style::new().fg(TEXT).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(hint, Style::new().fg(MUTED))),
        ])
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
        frame.render_widget(message, message_area);
        return;
    }

    let pending_pid = app.pending_pid();
    let selected = app.selected_index();
    let rows: Vec<Row> = visible
        .iter()
        .enumerate()
        .map(|(index, entry)| row_for(entry, index == selected, pending_pid == entry.pid))
        .collect();

    let header = Row::new(vec![
        Cell::from("PORT"),
        Cell::from("PROJET / PROCESSUS"),
        Cell::from("PID"),
        Cell::from("RAM"),
        Cell::from("STATUT"),
    ])
    .style(Style::new().fg(LIME).add_modifier(Modifier::BOLD));

    let table = Table::new(
        rows,
        [
            Constraint::Length(9),
            Constraint::Min(24),
            Constraint::Length(7),
            Constraint::Length(8),
            Constraint::Length(15),
        ],
    )
    .header(header)
    .block(block)
    .column_spacing(1);

    let mut state = TableState::default();
    state.select(Some(selected));
    frame.render_stateful_widget(table, area, &mut state);
}

fn row_for(entry: &PortEntry, selected: bool, pending: bool) -> Row<'static> {
    let primary = if selected { INK } else { TEXT };
    let accent = if selected { INK } else { LEMON };
    let secondary = if selected { INK_DIM } else { MUTED };

    let port = Cell::from(Line::from(vec![
        Span::styled(if selected { "▸ " } else { "  " }, Style::new().fg(accent)),
        Span::styled(
            format!(":{}", entry.port),
            Style::new().fg(accent).add_modifier(Modifier::BOLD),
        ),
    ]));

    let project = match &entry.project {
        Some(project) => Cell::from(Line::from(vec![
            Span::styled(
                project.name.clone(),
                Style::new().fg(primary).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" ({})", project.label()), Style::new().fg(accent)),
            Span::styled(format!(" · {}", entry.process), Style::new().fg(secondary)),
        ])),
        None => Cell::from(Line::from(Span::styled(
            entry.process.clone(),
            Style::new().fg(primary),
        ))),
    };

    let pid = Cell::from(Line::from(Span::styled(
        entry
            .pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "—".to_string()),
        Style::new().fg(if entry.protected { secondary } else { primary }),
    )));

    let ram = Cell::from(Line::from(Span::styled(
        if entry.memory_mb > 0.0 {
            format!("{:.0} Mo", entry.memory_mb)
        } else {
            "—".to_string()
        },
        Style::new().fg(secondary),
    )));

    let (status, color) = if pending {
        ("TERM…", DANGER)
    } else if entry.protected {
        ("PROTÉGÉ", LEMON)
    } else if entry.system {
        ("SYSTÈME", MUTED)
    } else {
        ("ACTIF", LIME)
    };
    let status = Cell::from(Line::from(vec![
        Span::styled(
            status,
            Style::new()
                .fg(if selected { INK } else { color })
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                " {}",
                if entry.uptime_seconds > 0 {
                    format_uptime(entry.uptime_seconds)
                } else {
                    "—".to_string()
                }
            ),
            Style::new().fg(secondary),
        ),
    ]));

    Row::new(vec![port, project, pid, ram, status]).style(Style::new().bg(if selected {
        LEMON
    } else {
        BG
    }))
}

fn draw_toast(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, visible: &[&PortEntry]) {
    let line = match app.toast() {
        Some(toast) => {
            let (icon, color) = match toast.kind {
                ToastKind::Info => ("●", LEMON),
                ToastKind::Success => ("🍋", LIME),
                ToastKind::Error => ("✗", DANGER),
            };
            Line::from(vec![
                Span::raw(" "),
                Span::styled(icon, Style::new().fg(color).add_modifier(Modifier::BOLD)),
                Span::raw(" "),
                Span::styled(toast.message.as_str(), Style::new().fg(color)),
            ])
        }
        None => {
            let summary = visible.get(app.selected_index()).map(|entry| {
                let label = entry
                    .project
                    .as_ref()
                    .map(|project| format!("{} ({})", project.name, project.label()))
                    .unwrap_or_else(|| entry.process.clone());
                format!(
                    " Sélection : :{} · {label} — Entrée pour libérer · O pour ouvrir",
                    entry.port
                )
            });
            Line::from(Span::styled(
                summary.unwrap_or_default(),
                Style::new().fg(MUTED),
            ))
        }
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_footer(frame: &mut Frame, area: ratatui::layout::Rect) {
    let line = Line::from(vec![
        Span::raw(" "),
        keycap("↑↓"),
        Span::styled(" naviguer  ", Style::new().fg(MUTED)),
        keycap("Entrée/K"),
        Span::styled(" tuer  ", Style::new().fg(MUTED)),
        keycap("/"),
        Span::styled(" filtrer  ", Style::new().fg(MUTED)),
        keycap("O"),
        Span::styled(" ouvrir  ", Style::new().fg(MUTED)),
        keycap("C"),
        Span::styled(" copier  ", Style::new().fg(MUTED)),
        keycap("R"),
        Span::styled(" rafraîchir  ", Style::new().fg(MUTED)),
        keycap("Q"),
        Span::styled(" quitter", Style::new().fg(MUTED)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn keycap(label: &str) -> Span<'static> {
    Span::styled(
        format!(" {label} "),
        Style::new()
            .fg(LEMON)
            .bg(SURFACE)
            .add_modifier(Modifier::BOLD),
    )
}

fn plural(count: usize) -> &'static str {
    if count > 1 {
        "s"
    } else {
        ""
    }
}

fn format_uptime(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3_600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h{:02}", seconds / 3_600, (seconds % 3_600) / 60)
    } else {
        format!("{}j{}h", seconds / 86_400, (seconds % 86_400) / 3_600)
    }
}
