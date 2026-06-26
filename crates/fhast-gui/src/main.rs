#![cfg_attr(windows, windows_subsystem = "windows")]

mod daemon;
mod ipc_client;
mod tray;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use anyhow::Context;
use eframe::egui::{self, Color32, RichText};
use fhast_core::{config_path, load_config_file, paths, save_config_file, ConfigFile};
use fhast_ipc::{DownloadDto, EventDto, IpcRequest, IPC_VERSION};
use ipc_client::{AddDownloadCommand, AutoGrabStatus, DetailData, Snapshot};
use tokio::runtime::Runtime;

use crate::daemon::DaemonManager;
use crate::tray::{TrayCommand, TrayController};

const REFRESH_INTERVAL: Duration = Duration::from_millis(500);
const ROW_HEIGHT: f32 = 24.0;
const RESIZE_HANDLE_WIDTH: f32 = 8.0;
const MIN_STATUS: f32 = 90.0;
const MIN_FILE: f32 = 150.0;
const MIN_PROGRESS: f32 = 140.0;
const MIN_SIZE: f32 = 130.0;
const MIN_SPEED: f32 = 70.0;
const MIN_CONNECTIONS: f32 = 55.0;
const MIN_ACTIONS: f32 = 215.0;
const DEFAULT_STATUS: f32 = 110.0;
const DEFAULT_FILE: f32 = 240.0;
const DEFAULT_PROGRESS: f32 = 190.0;
const DEFAULT_SIZE: f32 = 160.0;
const DEFAULT_SPEED: f32 = 95.0;
const DEFAULT_CONNECTIONS: f32 = 80.0;
const DEFAULT_ACTIONS: f32 = 235.0;

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
    download_columns: DownloadTableColumns,
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
    extension_editor: ExtensionEditor,
    native_host_status: NativeHostStatus,
    native_host_status_checked: bool,
    native_host_status_in_flight: bool,
    show_config: bool,
    show_events: bool,
    show_status: bool,
    show_extension_setup: bool,
    show_details: bool,
    show_close_prompt: bool,
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
    NativeHostStatus(Result<NativeHostStatus, String>),
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

#[derive(Clone, Copy)]
struct DownloadTableColumns {
    status: f32,
    file: f32,
    progress: f32,
    size: f32,
    speed: f32,
    connections: f32,
    actions: f32,
}

impl Default for DownloadTableColumns {
    fn default() -> Self {
        Self {
            status: DEFAULT_STATUS,
            file: DEFAULT_FILE,
            progress: DEFAULT_PROGRESS,
            size: DEFAULT_SIZE,
            speed: DEFAULT_SPEED,
            connections: DEFAULT_CONNECTIONS,
            actions: DEFAULT_ACTIONS,
        }
    }
}

impl DownloadTableColumns {
    fn total(self) -> f32 {
        self.status
            + self.file
            + self.progress
            + self.size
            + self.speed
            + self.connections
            + self.actions
    }

    fn total_with_handles(self) -> f32 {
        self.total() + RESIZE_HANDLE_WIDTH * 6.0
    }

    fn min_total() -> f32 {
        MIN_STATUS + MIN_FILE + MIN_PROGRESS + MIN_SIZE + MIN_SPEED + MIN_CONNECTIONS + MIN_ACTIONS
    }

    fn fit_to_available(&mut self, available_width: f32) {
        self.clamp_minimums();
        let target = (available_width - RESIZE_HANDLE_WIDTH * 6.0).max(Self::min_total());
        let delta = target - self.total();

        if delta.abs() < 1.0 {
            return;
        }

        if delta > 0.0 {
            self.file += delta * 0.45;
            self.progress += delta * 0.25;
            self.size += delta * 0.15;
            self.actions += delta * 0.15;
        } else {
            self.shrink_by(-delta);
        }
    }

