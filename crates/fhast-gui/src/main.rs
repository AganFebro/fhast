#![cfg_attr(windows, windows_subsystem = "windows")]

mod daemon;
mod ipc_client;
mod tray;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText};
use fhast_core::{config_path, load_config_file, paths, save_config_file, ConfigFile};
use fhast_ipc::{DownloadDto, EventDto, IpcRequest, IPC_VERSION};
use ipc_client::{AddDownloadCommand, AutoGrabStatus, DetailData, Snapshot};
use tokio::runtime::Runtime;

use crate::daemon::DaemonManager;
use crate::tray::{TrayCommand, TrayController};

const REFRESH_INTERVAL: Duration = Duration::from_millis(500);

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("fhast")
            .with_inner_size([1120.0, 760.0])
            .with_min_inner_size([860.0, 560.0]),
        ..Default::default()
    };

    eframe::run_native(
        "fhast",
        options,
        Box::new(|cc| {
            let app = FhastGuiApp::new(cc)
                .map_err(|error| -> Box<dyn std::error::Error + Send + Sync> { error.into() })?;
            Ok(Box::new(app))
        }),
    )
}

struct FhastGuiApp {
    runtime: Runtime,
    daemon: DaemonManager,
    socket_path: PathBuf,
    tx: Sender<WorkerMessage>,
    rx: Receiver<WorkerMessage>,
    downloads: Vec<DownloadDto>,
    events: Vec<EventDto>,
    detail: Option<DetailData>,
    selected_id: Option<String>,
    auto_grab: AutoGrabStatus,
    connected: bool,
    status_message: Option<String>,
    refresh_in_flight: bool,
    detail_in_flight: Option<String>,
    last_refresh: Instant,
    add_form: AddForm,
    show_add_dialog: bool,
    config: ConfigFile,
    config_editor: ConfigEditor,
    config_path: Option<PathBuf>,
    show_config: bool,
    show_events: bool,
    show_status: bool,
    pending_remove: Option<String>,
    tray: Option<TrayController>,
    tray_error: Option<String>,
    exit_requested: bool,
}

enum WorkerMessage {
    Snapshot(Result<Snapshot, String>),
    Detail {
        id: String,
        result: Result<Box<DetailData>, String>,
    },
    Action(Result<String, String>),
}

#[derive(Default)]
struct AddForm {
    url: String,
    output_path: String,
    connections: String,
    checksum_sha256: String,
    checksum_md5: String,
    error: Option<String>,
}

struct ConfigEditor {
    max_retries: String,
    max_connections_per_download: String,
    max_connections_global: String,
    max_connections_per_host: String,
    output_dir: String,
    sensitive_header_retention: String,
    error: Option<String>,
}

impl ConfigEditor {
    fn from_config(config: &ConfigFile) -> Self {
        Self {
            max_retries: config.max_retries.to_string(),
            max_connections_per_download: config.max_connections_per_download.to_string(),
            max_connections_global: config.max_connections_global.to_string(),
            max_connections_per_host: config.max_connections_per_host.to_string(),
            output_dir: config
                .output_dir
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            sensitive_header_retention: if config.sensitive_header_retention.is_empty() {
                "until_complete".to_owned()
            } else {
                config.sensitive_header_retention.clone()
            },
            error: None,
        }
    }

    fn to_config(&self) -> Result<ConfigFile, String> {
        let sensitive_header_retention = self.sensitive_header_retention.trim().to_owned();
        if !matches!(
            sensitive_header_retention.as_str(),
            "never" | "until_complete" | "encrypted"
        ) {
            return Err(
                "sensitive_header_retention must be never, until_complete, or encrypted".into(),
            );
        }

        Ok(ConfigFile {
            max_retries: parse_number(&self.max_retries, "max_retries")?,
            max_connections_per_download: parse_number(
                &self.max_connections_per_download,
                "max_connections_per_download",
            )?,
            max_connections_global: parse_number(
                &self.max_connections_global,
                "max_connections_global",
            )?,
            max_connections_per_host: parse_number(
                &self.max_connections_per_host,
                "max_connections_per_host",
            )?,
            output_dir: optional_path(&self.output_dir),
            sensitive_header_retention,
        })
    }
}

