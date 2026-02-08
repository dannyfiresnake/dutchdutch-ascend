use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use dutchdutch_ascend::{
    discovery::DiscoveryEvent, AscendClient, Discovery, Room,
};
use tokio::sync::broadcast;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io;

#[derive(PartialEq)]
enum AppState {
    Discovery,
    RoomControl,
}

struct App {
    state: AppState,
    discovery: Discovery,
    selected_room_index: usize,
    connected_client: Option<AscendClient>,
    selected_room_id: Option<uuid::Uuid>,
    status_message: String,
    update_receiver: Option<broadcast::Receiver<DiscoveryEvent>>,
    json_cursor: usize,
    json_scroll: usize,
}

impl App {
    fn new() -> Self {
        let discovery = Discovery::new();
        let update_receiver = discovery.subscribe();

        Self {
            state: AppState::Discovery,
            discovery,
            selected_room_index: 0,
            connected_client: None,
            selected_room_id: None,
            status_message: "Discovering rooms...".to_string(),
            update_receiver: Some(update_receiver),
            json_cursor: 0,
            json_scroll: 0,
        }
    }

    fn select_next(&mut self) {
        let room_count = self.discovery.room_count();
        if room_count > 0 {
            self.selected_room_index = (self.selected_room_index + 1) % room_count;
        }
    }

    fn select_previous(&mut self) {
        let room_count = self.discovery.room_count();
        if room_count > 0 {
            if self.selected_room_index == 0 {
                self.selected_room_index = room_count - 1;
            } else {
                self.selected_room_index -= 1;
            }
        }
    }

    async fn connect_to_selected_room(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let rooms = self.discovery.rooms();
        if rooms.is_empty() {
            self.status_message = "No rooms to connect to".to_string();
            return Ok(());
        }

        let room = &rooms[self.selected_room_index];
        self.status_message = format!("Selecting {}...", room.name());

        // Store just the ID, we'll get the room from discovery each time
        self.selected_room_id = Some(room.id());
        self.state = AppState::RoomControl;
        self.status_message = "Connected! Use +/- for volume, m for mute, q to quit, Esc to go back".to_string();

        Ok(())
    }

    fn get_current_room(&self) -> Option<Room> {
        let room_id = self.selected_room_id?;
        self.discovery.rooms().into_iter().find(|r| r.id() == room_id)
    }

