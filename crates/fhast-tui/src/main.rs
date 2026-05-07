use std::io;

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use fhast_core::paths;
use fhast_ipc::{
    connect_to_daemon, read_message, split_stream, write_message, DownloadDto, EventDto,
    IpcRequest, IpcResponse, SegmentDto, IPC_VERSION,
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
    Frame, Terminal,
};
use tokio::io::BufReader;

#[tokio::main]
async fn main() -> Result<()> {
    let mut app = App::new().await?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = app.run(&mut terminal).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

struct App {
    socket_path: std::path::PathBuf,
    downloads: Vec<DownloadDto>,
    events: Vec<EventDto>,
    selected_index: usize,
    view_mode: ViewMode,
    input_mode: InputMode,
    input_buffer: String,
    detail_segments: Vec<SegmentDto>,
    detail_headers: Vec<(String, String, bool)>,
    detail_download: Option<DownloadDto>,
    status_message: Option<String>,
    filter_text: String,
    running: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Downloads,
    Detail,
    Events,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Normal,
    AddUrl,
    Filter,
}

impl App {
    async fn new() -> Result<Self> {
        let socket_path = paths::socket_path().context("resolve daemon socket path")?;
        Ok(Self {
            socket_path,
            downloads: Vec::new(),
            events: Vec::new(),
            selected_index: 0,
            view_mode: ViewMode::Downloads,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            detail_segments: Vec::new(),
            detail_headers: Vec::new(),
            detail_download: None,
            status_message: None,
            filter_text: String::new(),
            running: true,
        })
    }

    async fn run(&mut self, terminal: &mut Terminal<impl ratatui::backend::Backend>) -> Result<()> {
        self.refresh_data().await?;
        let tick_rate = std::time::Duration::from_millis(500);
        let mut last_tick = tokio::time::Instant::now();

        while self.running {
            terminal.draw(|f| self.render(f))?;

            let timeout = tick_rate.saturating_sub(last_tick.elapsed());
            if event::poll(timeout)? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        self.handle_key(key.code).await?;
                    }
                    _ => {}
                }
            }

            if last_tick.elapsed() >= tick_rate {
                self.refresh_data().await?;
                last_tick = tokio::time::Instant::now();
            }
        }

        Ok(())
    }

    async fn refresh_data(&mut self) -> Result<()> {
        match self
            .send_request(IpcRequest::ListDownloads {
                version: IPC_VERSION,
            })
            .await
        {
            Ok(IpcResponse::DownloadList { downloads, .. }) => {
                self.downloads = downloads;
            }
            Ok(_) => {}
            Err(_) => {
                self.status_message = Some("daemon unreachable".to_owned());
                self.downloads.clear();
                return Ok(());
            }
        }

        if let Ok(IpcResponse::EventList { events, .. }) = self
            .send_request(IpcRequest::RecentEvents {
                version: IPC_VERSION,
                limit: 50,
            })
            .await
        {
            self.events = events;
        }

        self.status_message = None;
        Ok(())
    }

    async fn handle_key(&mut self, code: KeyCode) -> Result<()> {
        match self.input_mode {
            InputMode::AddUrl => return self.handle_add_url_key(code).await,
            InputMode::Filter => return self.handle_filter_key(code).await,
            InputMode::Normal => {}
        }

        match self.view_mode {
            ViewMode::Downloads => match code {
                KeyCode::Char('q') => self.running = false,
                KeyCode::Char('j') | KeyCode::Down => {
                    let count = self.filtered_download_count();
                    if count > 0 {
                        self.selected_index = (self.selected_index + 1).min(count - 1);
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.selected_index = self.selected_index.saturating_sub(1);
                }
                KeyCode::Enter => {
                    let filtered = self.filtered_downloads();
                    if let Some(d) = filtered.get(self.selected_index).copied() {
                        self.view_detail(d.id.clone()).await?;
                    }
                }
                KeyCode::Char(' ') => {
                    let (id, status) = {
                        let filtered = self.filtered_downloads();
                        match filtered.get(self.selected_index) {
                            Some(d) => (d.id.clone(), d.status.clone()),
                            None => return Ok(()),
                        }
                    };
                    self.toggle_pause(&id, &status).await?;
                }
                KeyCode::Char('r') => {
                    let id = {
                        let filtered = self.filtered_downloads();
                        match filtered.get(self.selected_index) {
                            Some(d) => d.id.clone(),
                            None => return Ok(()),
                        }
                    };
                    self.retry_download(&id).await?;
                }
                KeyCode::Char('d') => {
                    let id = {
                        let filtered = self.filtered_downloads();
                        match filtered.get(self.selected_index) {
                            Some(d) => d.id.clone(),
                            None => return Ok(()),
                        }
                    };
                    self.remove_download(&id).await?;
                }
                KeyCode::Char('a') => {
                    self.input_mode = InputMode::AddUrl;
                    self.input_buffer.clear();
                }
                KeyCode::Char('/') => {
                    self.input_mode = InputMode::Filter;
                    self.input_buffer.clear();
                }
                KeyCode::Tab => {
                    self.view_mode = ViewMode::Events;
                    self.filter_text.clear();
                }
                KeyCode::Esc => {
                    self.filter_text.clear();
                    self.selected_index = 0;
                }
                _ => {}
            },
            ViewMode::Detail => match code {
                KeyCode::Char('q') => self.running = false,
                KeyCode::Esc | KeyCode::Tab => {
                    self.view_mode = ViewMode::Downloads;
                    self.detail_segments.clear();
                    self.detail_download = None;
                }
                KeyCode::Char(' ') => {
                    let (id, status) = match &self.detail_download {
                        Some(d) => (d.id.clone(), d.status.clone()),
                        None => return Ok(()),
                    };
                    self.toggle_pause(&id, &status).await?;
                }
                KeyCode::Char('r') => {
                    let id = match &self.detail_download {
                        Some(d) => d.id.clone(),
                        None => return Ok(()),
                    };
                    self.retry_download(&id).await?;
                }
                KeyCode::Char('d') => {
                    let id = match &self.detail_download {
                        Some(d) => d.id.clone(),
                        None => return Ok(()),
                    };
                    self.remove_download(&id).await?;
                }
                _ => {}
            },
            ViewMode::Events => match code {
                KeyCode::Char('q') => self.running = false,
                KeyCode::Esc | KeyCode::Tab => {
                    self.view_mode = ViewMode::Downloads;
                }
                KeyCode::Char('j') | KeyCode::Down if !self.events.is_empty() => {
                    self.selected_index = (self.selected_index + 1).min(self.events.len() - 1);
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.selected_index = self.selected_index.saturating_sub(1);
                }
                _ => {}
            },
        }
        Ok(())
    }

    async fn view_detail(&mut self, id: String) -> Result<()> {
        if let Ok(IpcResponse::DownloadInfo { download, .. }) = self
            .send_request(IpcRequest::DownloadInfo {
                version: IPC_VERSION,
                id: id.clone(),
            })
            .await
        {
            self.detail_download = Some(download);
        }

        let id_clone = id.clone();
        if let Ok(IpcResponse::SegmentList { segments, .. }) = self
            .send_request(IpcRequest::DownloadSegments {
                version: IPC_VERSION,
                id,
            })
            .await
        {
            self.detail_segments = segments;
        }

        if let Ok(IpcResponse::HeaderList { headers, .. }) = self
            .send_request(IpcRequest::DownloadHeaders {
                version: IPC_VERSION,
                id: id_clone,
            })
            .await
        {
            self.detail_headers = headers;
        } else {
            self.detail_headers.clear();
        }

        self.view_mode = ViewMode::Detail;
        Ok(())
    }

    async fn toggle_pause(&mut self, id: &str, status: &str) -> Result<()> {
        let request = if status == "downloading" || status == "queued" {
            IpcRequest::PauseDownload {
                version: IPC_VERSION,
                id: id.to_owned(),
            }
        } else if status == "paused" {
            IpcRequest::ResumeDownload {
                version: IPC_VERSION,
                id: id.to_owned(),
            }
        } else {
            return Ok(());
        };

        let _ = self.send_request(request).await;
        self.status_message = Some(match status {
            "downloading" | "queued" => format!("paused {id}"),
            "paused" => format!("resumed {id}"),
            _ => return Ok(()),
        });
        self.refresh_data().await?;
        Ok(())
    }

    async fn retry_download(&mut self, id: &str) -> Result<()> {
        let _ = self
            .send_request(IpcRequest::RetryDownload {
                version: IPC_VERSION,
                id: id.to_owned(),
            })
            .await;
        self.status_message = Some(format!("retried {id}"));
        self.refresh_data().await?;
        Ok(())
    }

    async fn remove_download(&mut self, id: &str) -> Result<()> {
        let _ = self
            .send_request(IpcRequest::RemoveDownload {
                version: IPC_VERSION,
                id: id.to_owned(),
            })
            .await;
        self.status_message = Some(format!("removed {id}"));
        if self.selected_index >= self.downloads.len().saturating_sub(1) {
            self.selected_index = self.downloads.len().saturating_sub(1);
        }
        self.refresh_data().await?;
        Ok(())
    }

    async fn handle_add_url_key(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Enter => {
                let url = self.input_buffer.trim().to_owned();
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
                if !url.is_empty() {
                    let working_dir =
                        std::env::current_dir().context("resolve current directory")?;
                    let request = IpcRequest::AddDownload {
                        version: IPC_VERSION,
                        url,
                        output_path: None,
                        working_dir,
                        connections: None,
                        checksum_sha256: None,
                        checksum_md5: None,
                        headers: None,
                        sensitive_headers: None,
                    };
                    match self.send_request(request).await {
                        Ok(IpcResponse::DownloadAdded { download, .. }) => {
                            self.status_message =
                                Some(format!("queued {} -> {}", download.id, download.final_path));
                        }
                        Ok(IpcResponse::Error { message, .. }) => {
                            self.status_message = Some(format!("error: {message}"));
                        }
                        Err(e) => {
                            self.status_message = Some(format!("error: {e}"));
                        }
                        _ => {
                            self.status_message = Some("unexpected response".to_owned());
                        }
                    }
                    self.refresh_data().await?;
                }
            }
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
            }
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_filter_key(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Enter => {
                self.filter_text = self.input_buffer.clone();
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
                self.selected_index = 0;
            }
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
                self.filter_text.clear();
                self.selected_index = 0;
            }
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            _ => {}
        }
        Ok(())
    }

    fn filtered_downloads(&self) -> Vec<&DownloadDto> {
        if self.filter_text.is_empty() {
            self.downloads.iter().collect()
        } else {
            let lower = self.filter_text.to_lowercase();
            self.downloads
                .iter()
                .filter(|d| {
                    d.final_path.to_lowercase().contains(&lower)
                        || d.url.to_lowercase().contains(&lower)
                        || d.status.to_lowercase().contains(&lower)
                        || d.id.to_lowercase().contains(&lower)
                })
                .collect()
        }
    }

    fn filtered_download_count(&self) -> usize {
        self.filtered_downloads().len()
    }

    async fn send_request(&self, request: IpcRequest) -> Result<IpcResponse> {
        let stream = connect_to_daemon(&self.socket_path).await?;
        let (reader, mut writer) = split_stream(stream);
        let mut reader = BufReader::new(reader);

        write_message(&mut writer, &request).await?;
        let response = read_message(&mut reader).await?;
        Ok(response)
    }

    fn render(&mut self, f: &mut Frame) {
        let bottom_constraints: Vec<Constraint> = if self.input_mode == InputMode::Normal {
            vec![Constraint::Min(1), Constraint::Length(2)]
        } else {
            vec![
                Constraint::Min(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ]
        };

        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(bottom_constraints)
            .split(f.area());

        match self.view_mode {
            ViewMode::Downloads => self.render_downloads(f, main_chunks[0]),
            ViewMode::Detail => self.render_detail(f, main_chunks[0]),
            ViewMode::Events => self.render_events(f, main_chunks[0]),
        }

        if self.input_mode == InputMode::Normal {
            self.render_status_bar(f, main_chunks[1]);
        } else {
            self.render_input_bar(f, main_chunks[1]);
            self.render_status_bar(f, main_chunks[2]);
        }
    }

    fn render_downloads(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(50),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(area);

        let active: Vec<_> = self
            .downloads
            .iter()
            .filter(|d| {
                d.status == "downloading"
                    || d.status == "probing"
                    || d.status == "merging"
                    || d.status == "verifying"
            })
            .collect();
        let queued: Vec<_> = self
            .downloads
            .iter()
            .filter(|d| d.status == "queued")
            .collect();
        let completed: Vec<_> = self
            .downloads
            .iter()
            .filter(|d| d.status == "completed")
            .collect();
        let failed: Vec<_> = self
            .downloads
            .iter()
            .filter(|d| d.status == "failed" || d.status == "needs_refresh")
            .collect();

        let filtered = self.filtered_downloads();
        let title = if self.filter_text.is_empty() {
            "Downloads".to_owned()
        } else {
            format!("Downloads (filter: \"{}\")", self.filter_text)
        };

        let items: Vec<ListItem> = filtered
            .iter()
            .enumerate()
            .map(|(i, d)| self.download_list_item(d, i))
            .collect();
        let list = List::new(items)
            .block(Block::default().title(title).borders(Borders::ALL))
            .highlight_style(Style::default().bg(Color::DarkGray));
        f.render_stateful_widget(
            list,
            chunks[0],
            &mut ratatui::widgets::ListState::default().with_selected(Some(self.selected_index)),
        );

        self.render_section(
            chunks[1],
            "Status",
            &format!(
                "active: {} | queued: {} | completed: {} | failed: {} | total {}",
                active.len(),
                queued.len(),
                completed.len(),
                failed.len(),
                self.downloads.len(),
            ),
            f,
        );
        self.render_help(chunks[2], f);
    }

    fn download_list_item(&self, d: &DownloadDto, index: usize) -> ListItem<'_> {
        let status_color = match d.status.as_str() {
            "downloading" | "probing" | "merging" | "verifying" => Color::Yellow,
            "completed" => Color::Green,
            "failed" | "needs_refresh" => Color::Red,
            "paused" => Color::Cyan,
            _ => Color::Gray,
        };

        let progress = if let Some(total) = d.total_size {
            if total > 0 {
                let pct = (d.downloaded_bytes as f64 / total as f64) * 100.0;
                let bar_width = 20;
                let filled = ((pct / 100.0) * bar_width as f64) as usize;
                let bar_str = format!(
                    "{}{}",
                    "\u{2588}".repeat(filled),
                    "\u{2591}".repeat(bar_width - filled)
                );
                format!(" [{:>3.0}% {bar_str}]", pct)
            } else {
                " [?]".to_owned()
            }
        } else {
            " [?]".to_owned()
        };

        let bytes_str = if let Some(total) = d.total_size {
            format!(
                "{} / {}",
                format_bytes(d.downloaded_bytes),
                format_bytes(total)
            )
        } else {
            format_bytes(d.downloaded_bytes)
        };

        let speed = if let Some(bps) = d.downloaded_bytes_per_second {
            if bps > 0.0 {
                format!(" [{}]", format_speed(bps))
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let file_name = std::path::Path::new(&d.final_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| d.final_path.clone());

        let mut spans = vec![
            Span::styled(
                format!(" {:3} ", index),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("{:12} ", d.status.to_uppercase()),
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("{} ", file_name)),
        ];

        if d.status == "downloading" {
            spans.push(Span::raw(format!("{}{} {}", bytes_str, speed, progress)));
        } else {
            spans.push(Span::raw(bytes_str));
        }

        if d.status == "completed" || d.status == "failed" {
            spans.push(Span::styled(
                format!(" → {}", d.final_path),
                Style::default().fg(Color::DarkGray),
            ));
        }

        if let Some(ref err) = d.last_error {
            spans.push(Span::styled(
                format!(" ({err})"),
                Style::default().fg(Color::Red),
            ));
        }

        ListItem::new(Line::from(spans))
    }

    fn render_detail(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(35),
                Constraint::Percentage(30),
                Constraint::Percentage(25),
                Constraint::Percentage(10),
            ])
            .split(area);

        if let Some(ref d) = self.detail_download {
            let file_name = std::path::Path::new(&d.final_path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| d.final_path.clone());

            let mut lines = vec![
                Line::from(Span::styled(
                    format!("ID: {}", d.id),
                    Style::default().bold(),
                )),
                Line::from(format!("URL: {}", d.url)),
                Line::from(format!("File: {file_name}")),
                Line::from(Span::styled(
                    format!("Path: {}", d.final_path),
                    Style::default().fg(Color::Gray),
                )),
                Line::from(format!("Status: {}", d.status.to_uppercase())),
                Line::from(format!(
                    "Progress: {} / {}",
                    format_bytes(d.downloaded_bytes),
                    d.total_size.map_or("?".to_owned(), format_bytes)
                )),
                Line::from(format!("Connections: {}", d.requested_connections)),
            ];
            if let Some(ref hash) = d.checksum_sha256 {
                lines.push(Line::from(format!("SHA-256: {hash}")));
            }
            if let Some(ref err) = d.last_error {
                lines.push(Line::from(Span::styled(
                    format!("Error: {err}"),
                    Style::default().fg(Color::Red),
                )));
            }

            let info = Paragraph::new(lines)
                .block(Block::default().title("Details").borders(Borders::ALL));
            f.render_widget(info, chunks[0]);

            let segment_block = Block::default().title("Segments").borders(Borders::ALL);
            if self.detail_segments.is_empty() {
                let msg = Paragraph::new("single connection (no segments)").block(segment_block);
                f.render_widget(msg, chunks[1]);
            } else {
                let items: Vec<ListItem> = self
                    .detail_segments
                    .iter()
                    .map(|seg| {
                        let seg_size = seg.end_byte.saturating_sub(seg.start_byte) + 1;
                        let seg_progress = if seg_size > 0 {
                            ((seg.downloaded_bytes as f64 / seg_size as f64) * 100.0) as u16
                        } else {
                            0
                        };
                        let _bar = Gauge::default()
                            .gauge_style(Style::default().fg(match seg.status.as_str() {
                                "completed" => Color::Green,
                                "downloading" => Color::Yellow,
                                "failed" => Color::Red,
                                _ => Color::Gray,
                            }))
                            .percent(seg_progress);
                        let label = format!(
                            "[{}] {}/{}",
                            seg.index,
                            format_bytes(seg.downloaded_bytes),
                            format_bytes(seg.end_byte.saturating_sub(seg.start_byte) + 1)
                        );
                        ListItem::new(Line::from(vec![
                            Span::raw(format!("seg{}: ", seg.index)),
                            Span::raw(label),
                        ]))
                    })
                    .collect();

                let list = List::new(items).block(segment_block);
                f.render_widget(list, chunks[1]);
            }

            let header_block = Block::default()
                .title(format!("Headers ({})", self.detail_headers.len()))
                .borders(Borders::ALL);
            if self.detail_headers.is_empty() {
                let empty = Paragraph::new("no headers stored")
                    .block(header_block)
                    .style(Style::default().fg(Color::DarkGray));
                f.render_widget(empty, chunks[2]);
            } else {
                let header_items: Vec<ListItem> = self
                    .detail_headers
                    .iter()
                    .map(|(name, value, sensitive)| {
                        let (display_name, style) =
                            if let Some(rh) = name.strip_prefix("x-fhast-rh:") {
                                (rh.to_string(), Style::default().fg(Color::Yellow))
                            } else if *sensitive {
                                (name.clone(), Style::default().fg(Color::Red))
                            } else {
                                (name.clone(), Style::default().fg(Color::Cyan))
                            };
                        ListItem::new(Line::from(vec![
                            Span::styled(display_name, style),
                            Span::raw(": "),
                            Span::raw(if *sensitive {
                                redact_value(value)
                            } else {
                                truncate_value(value, 60)
                            }),
                        ]))
                    })
                    .collect();
                let list = List::new(header_items).block(header_block);
                f.render_widget(list, chunks[2]);
            }

            self.render_help(chunks[3], f);
        } else {
            let msg = Paragraph::new("No download selected")
                .block(Block::default().title("Details").borders(Borders::ALL));
            f.render_widget(msg, chunks[0]);
        }
    }

    fn render_events(&self, f: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .events
            .iter()
            .map(|e| {
                let level_color = match e.level.as_str() {
                    "error" => Color::Red,
                    "warn" => Color::Yellow,
                    _ => Color::Gray,
                };
                let dl = e.download_id.as_deref().unwrap_or("-");
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("[{:5}] ", e.level.to_uppercase()),
                        Style::default()
                            .fg(level_color)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(format!("{dl} ")),
                    Span::raw(&e.message),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title("Recent Events")
                    .borders(Borders::ALL),
            )
            .highlight_style(Style::default().bg(Color::DarkGray));
        f.render_stateful_widget(
            list,
            area,
            &mut ratatui::widgets::ListState::default().with_selected(Some(self.selected_index)),
        );
    }

    fn render_section(&self, area: Rect, title: &str, content: &str, f: &mut Frame) {
        let p = Paragraph::new(content).block(Block::default().title(title).borders(Borders::ALL));
        f.render_widget(p, area);
    }

    fn render_help(&self, area: Rect, f: &mut Frame) {
        let help = match self.view_mode {
            ViewMode::Downloads => "j/k: move | space: pause/resume | r: retry | d: remove | a: add | Tab: events | /: filter | q: quit",
            ViewMode::Detail => "Esc/Tab: back | space: pause/resume | r: retry | d: remove | q: quit",
            ViewMode::Events => "j/k: scroll | Esc/Tab: back | q: quit",
        };
        let p = Paragraph::new(help).block(Block::default().title("Keys").borders(Borders::ALL));
        f.render_widget(p, area);
    }

    fn render_status_bar(&self, f: &mut Frame, area: Rect) {
        let text = match &self.status_message {
            Some(msg) => Span::styled(msg, Style::default().fg(Color::Yellow)),
            None => Span::styled(
                format!(
                    "fhast | {} downloads | daemon: {}",
                    self.downloads.len(),
                    if self.downloads.is_empty() && self.status_message.is_none() {
                        "connecting..."
                    } else {
                        "connected"
                    },
                ),
                Style::default().fg(Color::DarkGray),
            ),
        };
        let bar = Paragraph::new(Line::from(text)).style(Style::default().bg(Color::Black));
        f.render_widget(bar, area);
    }

    fn render_input_bar(&self, f: &mut Frame, area: Rect) {
        let (prefix, buffer) = match self.input_mode {
            InputMode::AddUrl => ("Add URL: ", &self.input_buffer),
            InputMode::Filter => ("Filter: ", &self.input_buffer),
            InputMode::Normal => return,
        };
        let cursor = "█";
        let text = format!("{prefix}{buffer}{cursor}");
        let p = Paragraph::new(text).style(Style::default().bg(Color::DarkGray).fg(Color::White));
        f.render_widget(p, area);
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit_index = 0;
    while value >= 1024.0 && unit_index < UNITS.len() - 1 {
        value /= 1024.0;
        unit_index += 1;
    }
    if unit_index == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit_index])
    }
}

fn format_speed(bytes_per_sec: f64) -> String {
    format!("{}/s", format_bytes(bytes_per_sec as u64))
}

fn redact_value(value: &str) -> String {
    if value.len() <= 20 {
        "<redacted>".to_string()
    } else {
        format!("<redacted len={}>", value.len())
    }
}

fn truncate_value(value: &str, max: usize) -> String {
    if value.len() <= max {
        value.to_string()
    } else {
        format!("{}...", &value[..max])
    }
}