impl FhastGuiApp {
    fn new(cc: &eframe::CreationContext<'_>) -> anyhow::Result<Self> {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        cc.egui_ctx.set_pixels_per_point(1.05);

        let runtime = Runtime::new()?;
        let daemon_result = runtime.block_on(DaemonManager::ensure_running());
        let fallback_socket = paths::socket_path().unwrap_or_else(|_| PathBuf::from(""));
        let (daemon, socket_path, status_message, connected) = match daemon_result {
            Ok(manager) => {
                let started = manager.started_by_gui();
                let socket_path = manager.socket_path().clone();
                let message = if started {
                    Some("daemon started".to_owned())
                } else {
                    Some("connected to existing daemon".to_owned())
                };
                (manager, socket_path, message, true)
            }
            Err(error) => {
                let message = Some(format!("daemon unavailable: {error}"));
                (
                    DaemonManager::offline(fallback_socket.clone()),
                    fallback_socket,
                    message,
                    false,
                )
            }
        };

        let (tx, rx) = mpsc::channel();
        let config = load_config_file().unwrap_or_default();
        let config_path = config_path().ok();
        let tray_result = TrayController::new();
        let tray_error = tray_result
            .as_ref()
            .err()
            .map(|error| format!("system tray unavailable: {error}"));
        let tray = tray_result.ok();

        Ok(Self {
            runtime,
            daemon,
            socket_path,
            tx,
            rx,
            downloads: Vec::new(),
            events: Vec::new(),
            detail: None,
            selected_id: None,
            auto_grab: AutoGrabStatus::default(),
            connected,
            status_message,
            refresh_in_flight: false,
            detail_in_flight: None,
            last_refresh: Instant::now() - REFRESH_INTERVAL,
            add_form: AddForm::default(),
            show_add_dialog: false,
            config_editor: ConfigEditor::from_config(&config),
            config,
            config_path,
            show_config: false,
            show_events: false,
            show_status: false,
            pending_remove: None,
            tray,
            tray_error,
            exit_requested: false,
        })
    }

    fn receive_worker_messages(&mut self, ctx: &egui::Context) {
        while let Ok(message) = self.rx.try_recv() {
            match message {
                WorkerMessage::Snapshot(result) => {
                    self.refresh_in_flight = false;
                    match result {
                        Ok(snapshot) => {
                            self.connected = true;
                            self.downloads = snapshot.downloads;
                            self.events = snapshot.events;
                            self.auto_grab = snapshot.auto_grab;
                            self.ensure_selection();
                            if let Some(id) = self.selected_id.clone() {
                                self.request_detail(ctx, id);
                            }
                        }
                        Err(error) => {
                            self.connected = false;
                            self.status_message = Some(format!("daemon unreachable: {error}"));
                        }
                    }
                }
                WorkerMessage::Detail { id, result } => {
                    if self.detail_in_flight.as_deref() == Some(id.as_str()) {
                        self.detail_in_flight = None;
                    }
                    if self.selected_id.as_deref() == Some(id.as_str()) {
                        match result {
                            Ok(detail) => self.detail = Some(*detail),
                            Err(error) => {
                                self.status_message = Some(format!("details failed: {error}"))
                            }
                        }
                    }
                }
                WorkerMessage::Action(result) => {
                    match result {
                        Ok(message) => self.status_message = Some(message),
                        Err(error) => self.status_message = Some(format!("error: {error}")),
                    }
                    self.request_refresh(ctx);
                }
            }
        }
    }