    fn clamp_minimums(&mut self) {
        self.status = self.status.max(MIN_STATUS);
        self.file = self.file.max(MIN_FILE);
        self.progress = self.progress.max(MIN_PROGRESS);
        self.size = self.size.max(MIN_SIZE);
        self.speed = self.speed.max(MIN_SPEED);
        self.connections = self.connections.max(MIN_CONNECTIONS);
        self.actions = self.actions.max(MIN_ACTIONS);
    }

    fn shrink_by(&mut self, amount: f32) {
        let status_capacity = self.status - MIN_STATUS;
        let file_capacity = self.file - MIN_FILE;
        let progress_capacity = self.progress - MIN_PROGRESS;
        let size_capacity = self.size - MIN_SIZE;
        let speed_capacity = self.speed - MIN_SPEED;
        let connections_capacity = self.connections - MIN_CONNECTIONS;
        let actions_capacity = self.actions - MIN_ACTIONS;
        let total_capacity = status_capacity
            + file_capacity
            + progress_capacity
            + size_capacity
            + speed_capacity
            + connections_capacity
            + actions_capacity;

        if total_capacity <= 0.0 {
            return;
        }

        let ratio = amount.min(total_capacity) / total_capacity;
        self.status -= status_capacity * ratio;
        self.file -= file_capacity * ratio;
        self.progress -= progress_capacity * ratio;
        self.size -= size_capacity * ratio;
        self.speed -= speed_capacity * ratio;
        self.connections -= connections_capacity * ratio;
        self.actions -= actions_capacity * ratio;
    }
}

struct ConfigEditor {
    max_retries: String,
    max_connections_per_download: String,
    max_connections_global: String,
    max_connections_per_host: String,
    output_dir: String,
    sensitive_header_retention: String,
    chrome_extension_id: String,
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
            chrome_extension_id: config.chrome_extension_id.clone(),
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
            chrome_extension_id: normalize_extension_id(&self.chrome_extension_id)?,
        })
    }
}

struct ExtensionEditor {
    extension_id: String,
    error: Option<String>,
}

#[derive(Clone)]
struct NativeHostStatus {
    manifest_path: Option<PathBuf>,
    registry_manifest_path: Option<PathBuf>,
    installed: bool,
    error: Option<String>,
}

impl NativeHostStatus {
    fn unknown() -> Self {
        Self {
            manifest_path: native_host_manifest_path(),
            registry_manifest_path: None,
            installed: false,
            error: None,
        }
    }
}