    async fn adjust_volume(&mut self, delta: f64) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(room) = self.get_current_room() {
            let current_gain = room.gain().global;
            let new_gain = (current_gain + delta).clamp(-80.0, 10.0);

            if let Err(e) = room.set_gain(new_gain).await {
                self.status_message = format!("Failed to set gain: {}", e);
            } else {
                self.status_message = format!("Volume: {:.1} dB", new_gain);
            }
        } else {
            self.status_message = "No room connected".to_string();
        }
        Ok(())
    }

    async fn toggle_mute(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(room) = self.get_current_room() {
            let new_mute = !room.mute().global;
            if let Err(e) = room.set_mute(new_mute).await {
                self.status_message = format!("Failed to set mute: {}", e);
            } else {
                self.status_message = format!("Mute: {}", if new_mute { "ON" } else { "OFF" });
            }
        } else {
            self.status_message = "No room connected".to_string();
        }
        Ok(())
    }

    async fn toggle_standby(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(room) = self.get_current_room() {
            let new_standby = !room.sleep();

            if let Err(e) = room.set_standby(new_standby).await {
                self.status_message = format!("Failed to set standby: {}", e);
            } else {
                self.status_message = format!("Standby: {}", if new_standby { "ON" } else { "OFF" });
            }
        } else {
            self.status_message = "No room connected".to_string();
        }
        Ok(())
    }

    async fn cycle_input(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(room) = self.get_current_room() {
            let inputs = room.input_modes();

            if inputs.is_empty() {
                self.status_message = "No regular inputs available".to_string();
                return Ok(());
            }

            let current = room.selected_input().unwrap_or_else(|| inputs[0].clone());
            let current_idx = inputs.iter().position(|i| i == &current).unwrap_or(0);
            let next_idx = (current_idx + 1) % inputs.len();
            let next_input = inputs[next_idx].clone();

            if let Err(e) = room.set_input(&next_input).await {
                self.status_message = format!("Failed to set input: {}", e);
            } else {
                self.status_message = format!("Input: {}", next_input);
            }
        } else {
            self.status_message = "No room connected".to_string();
        }
        Ok(())
    }

    async fn cycle_xlr_mode(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(room) = self.get_current_room() {
            let xlr_modes = room.xlr_input_modes();

            if xlr_modes.is_empty() {
                self.status_message = "No XLR modes available".to_string();
                return Ok(());
            }

            let current = room.selected_xlr().unwrap_or_else(|| xlr_modes[0].clone());
            let current_idx = xlr_modes.iter().position(|i| i == &current).unwrap_or(0);
            let next_idx = (current_idx + 1) % xlr_modes.len();
            let next_mode = xlr_modes[next_idx].clone();

            if let Err(e) = room.set_xlr_mode(&next_mode).await {
                self.status_message = format!("Failed to set XLR mode: {}", e);
            } else {
                self.status_message = format!("XLR mode: {}", next_mode);
            }
        } else {
            self.status_message = "No room connected".to_string();
        }
        Ok(())
    }

    async fn toggle_linear_phase(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(room) = self.get_current_room() {
            let new_linear_phase = !room.linear_phase();

            if let Err(e) = room.set_linear_phase(new_linear_phase).await {
                self.status_message = format!("Failed to set linear phase: {}", e);
            } else {
                self.status_message = format!("Linear Phase: {}", if new_linear_phase { "ON" } else { "OFF" });
            }
        } else {
            self.status_message = "No room connected".to_string();
        }
        Ok(())
    }

    async fn handle_state_update(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(receiver) = &mut self.update_receiver {
            match receiver.try_recv() {
                Ok(event) => {
                    match event {
                        DiscoveryEvent::RoomAdded(room_id) => {
                            self.status_message = format!("New room discovered: {}", room_id);
                        }
                        DiscoveryEvent::RoomUpdated(room_id) => {
                            // Check if this is the room we're currently viewing
                            if self.selected_room_id == Some(room_id) {
                                // Room will automatically show updated state on next render
                                self.status_message = "State updated from network".to_string();
                            }
                        }
                        DiscoveryEvent::RoomRemoved(room_id) => {
                            self.status_message = format!("Room removed: {}", room_id);
                            // If we're viewing this room, go back to discovery
                            if self.selected_room_id == Some(room_id) {
                                self.go_back();
                            }
                        }
                    }
                }
                Err(broadcast::error::TryRecvError::Empty) => {
                    // No update available
                }
                Err(_) => {
                    // Connection closed or lagged
                }
            }
        }
        Ok(())
    }

    fn go_back(&mut self) {
        self.state = AppState::Discovery;
        self.connected_client = None;
        self.selected_room_id = None;
        self.json_cursor = 0;
        self.json_scroll = 0;
        self.status_message = format!("Discovered {} room(s). Press Enter to connect.", self.discovery.room_count());
    }

    fn json_cursor_down(&mut self, max_lines: usize, visible_height: usize) {
        if self.json_cursor + 1 < max_lines {
            self.json_cursor += 1;
            // Update scroll if cursor goes below visible area
            if self.json_cursor >= self.json_scroll + visible_height {
                self.json_scroll = self.json_cursor.saturating_sub(visible_height - 1);
            }
        }
    }

    fn json_cursor_up(&mut self) {
        if self.json_cursor > 0 {
            self.json_cursor -= 1;
            // Update scroll if cursor goes above visible area
            if self.json_cursor < self.json_scroll {
                self.json_scroll = self.json_cursor;
            }
        }
    }

    fn get_json_line_count(&self) -> usize {
        if let Some(room) = self.get_current_room() {
            let room_json = room.raw_json();
            if let Ok(json_str) = serde_json::to_string_pretty(&room_json) {
                return json_str.lines().count();
            }
        }
        0
    }
}

fn ui(f: &mut Frame, app: &App) {
    let outer_chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(f.size());

    match app.state {
        AppState::Discovery => {
            render_discovery(f, app, outer_chunks[0]);
        }
        AppState::RoomControl => {
            // Split horizontally for room control + JSON dump
            let inner_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(outer_chunks[0]);

            render_room_control(f, app, inner_chunks[0]);
            render_json_dump(f, app, inner_chunks[1]);
        }
    }

    render_status(f, app, outer_chunks[1]);
}