    fn ensure_selection(&mut self) {
        if let Some(selected_id) = &self.selected_id {
            if self
                .downloads
                .iter()
                .any(|download| &download.id == selected_id)
            {
                return;
            }
        }

        self.selected_id = self.downloads.first().map(|download| download.id.clone());
        self.detail = None;
    }

    fn request_refresh(&mut self, ctx: &egui::Context) {
        if self.refresh_in_flight || self.socket_path.as_os_str().is_empty() {
            return;
        }

        self.refresh_in_flight = true;
        let socket_path = self.socket_path.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        self.runtime.spawn(async move {
            let result = ipc_client::load_snapshot(socket_path)
                .await
                .map_err(|error| error.to_string());
            let _ = tx.send(WorkerMessage::Snapshot(result));
            ctx.request_repaint();
        });
    }

    fn request_detail(&mut self, ctx: &egui::Context, id: String) {
        if self.detail_in_flight.as_deref() == Some(id.as_str())
            || self.socket_path.as_os_str().is_empty()
        {
            return;
        }

        self.detail_in_flight = Some(id.clone());
        let socket_path = self.socket_path.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        self.runtime.spawn(async move {
            let result = ipc_client::load_detail(socket_path, id.clone())
                .await
                .map(Box::new)
                .map_err(|error| error.to_string());
            let _ = tx.send(WorkerMessage::Detail { id, result });
            ctx.request_repaint();
        });
    }

    fn spawn_action<F>(&mut self, ctx: &egui::Context, future: F)
    where
        F: Future<Output = anyhow::Result<String>> + Send + 'static,
    {
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        self.runtime.spawn(async move {
            let result = future.await.map_err(|error| error.to_string());
            let _ = tx.send(WorkerMessage::Action(result));
            ctx.request_repaint();
        });
    }

    fn submit_add_form(&mut self, ctx: &egui::Context) {
        let Some(command) = self.add_form_command() else {
            return;
        };
        let socket_path = self.socket_path.clone();
        self.spawn_action(ctx, ipc_client::add_download(socket_path, command));
        self.show_add_dialog = false;
        self.add_form = AddForm::default();
    }

    fn add_form_command(&mut self) -> Option<AddDownloadCommand> {
        let url = self.add_form.url.trim().to_owned();
        if !looks_like_url(&url) {
            self.add_form.error = Some("enter an http or https URL".to_owned());
            return None;
        }

        let connections = match optional_number(&self.add_form.connections, "connections") {
            Ok(value) => value,
            Err(error) => {
                self.add_form.error = Some(error);
                return None;
            }
        };

        self.add_form.error = None;
        Some(AddDownloadCommand {
            url,
            output_path: optional_path(&self.add_form.output_path),
            working_dir: default_working_dir(&self.config),
            connections,
            checksum_sha256: optional_string(&self.add_form.checksum_sha256),
            checksum_md5: optional_string(&self.add_form.checksum_md5),
        })
    }

    fn enqueue_url(&mut self, ctx: &egui::Context, url: String) {
        let command = AddDownloadCommand {
            url,
            output_path: None,
            working_dir: default_working_dir(&self.config),
            connections: None,
            checksum_sha256: None,
            checksum_md5: None,
        };
        let socket_path = self.socket_path.clone();
        self.spawn_action(ctx, ipc_client::add_download(socket_path, command));
    }

    fn control_download(&mut self, ctx: &egui::Context, request: IpcRequest) {
        let socket_path = self.socket_path.clone();
        self.spawn_action(ctx, ipc_client::control_download(socket_path, request));
    }