impl ExtensionEditor {
    fn from_config(config: &ConfigFile) -> Self {
        Self {
            extension_id: config.chrome_extension_id.clone(),
            error: None,
        }
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
        let tray_result = TrayController::new(&cc.egui_ctx);
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
            download_columns: DownloadTableColumns::default(),
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
            extension_editor: ExtensionEditor::from_config(&config),
            native_host_status: NativeHostStatus::unknown(),
            native_host_status_checked: false,
            native_host_status_in_flight: false,
            config,
            config_path,
            show_config: false,
            show_events: false,
            show_status: false,
            show_extension_setup: false,
            show_details: false,
            show_close_prompt: false,
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
                            if self.show_details {
                                if let Some(id) = self.selected_id.clone() {
                                    self.request_detail(ctx, id);
                                }
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
                    self.request_native_host_status(ctx);
                }
                WorkerMessage::NativeHostStatus(result) => {
                    self.native_host_status_in_flight = false;
                    self.native_host_status_checked = true;
                    match result {
                        Ok(status) => self.native_host_status = status,
                        Err(error) => {
                            self.native_host_status.error = Some(error);
                            self.native_host_status.installed = false;
                        }
                    }
                }
            }
        }
    }

    fn reset_native_host_status(&mut self) {
        self.native_host_status = NativeHostStatus::unknown();
        self.native_host_status_checked = false;
    }

    fn has_saved_extension_id(&self) -> bool {
        !self.config.chrome_extension_id.trim().is_empty()
    }

    fn needs_native_host_warning(&self) -> bool {
        self.has_saved_extension_id()
            && self.native_host_status_checked
            && !self.native_host_status.installed
    }

    fn request_native_host_status(&mut self, ctx: &egui::Context) {
        if self.native_host_status_in_flight {
            return;
        }

        self.native_host_status_in_flight = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        self.runtime.spawn(async move {
            let result = load_native_host_status()
                .await
                .map_err(|error| error.to_string());
            let _ = tx.send(WorkerMessage::NativeHostStatus(result));
            ctx.request_repaint();
        });
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

    fn select_download(&mut self, ctx: &egui::Context, id: String) {
        if self.selected_id.as_deref() != Some(id.as_str()) {
            self.selected_id = Some(id.clone());
            self.detail = None;
        }
        self.request_detail(ctx, id);
    }

    fn open_download_details(&mut self, ctx: &egui::Context, id: String) {
        self.show_details = true;
        self.select_download(ctx, id);
    }

    fn request_exit(&mut self, ctx: &egui::Context) {
        match self.runtime.block_on(self.daemon.stop()) {
            Ok(Some(message)) => self.status_message = Some(message),
            Ok(None) => {}
            Err(error) => self.status_message = Some(format!("daemon stop failed: {error}")),
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

        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }

    fn show_from_tray(ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    fn prompt_for_close(&mut self, ctx: &egui::Context) {
        self.show_close_prompt = true;
        Self::show_from_tray(ctx);
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
                self.request_native_host_status(ctx);
                self.show_status = true;
            }
            if ui.button("Extension").clicked() {
                self.extension_editor = ExtensionEditor::from_config(&self.config);
                self.request_native_host_status(ctx);
                self.show_extension_setup = true;
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

            if self.config.chrome_extension_id.trim().is_empty() {
                ui.label(
                    RichText::new("extension ID missing").color(Color32::from_rgb(236, 184, 98)),
                );
            } else if self.needs_native_host_warning() {
                ui.label(
                    RichText::new("Chrome integration (Native Host) not registered")
                        .color(Color32::from_rgb(236, 184, 98)),
                );
            }
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

        self.download_columns.fit_to_available(ui.available_width());
        let table_width = self.download_columns.total_with_handles();

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(table_width);
                self.render_download_table_header(ui);
                let columns = self.download_columns;
                for download in downloads {
                    self.render_download_row(ctx, ui, &download, columns);
                    ui.add_space(4.0);
                }
            });
    }

    fn render_download_table_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            table_header_cell(ui, "Status", self.download_columns.status);
            resize_column_handle(
                ui,
                &mut self.download_columns.status,
                &mut self.download_columns.file,
                MIN_STATUS,
                MIN_FILE,
            );
            table_header_cell(ui, "File", self.download_columns.file);
            resize_column_handle(
                ui,
                &mut self.download_columns.file,
                &mut self.download_columns.progress,
                MIN_FILE,
                MIN_PROGRESS,
            );
            table_header_cell(ui, "Progress", self.download_columns.progress);
            resize_column_handle(
                ui,
                &mut self.download_columns.progress,
                &mut self.download_columns.size,
                MIN_PROGRESS,
                MIN_SIZE,
            );
            table_header_cell(ui, "Size", self.download_columns.size);
            resize_column_handle(
                ui,
                &mut self.download_columns.size,
                &mut self.download_columns.speed,
                MIN_SIZE,
                MIN_SPEED,
            );
            table_header_cell(ui, "Speed", self.download_columns.speed);
            resize_column_handle(
                ui,
                &mut self.download_columns.speed,
                &mut self.download_columns.connections,
                MIN_SPEED,
                MIN_CONNECTIONS,
            );
            table_header_cell(ui, "Conn", self.download_columns.connections);
            resize_column_handle(
                ui,
                &mut self.download_columns.connections,
                &mut self.download_columns.actions,
                MIN_CONNECTIONS,
                MIN_ACTIONS,
            );
            table_header_cell(ui, "Actions", self.download_columns.actions);
        });
        ui.separator();
    }

    fn render_download_row(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        download: &DownloadDto,
        columns: DownloadTableColumns,
    ) {
        let selected = self.selected_id.as_deref() == Some(download.id.as_str());
        let fill = if selected {
            Color32::from_rgb(42, 49, 58)
        } else {
            Color32::from_rgb(28, 30, 34)
        };

        egui::Frame::group(ui.style()).fill(fill).show(ui, |ui| {
            ui.set_min_width(columns.total_with_handles());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let status_color = status_color(&download.status);
                table_rich_cell(
                    ui,
                    RichText::new(status_label(&download.status))
                        .color(status_color)
                        .strong(),
                    columns.status,
                );
                table_column_gap(ui);

                let file_name = file_name(&download.final_path);
                let file_response = ui.add_sized(
                    [columns.file, ROW_HEIGHT],
                    egui::Label::new(RichText::new(file_name).strong())
                        .sense(egui::Sense::click())
                        .truncate(),
                );
                if file_response.clicked() {
                    self.select_download(ctx, download.id.clone());
                }
                if file_response.double_clicked() {
                    self.open_download_details(ctx, download.id.clone());
                }
                table_column_gap(ui);

                ui.allocate_ui_with_layout(
                    egui::vec2(columns.progress, ROW_HEIGHT),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add(
                            egui::ProgressBar::new(progress_fraction(download))
                                .desired_width(columns.progress - 6.0)
                                .text(progress_text(download)),
                        );
                    },
                );
                table_column_gap(ui);
                table_text_cell(ui, bytes_text(download), columns.size);
                table_column_gap(ui);

                let speed = download
                    .downloaded_bytes_per_second
                    .filter(|speed| *speed > 0.0)
                    .map(format_speed)
                    .unwrap_or_else(|| "-".to_owned());
                table_text_cell(ui, speed, columns.speed);
                table_column_gap(ui);
                table_text_cell(
                    ui,
                    download.requested_connections.to_string(),
                    columns.connections,
                );
                table_column_gap(ui);

                ui.allocate_ui_with_layout(
                    egui::vec2(columns.actions, ROW_HEIGHT),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        if ui
                            .add_sized([74.0, ROW_HEIGHT], egui::Button::new("Remove"))
                            .clicked()
                        {
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
                    },
                );
            });

            if let Some(error) = &download.last_error {
                table_rich_cell(
                    ui,
                    RichText::new(error).color(Color32::from_rgb(236, 98, 98)),
                    columns.total_with_handles(),
                );
            }
        });
    }

    fn render_details(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Details");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    self.show_details = false;
                }
            });
        });
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

        ui.add_space(8.0);
        egui::CollapsingHeader::new(format!("Segments ({})", detail.segments.len()))
            .default_open(false)
            .show(ui, |ui| {
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
            });

        egui::CollapsingHeader::new(format!("Headers ({})", detail.headers.len()))
            .default_open(false)
            .show(ui, |ui| {
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
            });
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
                config_field(
                    ui,
                    "Chrome extension ID",
                    &mut self.config_editor.chrome_extension_id,
                );

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
                                    self.extension_editor =
                                        ExtensionEditor::from_config(&self.config);
                                    self.config_editor.error = None;
                                    self.reset_native_host_status();
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
                                self.extension_editor = ExtensionEditor::from_config(&self.config);
                                self.reset_native_host_status();
                                self.status_message = Some("config reloaded".to_owned());
                            }
                            Err(error) => self.config_editor.error = Some(error.to_string()),
                        }
                    }
                });
            });
        self.show_config = open;
    }

    fn save_extension_settings(&mut self) -> bool {
        match normalize_extension_id(&self.extension_editor.extension_id) {
            Ok(extension_id) => {
                self.config.chrome_extension_id = extension_id;
                match save_config_file(&self.config) {
                    Ok(()) => {
                        self.config_editor = ConfigEditor::from_config(&self.config);
                        self.extension_editor = ExtensionEditor::from_config(&self.config);
                        self.extension_editor.error = None;
                        self.reset_native_host_status();
                        self.status_message = Some("extension settings saved".to_owned());
                        true
                    }
                    Err(error) => {
                        self.extension_editor.error = Some(error.to_string());
                        false
                    }
                }
            }
            Err(error) => {
                self.extension_editor.error = Some(error);
                false
            }
        }
    }

    fn render_extension_window(&mut self, ctx: &egui::Context) {
        if !self.show_extension_setup {
            return;
        }

        let mut open = self.show_extension_setup;
        egui::Window::new("Chrome Integration (Native Host)")
            .open(&mut open)
            .default_width(560.0)
            .show(ctx, |ui| {
                if let Some(path) = &self.config_path {
                    ui.label(format!("Config file: {}", path.display()));
                }

                ui.label("Chrome integration uses the Native Host helper: fhast-native-host.exe.");

                config_field(
                    ui,
                    "Chrome extension ID",
                    &mut self.extension_editor.extension_id,
                );

                if self.extension_editor.extension_id.trim().is_empty() {
                    ui.label(
                        RichText::new("Extension ID is not set.")
                            .color(Color32::from_rgb(236, 184, 98)),
                    );
                }

                if let Some(path) = &self.native_host_status.manifest_path {
                    detail_line(
                        ui,
                        "Chrome integration (Native Host)",
                        native_host_status_label(&self.native_host_status),
                    );
                    detail_line(ui, "Manifest", &path.display().to_string());
                }
                if let Some(path) = &self.native_host_status.registry_manifest_path {
                    detail_line(ui, "Registry", &path.display().to_string());
                }
                if self.native_host_status_in_flight {
                    ui.label("Checking native host status...");
                }
                if let Some(error) = &self.native_host_status.error {
                    ui.label(RichText::new(error).color(Color32::from_rgb(236, 184, 98)));
                }

                if let Some(error) = &self.extension_editor.error {
                    ui.label(RichText::new(error).color(Color32::from_rgb(236, 98, 98)));
                }

                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        self.save_extension_settings();
                    }

                    let can_install = !self.extension_editor.extension_id.trim().is_empty();
                    if ui
                        .add_enabled(
                            can_install,
                            egui::Button::new("Register Chrome Integration (Native Host)"),
                        )
                        .clicked()
                        && self.save_extension_settings()
                    {
                        let extension_id = self.config.chrome_extension_id.clone();
                        self.spawn_action(ctx, install_native_host(extension_id));
                    }

                    if ui.button("Reload").clicked() {
                        match load_config_file() {
                            Ok(config) => {
                                self.config = config;
                                self.config_editor = ConfigEditor::from_config(&self.config);
                                self.extension_editor = ExtensionEditor::from_config(&self.config);
                                self.reset_native_host_status();
                                self.status_message =
                                    Some("extension settings reloaded".to_owned());
                            }
                            Err(error) => self.extension_editor.error = Some(error.to_string()),
                        }
                    }
                });
            });
        self.show_extension_setup = open;
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
                detail_line(
                    ui,
                    "Extension ID",
                    if self.config.chrome_extension_id.trim().is_empty() {
                        "not set"
                    } else {
                        "set"
                    },
                );
                if let Some(path) = native_host_manifest_path() {
                    detail_line(
                        ui,
                        "Chrome integration (Native Host)",
                        native_host_status_label(&self.native_host_status),
                    );
                    detail_line(ui, "Manifest", &path.display().to_string());
                }
                if let Some(path) = &self.native_host_status.registry_manifest_path {
                    detail_line(ui, "Registry", &path.display().to_string());
                }
                if self.native_host_status_in_flight {
                    detail_line(ui, "Native host check", "running");
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

    fn render_close_prompt(&mut self, ctx: &egui::Context) {
        if !self.show_close_prompt {
            return;
        }

        egui::Window::new("Close fhast?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("Choose what should happen when this window closes.");
                ui.horizontal(|ui| {
                    if ui.button("Minimize to Tray").clicked() {
                        self.show_close_prompt = false;
                        self.hide_to_tray(ctx);
                    }
                    if ui.button("Close fhast").clicked() {
                        self.show_close_prompt = false;
                        self.request_exit(ctx);
                    }
                    if ui.button("Cancel").clicked() {
                        self.show_close_prompt = false;
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

        if self.has_saved_extension_id()
            && !self.native_host_status_checked
            && !self.native_host_status_in_flight
        {
            self.request_native_host_status(ctx);
        }

        if ctx.input(|input| input.viewport().close_requested()) && !self.exit_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.prompt_for_close(ctx);
        }

        if self.last_refresh.elapsed() >= REFRESH_INTERVAL {
            self.request_refresh(ctx);
            self.last_refresh = Instant::now();
        }

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            self.render_toolbar(ctx, ui);
        });

        if self.show_details {
            egui::SidePanel::right("details")
                .resizable(true)
                .default_width(390.0)
                .show(ctx, |ui| self.render_details(ui));
        }

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
        self.render_extension_window(ctx);
        self.render_events_window(ctx);
        self.render_status_window(ctx);
        self.render_remove_confirmation(ctx);
        self.render_close_prompt(ctx);

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

fn normalize_extension_id(value: &str) -> Result<String, String> {
    let extension_id = value.trim().to_ascii_lowercase();
    if extension_id.is_empty() {
        return Ok(String::new());
    }

    if is_valid_extension_id(&extension_id) {
        Ok(extension_id)
    } else {
        Err("Chrome extension ID must be 32 characters from a to p".to_owned())
    }
}

fn is_valid_extension_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| matches!(byte, b'a'..=b'p'))
}

async fn install_native_host(extension_id: String) -> anyhow::Result<String> {
    tokio::task::spawn_blocking(move || install_native_host_blocking(&extension_id))
        .await
        .context("install native host task failed")?
}

fn install_native_host_blocking(extension_id: &str) -> anyhow::Result<String> {
    let native_host = native_host_binary_path()?;
    let mut command = Command::new(&native_host);
    command
        .arg("--install-host")
        .env("fhast_EXTENSION_ID", extension_id)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let output = command
        .output()
        .with_context(|| format!("run {} --install-host", native_host.display()))?;

    if !output.status.success() {
        anyhow::bail!(
            "Chrome integration (Native Host) registration failed: {}",
            command_output_text(&output.stdout, &output.stderr)
        );
    }

    let output_text = command_output_text(&output.stdout, &output.stderr);
    let message = output_text.trim().lines().last().unwrap_or("");

    if message.is_empty() || message == "native host manifest installed" {
        Ok("Chrome integration (Native Host) registered".to_owned())
    } else {
        Ok(format!("Chrome integration (Native Host): {message}"))
    }
}

fn native_host_binary_path() -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("fhast_NATIVE_HOST") {
        return Ok(PathBuf::from(path));
    }

    let binary_name = if cfg!(windows) {
        "fhast-native-host.exe"
    } else {
        "fhast-native-host"
    };

    let mut sibling = std::env::current_exe().context("resolve current executable")?;
    sibling.set_file_name(binary_name);
    if sibling.exists() {
        return Ok(sibling);
    }

    Ok(PathBuf::from(binary_name))
}