fn render_discovery(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Discovered Rooms (j/k to select, Enter to connect, q to quit) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let rooms = app.discovery.rooms();
    if rooms.is_empty() {
        let text = Paragraph::new("Discovering rooms...\n\nWaiting for Dutch and Dutch speakers on the network.")
            .block(block)
            .wrap(Wrap { trim: true });
        f.render_widget(text, area);
    } else {
        let items: Vec<ListItem> = rooms
            .iter()
            .map(|room| {
                let content = vec![
                    Line::from(vec![
                        Span::styled("Name: ", Style::default().fg(Color::Yellow)),
                        Span::raw(room.name()),
                    ]),
                    Line::from(vec![
                        Span::styled("ID: ", Style::default().fg(Color::Yellow)),
                        Span::raw(format!("{}", room.id())),
                    ]),
                    Line::from(vec![
                        Span::styled("Devices: ", Style::default().fg(Color::Yellow)),
                        Span::raw(format!("{}", room.member_count())),
                    ]),
                    Line::from(""),
                ];
                ListItem::new(content)
            })
            .collect();

        let mut state = ListState::default();
        state.select(Some(app.selected_room_index));

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(">> ");

        f.render_stateful_widget(list, area, &mut state);
    }
}

fn render_room_control(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Room Control (+/- vol, m mute, s standby, i input, x xlr, p linear, Esc back, q quit) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));

    if let Some(room) = app.get_current_room() {
        // Get a consistent snapshot of the room state
        let state = room.state_snapshot();

        let mut lines = vec![
            Line::from(vec![
                Span::styled("Room: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw(&state.name),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Volume: ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    format!("{:.1} dB", state.gain.global),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("Mute: ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    if state.mute.global { "ON" } else { "OFF" },
                    if state.mute.global {
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Green)
                    },
                ),
            ]),
            Line::from(vec![
                Span::styled("Standby: ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    if state.sleep { "ON" } else { "OFF" },
                    if state.sleep {
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Green)
                    },
                ),
            ]),
            Line::from(vec![
                Span::styled("Linear Phase: ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    if state.linear_phase { "ON" } else { "OFF" },
                    if state.linear_phase {
                        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Gray)
                    },
                ),
            ]),
            Line::from(vec![
                Span::styled("Input: ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    state.selected_input.as_deref().unwrap_or("Unknown"),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("XLR Mode: ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    state.selected_xlr.as_deref().unwrap_or("Unknown"),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
        ];

        // Streaming info (now playing)
        if let Some(streaming_info) = &state.streaming_info {
            // Only show if there's actually content playing or display info
            if streaming_info.is_playing || !streaming_info.display.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("Now Playing ({})", streaming_info.service_name),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                )));

                // Display lines (title, artist, album, etc.)
                for (i, line) in streaming_info.display.iter().enumerate() {
                    if !line.is_empty() {
                        let label = match i {
                            0 => "  Title: ",
                            1 => "  Artist: ",
                            2 => "  Album: ",
                            _ => "  ",
                        };
                        lines.push(Line::from(vec![
                            Span::styled(label, Style::default().fg(Color::Yellow)),
                            Span::styled(
                                line,
                                if i == 0 {
                                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                                } else {
                                    Style::default().fg(Color::Cyan)
                                },
                            ),
                        ]));
                    }
                }

                // Playback state
                lines.push(Line::from(vec![
                    Span::styled("  State: ", Style::default().fg(Color::Yellow)),
                    Span::styled(
                        if streaming_info.is_playing { "Playing" } else { "Paused/Stopped" },
                        if streaming_info.is_playing {
                            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::Gray)
                        },
                    ),
                ]));

                // Time position (both already converted to seconds)
                if streaming_info.track_length > 0.0 {
                    let pos_min = (streaming_info.track_position / 60.0).floor() as i32;
                    let pos_sec = (streaming_info.track_position % 60.0).floor() as i32;
                    let dur_min = (streaming_info.track_length / 60.0).floor() as i32;
                    let dur_sec = (streaming_info.track_length % 60.0).floor() as i32;

                    lines.push(Line::from(vec![
                        Span::styled("  Time: ", Style::default().fg(Color::Yellow)),
                        Span::styled(
                            format!("{}:{:02} / {}:{:02}", pos_min, pos_sec, dur_min, dur_sec),
                            Style::default().fg(Color::Cyan),
                        ),
                    ]));
                }

                // Shuffle and repeat
                if streaming_info.shuffle || !streaming_info.repeat.is_empty() {
                    let mut status = Vec::new();
                    if streaming_info.shuffle {
                        status.push("Shuffle");
                    }
                    if !streaming_info.repeat.is_empty() {
                        status.push(&streaming_info.repeat);
                    }
                    lines.push(Line::from(vec![
                        Span::styled("  Mode: ", Style::default().fg(Color::Yellow)),
                        Span::styled(
                            status.join(", "),
                            Style::default().fg(Color::Cyan),
                        ),
                    ]));
                }

                lines.push(Line::from(""));
            }
        }

        // Input modes
        if !state.input_modes.is_empty() {
            lines.push(Line::from(Span::styled("Inputs:", Style::default().fg(Color::Yellow))));
            for input in &state.input_modes {
                let is_active = state.selected_input.as_ref() == Some(input);
                let prefix = if is_active { "  ▶ " } else { "    " };
                lines.push(Line::from(vec![
                    Span::raw(prefix),
                    Span::styled(
                        input,
                        if is_active {
                            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                        },
                    ),
                ]));
            }
            lines.push(Line::from(""));
        }

        // XLR modes
        if !state.xlr_input_modes.is_empty() {
            lines.push(Line::from(Span::styled("XLR Modes:", Style::default().fg(Color::Yellow))));
            for xlr_mode in &state.xlr_input_modes {
                let is_active = state.selected_xlr.as_ref() == Some(xlr_mode);
                let prefix = if is_active { "  ▶ " } else { "    " };
                lines.push(Line::from(vec![
                    Span::raw(prefix),
                    Span::styled(
                        xlr_mode,
                        if is_active {
                            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                        },
                    ),
                ]));
            }
            lines.push(Line::from(""));
        }

        // Tone settings (from selected voicing)
        if let Some(selected_id) = &state.selected_voicing_profile {
            if let Some(voicing_profile) = state.voicing.get(selected_id) {
                lines.push(Line::from(Span::styled("Tone:", Style::default().fg(Color::Yellow))));
                lines.push(Line::from(vec![
                    Span::raw("  Sub:    "),
                    Span::styled(
                        format!("{:.1} dB", voicing_profile.sub),
                        Style::default().fg(Color::Cyan),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::raw("  Bass:   "),
                    Span::styled(
                        format!("{:.1} dB", voicing_profile.bass),
                        Style::default().fg(Color::Cyan),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::raw("  Treble: "),
                    Span::styled(
                        format!("{:.1} dB", voicing_profile.treble),
                        Style::default().fg(Color::Cyan),
                    ),
                ]));
                lines.push(Line::from(""));
            }
        }

        // Voicing profiles
        if !state.voicing.is_empty() {
            lines.push(Line::from(Span::styled("Voicings:", Style::default().fg(Color::Yellow))));
            for (voicing_id, voicing_prof) in &state.voicing {
                let is_active = state.selected_voicing_profile.as_ref() == Some(voicing_id);
                let prefix = if is_active { "  ▶ " } else { "    " };
                lines.push(Line::from(vec![
                    Span::raw(prefix),
                    Span::styled(
                        &voicing_prof.name,
                        if is_active {
                            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                        },
                    ),
                ]));
            }
            lines.push(Line::from(""));
        }

        if !state.presets.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("Presets:", Style::default().fg(Color::Yellow))));
            for (preset_id, preset) in &state.presets {
                let is_active = state.last_selected_preset.as_ref() == Some(preset_id);
                let prefix = if is_active { "  ▶ " } else { "    " };
                lines.push(Line::from(vec![
                    Span::raw(prefix),
                    Span::styled(
                        &preset.name,
                        if is_active {
                            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                        },
                    ),
                ]));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Devices: ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("{}", state.members.len())),
        ]));

        // Show member names if available
        if !state.member_names.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(
                    state.member_names.join(", "),
                    Style::default().fg(Color::Cyan),
                ),
            ]));
        }

        let text = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
        f.render_widget(text, area);
    } else {
        let text = Paragraph::new("Loading room data...")
            .block(block)
            .wrap(Wrap { trim: true });
        f.render_widget(text, area);
    }
}