    fn request_exit(&mut self, ctx: &egui::Context) {
        if self.daemon.started_by_gui() {
            match self.runtime.block_on(self.daemon.stop_if_started()) {
                Ok(Some(message)) => self.status_message = Some(message),
                Ok(None) => {}
                Err(error) => self.status_message = Some(format!("daemon stop failed: {error}")),
            }
        }
        self.exit_requested = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn hide_to_tray(&mut self, ctx: &egui::Context) {
        self.status_message = if self.tray.is_some() {
            Some("minimized to tray; downloads continue".to_owned())
        } else {
            Some("window minimized; downloads continue".to_owned())
        };

        if self.tray.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
    }

    fn show_from_tray(ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    fn handle_tray(&mut self, ctx: &egui::Context) {
        let command = self.tray.as_ref().and_then(TrayController::poll_command);
        match command {
            Some(TrayCommand::Show) => Self::show_from_tray(ctx),
            Some(TrayCommand::Exit) => self.request_exit(ctx),
            None => {}
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped_files = ctx.input(|input| input.raw.dropped_files.clone());
        if dropped_files.is_empty() {
            return;
        }

        let mut queued = 0;
        for file in dropped_files {
            if let Some(url) = url_from_drop(&file) {
                self.enqueue_url(ctx, url);
                queued += 1;
            }
        }

        if queued == 0 {
            self.status_message =
                Some("drop a URL, .url shortcut, or text file containing a URL".to_owned());
        }
    }

    fn render_toolbar(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.heading(RichText::new("fhast").strong());
            ui.separator();

            if ui.button("+ Add URL").clicked() {
                self.show_add_dialog = true;
            }
            if ui.button("Config").clicked() {
                self.config_editor = ConfigEditor::from_config(&self.config);
                self.show_config = true;
            }
            if ui.button("Events").clicked() {
                self.show_events = true;
            }
            if ui.button("Doctor").clicked() {
                self.show_status = true;
            }
            if ui.button("Exit").clicked() {
                self.request_exit(ctx);
            }

            ui.separator();
            let daemon_text = if self.connected {
                RichText::new("daemon connected").color(Color32::from_rgb(109, 220, 147))
            } else {
                RichText::new("daemon offline").color(Color32::from_rgb(236, 98, 98))
            };
            ui.label(daemon_text);

            let auto_text = if self.auto_grab.recent_native_capture {
                RichText::new("auto-grab recent").color(Color32::from_rgb(109, 220, 220))
            } else {
                RichText::new("auto-grab idle").color(Color32::GRAY)
            };
            ui.label(auto_text);
        });
    }

    fn render_downloads(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let downloads = self.downloads.clone();
        if downloads.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(96.0);
                ui.heading("No downloads yet");
                ui.label("Add a URL or drag a URL file into the window.");
            });
            return;
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            for download in downloads {
                self.render_download_row(ctx, ui, &download);
                ui.add_space(6.0);
            }
        });
    }

    fn render_download_row(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        download: &DownloadDto,
    ) {
        let selected = self.selected_id.as_deref() == Some(download.id.as_str());
        let fill = if selected {
            Color32::from_rgb(42, 49, 58)
        } else {
            Color32::from_rgb(28, 30, 34)
        };

        egui::Frame::group(ui.style()).fill(fill).show(ui, |ui| {
            ui.horizontal(|ui| {
                let status_color = status_color(&download.status);
                ui.label(
                    RichText::new(status_label(&download.status))
                        .color(status_color)
                        .strong(),
                );

                let file_name = file_name(&download.final_path);
                if ui
                    .selectable_label(selected, RichText::new(file_name).strong())
                    .clicked()
                {
                    self.selected_id = Some(download.id.clone());
                    self.detail = None;
                    self.request_detail(ctx, download.id.clone());
                }

                ui.add_space(8.0);
                ui.add(
                    egui::ProgressBar::new(progress_fraction(download))
                        .desired_width(210.0)
                        .text(progress_text(download)),
                );
                ui.label(bytes_text(download));

                if let Some(speed) = download.downloaded_bytes_per_second {
                    if speed > 0.0 {
                        ui.label(format_speed(speed));
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Remove").clicked() {
                        self.pending_remove = Some(download.id.clone());
                    }

                    let retry_enabled = matches!(
                        download.status.as_str(),
                        "failed" | "needs_refresh" | "cancelled"
                    );
                    if ui
                        .add_enabled(retry_enabled, egui::Button::new("Retry"))
                        .clicked()
                    {
                        self.control_download(
                            ctx,
                            IpcRequest::RetryDownload {
                                version: IPC_VERSION,
                                id: download.id.clone(),
                            },
                        );
                    }

                    let can_resume = download.status == "paused";
                    let can_pause = matches!(
                        download.status.as_str(),
                        "queued" | "probing" | "downloading" | "merging" | "verifying"
                    );
                    let label = if can_resume { "Resume" } else { "Pause" };
                    if ui
                        .add_enabled(can_resume || can_pause, egui::Button::new(label))
                        .clicked()
                    {
                        let request = if can_resume {
                            IpcRequest::ResumeDownload {
                                version: IPC_VERSION,
                                id: download.id.clone(),
                            }
                        } else {
                            IpcRequest::PauseDownload {
                                version: IPC_VERSION,
                                id: download.id.clone(),
                            }
                        };
                        self.control_download(ctx, request);
                    }
                });
            });

            if let Some(error) = &download.last_error {
                ui.label(RichText::new(error).color(Color32::from_rgb(236, 98, 98)));
            }
        });
    }

    fn render_details(&mut self, ui: &mut egui::Ui) {
        ui.heading("Details");
        ui.separator();

        let Some(selected_id) = self.selected_id.as_ref() else {
            ui.label("Select a download to inspect it.");
            return;
        };

        let Some(detail) = self
            .detail
            .as_ref()
            .filter(|detail| &detail.download.id == selected_id)
        else {
            ui.label("Loading details...");
            return;
        };

        let download = &detail.download;
        detail_line(ui, "URL", &download.url);
        detail_line(ui, "Save to", &download.final_path);
        detail_line(ui, "Status", &download.status);
        detail_line(ui, "Progress", &bytes_text(download));
        detail_line(
            ui,
            "Connections",
            &download.requested_connections.to_string(),
        );
        if let Some(speed) = download.downloaded_bytes_per_second {
            detail_line(ui, "Speed", &format_speed(speed));
        }
        if let Some(error) = &download.last_error {
            ui.label(
                RichText::new(format!("Error: {error}")).color(Color32::from_rgb(236, 98, 98)),
            );
        }

        ui.add_space(12.0);
        ui.heading("Segments");
        if detail.segments.is_empty() {
            ui.label("single connection");
        } else {
            for segment in &detail.segments {
                let total = segment.end_byte.saturating_sub(segment.start_byte) + 1;
                let fraction = if total > 0 {
                    segment.downloaded_bytes as f32 / total as f32
                } else {
                    0.0
                };
                ui.horizontal(|ui| {
                    ui.label(format!("#{}", segment.index));
                    ui.add(
                        egui::ProgressBar::new(fraction.clamp(0.0, 1.0))
                            .desired_width(180.0)
                            .text(format!(
                                "{} / {}",
                                format_bytes(segment.downloaded_bytes),
                                format_bytes(total)
                            )),
                    );
                    ui.label(&segment.status);
                });
            }
        }

        ui.add_space(12.0);
        ui.heading(format!("Headers ({})", detail.headers.len()));
        if detail.headers.is_empty() {
            ui.label("no headers stored");
        } else {
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .show(ui, |ui| {
                    for (name, value, sensitive) in &detail.headers {
                        let shown_value = if *sensitive {
                            "<redacted>".to_owned()
                        } else {
                            truncate(value, 96)
                        };
                        ui.horizontal_wrapped(|ui| {
                            ui.monospace(name);
                            ui.label(shown_value);
                        });
                    }
                });
        }
    }

    fn render_add_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_add_dialog {
            return;
        }

        let mut open = self.show_add_dialog;
        egui::Window::new("Add URL")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("URL");
                ui.text_edit_singleline(&mut self.add_form.url);
                ui.label("Output path (optional)");
                ui.text_edit_singleline(&mut self.add_form.output_path);
                ui.label("Connections (optional)");
                ui.text_edit_singleline(&mut self.add_form.connections);
                ui.label("SHA-256 (optional)");
                ui.text_edit_singleline(&mut self.add_form.checksum_sha256);
                ui.label("MD5 (optional)");
                ui.text_edit_singleline(&mut self.add_form.checksum_md5);
                ui.label(format!(
                    "Default folder: {}",
                    default_working_dir(&self.config).display()
                ));

                if let Some(error) = &self.add_form.error {
                    ui.label(RichText::new(error).color(Color32::from_rgb(236, 98, 98)));
                }

                ui.horizontal(|ui| {
                    if ui.button("Add").clicked() {
                        self.submit_add_form(ctx);
                    }
                    if ui.button("Cancel").clicked() {
                        self.show_add_dialog = false;
                        self.add_form.error = None;
                    }
                });
            });
        self.show_add_dialog = open && self.show_add_dialog;
    }

    fn render_config_window(&mut self, ctx: &egui::Context) {
        if !self.show_config {
            return;
        }

        let mut open = self.show_config;
        egui::Window::new("Config")
            .open(&mut open)
            .default_width(460.0)
            .show(ctx, |ui| {
                if let Some(path) = &self.config_path {
                    ui.label(format!("Config file: {}", path.display()));
                }

                config_field(ui, "Max retries", &mut self.config_editor.max_retries);
                config_field(
                    ui,
                    "Connections per download",
                    &mut self.config_editor.max_connections_per_download,
                );
                config_field(
                    ui,
                    "Global connections",
                    &mut self.config_editor.max_connections_global,
                );
                config_field(
                    ui,
                    "Connections per host",
                    &mut self.config_editor.max_connections_per_host,
                );
                config_field(ui, "Output dir", &mut self.config_editor.output_dir);

                egui::ComboBox::from_label("Sensitive header retention")
                    .selected_text(&self.config_editor.sensitive_header_retention)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.config_editor.sensitive_header_retention,
                            "never".to_owned(),
                            "never",
                        );
                        ui.selectable_value(
                            &mut self.config_editor.sensitive_header_retention,
                            "until_complete".to_owned(),
                            "until_complete",
                        );
                        ui.selectable_value(
                            &mut self.config_editor.sensitive_header_retention,
                            "encrypted".to_owned(),
                            "encrypted",
                        );
                    });

                if let Some(error) = &self.config_editor.error {
                    ui.label(RichText::new(error).color(Color32::from_rgb(236, 98, 98)));
                }

                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        match self.config_editor.to_config() {
                            Ok(config) => match save_config_file(&config) {
                                Ok(()) => {
                                    self.config = config;
                                    self.config_editor.error = None;
                                    self.status_message = Some("config saved".to_owned());
                                }
                                Err(error) => self.config_editor.error = Some(error.to_string()),
                            },
                            Err(error) => self.config_editor.error = Some(error),
                        }
                    }
                    if ui.button("Reload").clicked() {
                        match load_config_file() {
                            Ok(config) => {
                                self.config = config;
                                self.config_editor = ConfigEditor::from_config(&self.config);
                                self.status_message = Some("config reloaded".to_owned());
                            }
                            Err(error) => self.config_editor.error = Some(error.to_string()),
                        }
                    }
                });
            });
        self.show_config = open;
    }

    fn render_events_window(&mut self, ctx: &egui::Context) {
        if !self.show_events {
            return;
        }

        let mut open = self.show_events;
        egui::Window::new("Recent Events")
            .open(&mut open)
            .default_width(620.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for event in &self.events {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(event.level.to_uppercase())
                                    .color(event_color(&event.level)),
                            );
                            ui.monospace(&event.created_at);
                            if let Some(download_id) = &event.download_id {
                                ui.monospace(truncate(download_id, 8));
                            }
                            ui.label(&event.message);
                        });
                    }
                });
            });
        self.show_events = open;
    }

    fn render_status_window(&mut self, ctx: &egui::Context) {
        if !self.show_status {
            return;
        }

        let mut open = self.show_status;
        egui::Window::new("Doctor")
            .open(&mut open)
            .default_width(560.0)
            .show(ctx, |ui| {
                detail_line(
                    ui,
                    "Daemon",
                    if self.connected {
                        "connected"
                    } else {
                        "offline"
                    },
                );
                detail_line(
                    ui,
                    "Started by GUI",
                    if self.daemon.started_by_gui() {
                        "yes"
                    } else {
                        "no"
                    },
                );
                detail_line(ui, "Socket", &self.socket_path.display().to_string());
                if let Ok(path) = paths::database_path() {
                    detail_line(ui, "Database", &path.display().to_string());
                }
                if let Some(path) = &self.config_path {
                    detail_line(ui, "Config", &path.display().to_string());
                }
                detail_line(ui, "Downloads", &self.downloads.len().to_string());
                detail_line(ui, "Active", &active_count(&self.downloads).to_string());
                if let Some(capture_at) = &self.auto_grab.last_capture_at {
                    detail_line(ui, "Last auto-grab", capture_at);
                } else {
                    detail_line(ui, "Last auto-grab", "not seen recently");
                }
                if let Some(error) = &self.tray_error {
                    ui.label(RichText::new(error).color(Color32::from_rgb(236, 184, 98)));
                }
            });
        self.show_status = open;
    }

    fn render_remove_confirmation(&mut self, ctx: &egui::Context) {
        let Some(id) = self.pending_remove.clone() else {
            return;
        };

        egui::Window::new("Remove Download")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("Remove this download and clean up its files?");
                ui.monospace(&id);
                ui.horizontal(|ui| {
                    if ui.button("Remove").clicked() {
                        self.control_download(
                            ctx,
                            IpcRequest::RemoveDownload {
                                version: IPC_VERSION,
                                id: id.clone(),
                            },
                        );
                        self.pending_remove = None;
                    }
                    if ui.button("Cancel").clicked() {
                        self.pending_remove = None;
                    }
                });
            });
    }
}