fn native_host_manifest_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let config_path = config_path().ok()?;
        let config_dir = config_path.parent()?;
        Some(
            config_dir
                .join("native-host")
                .join("fhast_native_host.json"),
        )
    }

    #[cfg(not(windows))]
    {
        Some(
            chrome_user_data_dir()?
                .join("NativeMessagingHosts")
                .join("fhast_native_host.json"),
        )
    }
}

async fn load_native_host_status() -> anyhow::Result<NativeHostStatus> {
    tokio::task::spawn_blocking(native_host_status_blocking)
        .await
        .context("native host status task failed")?
}

fn native_host_status_blocking() -> anyhow::Result<NativeHostStatus> {
    let manifest_path = native_host_manifest_path();

    #[cfg(windows)]
    {
        let registry_manifest_path = windows_native_host_manifest_value();
        let installed = registry_manifest_path
            .as_ref()
            .is_some_and(|path| path.exists());
        Ok(NativeHostStatus {
            manifest_path,
            registry_manifest_path,
            installed,
            error: None,
        })
    }

    #[cfg(not(windows))]
    {
        let installed = manifest_path.as_ref().is_some_and(|path| path.exists());
        Ok(NativeHostStatus {
            manifest_path,
            registry_manifest_path: None,
            installed,
            error: None,
        })
    }
}