fn render_json_dump(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Room JSON (j/k scroll) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));

    if let Some(room) = app.get_current_room() {
        let room_json = room.raw_json();
        // Pretty-print the raw JSON
        let json_str = match serde_json::to_string_pretty(&room_json) {
            Ok(json) => json,
            Err(e) => format!("Error serializing JSON: {}", e),
        };

        let json_lines: Vec<&str> = json_str.lines().collect();

        // Create styled lines with cursor highlight
        let styled_lines: Vec<Line> = json_lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                if i == app.json_cursor {
                    Line::from(Span::styled(
                        format!("> {}", line),
                        Style::default().bg(Color::DarkGray).fg(Color::White),
                    ))
                } else {
                    Line::from(format!("  {}", line))
                }
            })
            .collect();

        // Calculate scroll to keep cursor visible
        let height = area.height.saturating_sub(2) as usize; // Subtract borders
        let scroll = if app.json_cursor >= app.json_scroll + height {
            app.json_cursor.saturating_sub(height - 1)
        } else if app.json_cursor < app.json_scroll {
            app.json_cursor
        } else {
            app.json_scroll
        };

        let text = Paragraph::new(styled_lines)
            .block(block)
            .scroll((scroll as u16, 0));

        f.render_widget(text, area);
    } else {
        let text = Paragraph::new("No room data available")
            .block(block)
            .wrap(Wrap { trim: true });

        f.render_widget(text, area);
    }
}