impl eframe::App for FhastGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive_worker_messages(ctx);
        self.handle_tray(ctx);
        self.handle_dropped_files(ctx);

        if ctx.input(|input| input.viewport().close_requested()) && !self.exit_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide_to_tray(ctx);
        }

        if self.last_refresh.elapsed() >= REFRESH_INTERVAL {
            self.request_refresh(ctx);
            self.last_refresh = Instant::now();
        }

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            self.render_toolbar(ctx, ui);
        });

        egui::SidePanel::right("details")
            .resizable(true)
            .default_width(390.0)
            .show(ctx, |ui| self.render_details(ui));

        egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(if self.connected {
                    "Status: connected"
                } else {
                    "Status: offline"
                });
                ui.separator();
                ui.label(format!("{} active", active_count(&self.downloads)));
                ui.separator();
                ui.label(format!("{} total", self.downloads.len()));
                if let Some(message) = &self.status_message {
                    ui.separator();
                    ui.label(message);
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_downloads(ctx, ui);
        });

        self.render_add_dialog(ctx);
        self.render_config_window(ctx);
        self.render_events_window(ctx);
        self.render_status_window(ctx);
        self.render_remove_confirmation(ctx);

        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

fn parse_number<T>(value: &str, field: &str) -> Result<T, String>
where
    T: std::str::FromStr,
{
    value
        .trim()
        .parse::<T>()
        .map_err(|_| format!("{field} must be a number"))
}