fn native_host_status_label(status: &NativeHostStatus) -> &'static str {
    if status.installed {
        "registered"
    } else if status
        .manifest_path
        .as_ref()
        .is_some_and(|path| path.exists())
    {
        "manifest exists, not registered"
    } else {
        "not registered"
    }
}

#[cfg(windows)]
fn windows_native_host_manifest_value() -> Option<PathBuf> {
    const REGISTRY_KEY: &str =
        r"HKCU\Software\Google\Chrome\NativeMessagingHosts\fhast_native_host";

    let mut command = Command::new("reg");
    command.args(["query", REGISTRY_KEY, "/ve"]);

    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let value = stdout
        .lines()
        .find_map(|line| line.split_once("REG_SZ").map(|(_, value)| value.trim()))?;
    if value.is_empty() {
        None
    } else {
        Some(PathBuf::from(value))
    }
}

#[cfg(not(windows))]
fn chrome_user_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        Some(
            PathBuf::from(std::env::var_os("HOME")?)
                .join("Library")
                .join("Application Support")
                .join("Google")
                .join("Chrome"),
        )
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
            Some(PathBuf::from(dir).join("google-chrome"))
        } else {
            let home = std::env::var_os("HOME")?;
            Some(PathBuf::from(home).join(".config").join("google-chrome"))
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    None
}