fn render_status(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Status ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Gray));

    let text = Paragraph::new(app.status_message.clone())
        .block(block)
        .wrap(Wrap { trim: true });

    f.render_widget(text, area);
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = App::new();

    // Start discovery
    app.discovery.start().await?;

    // Main loop
    let res = run_app(&mut terminal, &mut app).await;

    // Stop discovery
    app.discovery.stop().await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("Error: {}", err);
    }

    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        // Update status message with room count
        if app.state == AppState::Discovery {
            let room_count = app.discovery.room_count();
            if room_count > 0 {
                app.status_message = format!("Found {} room(s). Press Enter to connect.", room_count);
            }
        }

        // Draw UI
        terminal.draw(|f| ui(f, app))?;

        // Handle state updates from network
        app.handle_state_update().await?;

        // Handle input events (non-blocking)
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match app.state {
                        AppState::Discovery => match key.code {
                            KeyCode::Char('q') => return Ok(()),
                            KeyCode::Char('j') => app.select_next(),
                            KeyCode::Char('k') => app.select_previous(),
                            KeyCode::Down => app.select_next(),
                            KeyCode::Up => app.select_previous(),
                            KeyCode::Enter => {
                                app.connect_to_selected_room().await?;
                            }
                            _ => {}
                        },
                        AppState::RoomControl => match key.code {
                            KeyCode::Char('q') => return Ok(()),
                            KeyCode::Esc => app.go_back(),
                            KeyCode::Char('+') | KeyCode::Char('=') => {
                                app.adjust_volume(1.0).await?;
                            }
                            KeyCode::Char('-') | KeyCode::Char('_') => {
                                app.adjust_volume(-1.0).await?;
                            }
                            KeyCode::Char('m') => {
                                app.toggle_mute().await?;
                            }
                            KeyCode::Char('s') => {
                                app.toggle_standby().await?;
                            }
                            KeyCode::Char('i') => {
                                app.cycle_input().await?;
                            }
                            KeyCode::Char('x') => {
                                app.cycle_xlr_mode().await?;
                            }
                            KeyCode::Char('p') => {
                                app.toggle_linear_phase().await?;
                            }
                            KeyCode::Char('j') => {
                                let line_count = app.get_json_line_count();
                                app.json_cursor_down(line_count, 20); // Assume ~20 lines visible
                            }
                            KeyCode::Char('k') => {
                                app.json_cursor_up();
                            }
                            _ => {}
                        },
                    }
                }
            }
        }
    }
}