fn optional_number(value: &str, field: &str) -> Result<Option<u16>, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    trimmed
        .parse::<u16>()
        .map(Some)
        .map_err(|_| format!("{field} must be a number between 1 and 65535"))
}

fn optional_path(value: &str) -> Option<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

fn optional_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn default_working_dir(config: &ConfigFile) -> PathBuf {
    if let Some(output_dir) = &config.output_dir {
        if !output_dir.as_os_str().is_empty() {
            return output_dir.clone();
        }
    }

    if let Some(downloads) = downloads_dir() {
        return downloads;
    }

    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn downloads_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .map(|home| home.join("Downloads"))
    }

    #[cfg(not(windows))]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Downloads"))
    }
}

fn looks_like_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

fn url_from_drop(file: &egui::DroppedFile) -> Option<String> {
    if let Some(bytes) = &file.bytes {
        if let Ok(text) = std::str::from_utf8(bytes.as_ref()) {
            if let Some(url) = first_url_in_text(text) {
                return Some(url);
            }
        }
    }

    let path = file.path.as_ref()?;
    let path_text = path.to_string_lossy();
    if looks_like_url(&path_text) {
        return Some(path_text.to_string());
    }

    if path.is_file() {
        if let Ok(text) = std::fs::read_to_string(path) {
            return first_url_in_text(&text);
        }
    }

    None
}