fn command_output_text(stdout: &[u8], stderr: &[u8]) -> String {
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(stdout));
    if !stderr.is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&String::from_utf8_lossy(stderr));
    }
    text.trim().to_owned()
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

fn table_header_cell(ui: &mut egui::Ui, label: &str, width: f32) {
    table_rich_cell(ui, RichText::new(label).strong(), width);
}

fn table_text_cell(ui: &mut egui::Ui, value: String, width: f32) {
    ui.add_sized([width, ROW_HEIGHT], egui::Label::new(value).truncate());
}

fn table_rich_cell(ui: &mut egui::Ui, value: RichText, width: f32) {
    ui.add_sized([width, ROW_HEIGHT], egui::Label::new(value).truncate());
}

fn table_column_gap(ui: &mut egui::Ui) {
    ui.add_space(RESIZE_HANDLE_WIDTH);
}

fn resize_column_handle(
    ui: &mut egui::Ui,
    left_width: &mut f32,
    right_width: &mut f32,
    left_min: f32,
    right_min: f32,
) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(RESIZE_HANDLE_WIDTH, ROW_HEIGHT),
        egui::Sense::click_and_drag(),
    );

    if response.hovered() || response.dragged() {
        ui.output_mut(|output| output.cursor_icon = egui::CursorIcon::ResizeHorizontal);
    }

    if response.dragged() {
        let delta = ui.input(|input| input.pointer.delta().x);
        let min_delta = left_min - *left_width;
        let max_delta = *right_width - right_min;
        let applied = delta.clamp(min_delta, max_delta);
        *left_width += applied;
        *right_width -= applied;
    }

    let color = if response.hovered() || response.dragged() {
        Color32::from_rgb(109, 190, 236)
    } else {
        Color32::from_rgb(63, 68, 75)
    };
    let center_x = rect.center().x;
    ui.painter().line_segment(
        [
            egui::pos2(center_x, rect.top() + 4.0),
            egui::pos2(center_x, rect.bottom() - 4.0),
        ],
        egui::Stroke::new(1.0, color),
    );
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
