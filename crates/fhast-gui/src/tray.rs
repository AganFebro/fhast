#[cfg(windows)]
mod imp {
    use std::sync::mpsc::{self, Receiver};

    use anyhow::Result;
    use egui::{Context, ViewportCommand};
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

    #[derive(Clone, Copy)]
    pub enum TrayCommand {
        Show,
        Exit,
    }

    pub struct TrayController {
        _tray_icon: TrayIcon,
        _show_item: MenuItem,
        _separator: PredefinedMenuItem,
        _exit_item: MenuItem,
        command_rx: Receiver<TrayCommand>,
    }

    impl TrayController {
        pub fn new(ctx: &Context) -> Result<Self> {
            let tray_menu = Menu::new();
            let show_item = MenuItem::with_id(
                tray_icon::menu::MenuId::new("show"),
                "Show fhast",
                true,
                None,
            );
            let exit_item = MenuItem::with_id(tray_icon::menu::MenuId::new("exit"), "Exit", true, None);
            let separator = PredefinedMenuItem::separator();

            tray_menu.append(&show_item)?;
            tray_menu.append(&separator)?;
            tray_menu.append(&exit_item)?;

            let (command_tx, command_rx) = mpsc::channel();
            let show_id = show_item.id().clone();
            let exit_id = exit_item.id().clone();
            let menu_tx = command_tx.clone();
            let menu_ctx = ctx.clone();
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                let command = if event.id == show_id {
                    Some(TrayCommand::Show)
                } else if event.id == exit_id {
                    Some(TrayCommand::Exit)
                } else {
                    None
                };

                if let Some(command) = command {
                    match command {
                        TrayCommand::Show => {
                            menu_ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                            menu_ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                            menu_ctx.send_viewport_cmd(ViewportCommand::Focus);
                        }
                        TrayCommand::Exit => {
                            menu_ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                        }
                    }
                    let _ = menu_tx.send(command);
                    menu_ctx.request_repaint();
                }
            }));

            let tray_tx = command_tx;
            let tray_ctx = ctx.clone();
            TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
                if matches!(event, TrayIconEvent::DoubleClick { .. }) {
                    tray_ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                    tray_ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                    tray_ctx.send_viewport_cmd(ViewportCommand::Focus);
                    let _ = tray_tx.send(TrayCommand::Show);
                    tray_ctx.request_repaint();
                }
            }));

            let icon = tray_icon()?;
            let tray_icon = TrayIconBuilder::new()
                .with_tooltip("fhast")
                .with_icon(icon)
                .with_menu(Box::new(tray_menu))
                .build()?;

            Ok(Self {
                _tray_icon: tray_icon,
                _show_item: show_item,
                _separator: separator,
                _exit_item: exit_item,
                command_rx,
            })
        }

        pub fn poll_command(&self) -> Option<TrayCommand> {
            self.command_rx.try_recv().ok()
        }
    }

    fn tray_icon() -> Result<Icon> {
        let width = 32;
        let height = 32;
        let mut rgba = Vec::with_capacity(width * height * 4);

        for y in 0..height {
            for x in 0..width {
                let edge = x < 2 || y < 2 || x >= width - 2 || y >= height - 2;
                let slash = x + y > 14 && x + y < 20 || x + y > 24 && x + y < 30;
                let active = edge || slash || (x > 7 && x < 25 && y > 13 && y < 19);
                let (r, g, b, a) = if active {
                    (78, 201, 176, 255)
                } else {
                    (18, 18, 20, 255)
                };
                rgba.extend_from_slice(&[r, g, b, a]);
            }
        }

        Ok(Icon::from_rgba(rgba, width as u32, height as u32)?)
    }
}

pub use imp::{TrayCommand, TrayController};
