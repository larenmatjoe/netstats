#![allow(dead_code)]

use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, Paragraph},
};

pub struct NetworkStats {
    pub total_bytes: usize,
    pub current_throughput: usize,
    pub packets_captured: usize,
}

impl Default for NetworkStats {
    fn default() -> Self {
        Self {
            total_bytes: 0,
            current_throughput: 0,
            packets_captured: 0,
        }
    }
}

pub struct DataPoint {
    pub time: f64,
    pub value: f64,
}

pub struct AppState {
    pub stats: NetworkStats,
    pub throughput_history: VecDeque<DataPoint>,
    pub packet_size_history: VecDeque<DataPoint>,
    pub start_time: Instant,
    pub last_update: Instant,
    pub running: bool,
    pub window_size: usize,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            stats: NetworkStats::default(),
            throughput_history: VecDeque::with_capacity(100),
            packet_size_history: VecDeque::with_capacity(100),
            start_time: Instant::now(),
            last_update: Instant::now(),
            running: true,
            window_size: 60, // 60 seconds of data
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_stats(&mut self, packet_size: usize) {
        self.stats.total_bytes += packet_size;
        self.stats.packets_captured += 1;
        self.stats.current_throughput = packet_size;

        let now = Instant::now();
        let elapsed = now.duration_since(self.start_time).as_secs_f64();

        // Add throughput data point
        self.throughput_history.push_back(DataPoint {
            time: elapsed,
            value: (packet_size as f64) / 1024.0, // KB
        });

        // Add packet size data point
        self.packet_size_history.push_back(DataPoint {
            time: elapsed,
            value: packet_size as f64,
        });

        // Maintain window size
        while self.throughput_history.len() > self.window_size {
            self.throughput_history.pop_front();
        }

        while self.packet_size_history.len() > self.window_size {
            self.packet_size_history.pop_front();
        }

        self.last_update = now;
    }
}

pub struct NetworkPlotter {
    state: Arc<Mutex<AppState>>,
}