fn first_url_in_text(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim().trim_matches('"');
        let candidate = trimmed.strip_prefix("URL=").unwrap_or(trimmed);
        if looks_like_url(candidate) {
            return Some(candidate.to_owned());
        }
    }
    None
}

fn active_count(downloads: &[DownloadDto]) -> usize {
    downloads
        .iter()
        .filter(|download| {
            matches!(
                download.status.as_str(),
                "queued" | "probing" | "downloading" | "merging" | "verifying"
            )
        })
        .count()
}

fn progress_fraction(download: &DownloadDto) -> f32 {
    match download.total_size {
        Some(total) if total > 0 => {
            (download.downloaded_bytes as f32 / total as f32).clamp(0.0, 1.0)
        }
        Some(_) if download.status == "completed" => 1.0,
        _ => 0.0,
    }
}

fn progress_text(download: &DownloadDto) -> String {
    match download.total_size {
        Some(total) if total > 0 => format!("{:.1}%", progress_fraction(download) * 100.0),
        Some(_) => "100%".to_owned(),
        None => download.status.clone(),
    }
}

fn bytes_text(download: &DownloadDto) -> String {
    match download.total_size {
        Some(total) => format!(
            "{} / {}",
            format_bytes(download.downloaded_bytes),
            format_bytes(total)
        ),
        None => format_bytes(download.downloaded_bytes),
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = UNITS[0];

    for candidate in UNITS.iter().skip(1) {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = candidate;
    }

    format!("{value:.1} {unit}")
}

fn format_speed(bytes_per_second: f64) -> String {
    format!("{}/s", format_bytes(bytes_per_second.max(0.0) as u64))
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_owned())
}

fn status_label(status: &str) -> String {
    status.replace('_', " ").to_uppercase()
}

fn status_color(status: &str) -> Color32 {
    match status {
        "downloading" | "probing" | "merging" | "verifying" => Color32::from_rgb(236, 184, 98),
        "completed" => Color32::from_rgb(109, 220, 147),
        "failed" | "needs_refresh" => Color32::from_rgb(236, 98, 98),
        "paused" => Color32::from_rgb(109, 190, 236),
        _ => Color32::GRAY,
    }
}

fn event_color(level: &str) -> Color32 {
    match level {
        "error" => Color32::from_rgb(236, 98, 98),
        "warn" => Color32::from_rgb(236, 184, 98),
        _ => Color32::from_rgb(109, 220, 147),
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }

    let mut truncated = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

fn detail_line(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("{label}:")).strong());
        ui.label(value);
    });
}

fn config_field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.horizontal(|ui| {
        ui.add_sized([190.0, 20.0], egui::Label::new(label));
        ui.text_edit_singleline(value);
    });
}