impl NetworkPlotter {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(AppState::new())),
        }
    }

    pub fn get_state(&self) -> Arc<Mutex<AppState>> {
        Arc::clone(&self.state)
    }

    pub fn update(&self, packet_size: usize) {
        if let Ok(mut state) = self.state.lock() {
            state.update_stats(packet_size);
        }
    }

    pub fn start_ui(self) -> io::Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let res = self.run_app(&mut terminal);

        // Restore terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        if let Err(err) = res {
            println!("{:?}", err);
        }

        Ok(())
    }

    fn run_app<B: Backend>(&self, terminal: &mut Terminal<B>) -> io::Result<()> {
        let state_clone = Arc::clone(&self.state);

        loop {
            terminal.draw(|f| self.ui(f, &state_clone))?;

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if let Ok(mut state) = state_clone.lock() {
                        match key.code {
                            KeyCode::Char('q') => {
                                state.running = false;
                                return Ok(());
                            }
                            KeyCode::Char('+') => {
                                state.window_size = state.window_size.saturating_add(10);
                            }
                            KeyCode::Char('-') => {
                                state.window_size = state.window_size.saturating_sub(10).max(10);
                            }
                            _ => {}
                        }
                    }
                }
            }

            if let Ok(state) = state_clone.lock() {
                if !state.running {
                    break;
                }
            }
        }

        Ok(())
    }

    fn ui<B: Backend>(&self, f: &mut Frame<B>, state_arc: &Arc<Mutex<AppState>>) {
        let size = f.size();

        // Lock state only when needed to minimize contention
        let Ok(state) = state_arc.lock() else {
            return;
        };

        // Create the layout
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
            .split(size);

        // Key bindings help text with prettier styling
        let help_text = vec![
            Span::raw("Press "),
            Span::styled(
                "q",
                Style::default()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" to quit, "),
            Span::styled(
                "+ -",
                Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" to change window size"),
        ];
        let help_paragraph =
            Paragraph::new(Line::from(help_text)).block(Block::default().borders(Borders::NONE));
        f.render_widget(help_paragraph, chunks[0]);

        // Render the stats with charts
        self.render_stats(f, chunks[1], &state);
    }

    fn render_stats<B: Backend>(&self, f: &mut Frame<B>, area: Rect, state: &AppState) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(0)
            .constraints([Constraint::Ratio(1, 3), Constraint::Ratio(2, 3)].as_ref())
            .split(area);

        let top_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)].as_ref())
            .split(chunks[0]);

        // Total bytes captured with enhanced styling
        let total_bytes = state.stats.total_bytes as f64;
        let (size_value, size_unit) = if total_bytes >= 1024.0 * 1024.0 * 1024.0 {
            (total_bytes / (1024.0 * 1024.0 * 1024.0), "GB")
        } else {
            (total_bytes / (1024.0 * 1024.0), "MB")
        };

        let total_bytes_text = vec![Line::from(vec![
            Span::styled("Total Data Captured: ", Style::default().fg(Color::Green)),
            Span::styled(
                format!("{:.2} {}", size_value, size_unit),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ])];
        let total_bytes_paragraph = Paragraph::new(total_bytes_text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(Span::styled(
                    "Total Data",
                    Style::default()
                        .fg(Color::LightCyan)
                        .add_modifier(Modifier::BOLD),
                )),
        );
        f.render_widget(total_bytes_paragraph, top_chunks[0]);

        // Packets captured with enhanced styling
        let packets_text = vec![Line::from(vec![
            Span::styled("Packets Captured: ", Style::default().fg(Color::Magenta)),
            Span::styled(
                state.stats.packets_captured.to_string(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ])];
        let packets_paragraph = Paragraph::new(packets_text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Magenta))
                .title(Span::styled(
                    "Packet Count",
                    Style::default()
                        .fg(Color::LightMagenta)
                        .add_modifier(Modifier::BOLD),
                )),
        );
        f.render_widget(packets_paragraph, top_chunks[1]);

        // Render the packet size chart at the bottom
        self.render_packet_size_chart(f, chunks[1], state);
    }

    fn render_packet_size_chart<B: Backend>(&self, f: &mut Frame<B>, area: Rect, state: &AppState) {
        if state.packet_size_history.is_empty() {
            let message = Paragraph::new("No data available yet...").block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Blue))
                    .title(Span::styled(
                        "Packet Sizes",
                        Style::default()
                            .fg(Color::LightBlue)
                            .add_modifier(Modifier::BOLD),
                    )),
            );
            f.render_widget(message, area);
            return;
        }

        // Prepare data for chart
        let data: Vec<(f64, f64)> = state
            .packet_size_history
            .iter()
            .map(|point| (point.time, point.value))
            .collect();

        // Calculate x-axis boundaries
        let x_min = if let Some(first) = state.packet_size_history.front() {
            first.time
        } else {
            0.0
        };
        let x_max = if let Some(last) = state.packet_size_history.back() {
            last.time
        } else {
            60.0
        };

        // Calculate y-axis boundaries
        let y_max = state
            .packet_size_history
            .iter()
            .map(|point| point.value)
            .fold(1.0_f64, |max_val: f64, val| max_val.max(val))
            * 1.2;

        // Use a prettier dot and color gradient for the dataset
        let datasets = vec![
            Dataset::default()
                .name("Packet Size (bytes)")
                .marker(symbols::Marker::Braille)
                .graph_type(ratatui::widgets::GraphType::Line)
                .style(Style::default().fg(Color::LightBlue))
                .data(&data),
        ];

        let chart = Chart::new(datasets)
            .block(
                Block::default()
                    .title(Span::styled(
                        "Packet Sizes Over Time",
                        Style::default()
                            .fg(Color::LightBlue)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Blue)),
            )
            .x_axis(
                Axis::default()
                    .title(Span::styled(
                        "Time (s)",
                        Style::default()
                            .fg(Color::LightRed)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .style(Style::default().fg(Color::Gray))
                    .bounds([x_min, x_max])
                    .labels(
                        [
                            Span::styled(
                                format!("{:.0}", x_min),
                                Style::default().fg(Color::Yellow),
                            ),
                            Span::styled(
                                format!("{:.0}", (x_min + x_max) / 2.0),
                                Style::default().fg(Color::Yellow),
                            ),
                            Span::styled(
                                format!("{:.0}", x_max),
                                Style::default().fg(Color::Yellow),
                            ),
                        ]
                        .to_vec(),
                    ),
            )
            .y_axis(
                Axis::default()
                    .title(Span::styled(
                        "Bytes",
                        Style::default()
                            .fg(Color::LightGreen)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .style(Style::default().fg(Color::Gray))
                    .bounds([0.0, y_max])
                    .labels(
                        [
                            Span::styled("0", Style::default().fg(Color::Yellow)),
                            Span::styled(
                                format!("{:.0}", y_max / 2.0),
                                Style::default().fg(Color::Yellow),
                            ),
                            Span::styled(
                                format!("{:.0}", y_max),
                                Style::default().fg(Color::Yellow),
                            ),
                        ]
                        .to_vec(),
                    ),
            );

        f.render_widget(chart, area);
    }
}

