use anyhow::Result;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub enum TrayCommand {
    Show,
    Exit,
}

pub struct TrayController {
    _tray_icon: TrayIcon,
    show_id: tray_icon::menu::MenuId,
    exit_id: tray_icon::menu::MenuId,
}

impl TrayController {
    pub fn new() -> Result<Self> {
        let tray_menu = Menu::new();
        let show_item = MenuItem::with_id(
            tray_icon::menu::MenuId::new("show"),
            "Show fhast",
            true,
            None,
        );
        let exit_item = MenuItem::with_id(tray_icon::menu::MenuId::new("exit"), "Exit", true, None);

        tray_menu.append(&show_item)?;
        tray_menu.append(&PredefinedMenuItem::separator())?;
        tray_menu.append(&exit_item)?;

        let icon = tray_icon()?;
        let tray_icon = TrayIconBuilder::new()
            .with_tooltip("fhast")
            .with_icon(icon)
            .with_menu(Box::new(tray_menu))
            .build()?;

        Ok(Self {
            _tray_icon: tray_icon,
            show_id: show_item.id().clone(),
            exit_id: exit_item.id().clone(),
        })
    }

    pub fn poll_command(&self) -> Option<TrayCommand> {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.show_id {
                return Some(TrayCommand::Show);
            }
            if event.id == self.exit_id {
                return Some(TrayCommand::Exit);
            }
        }

        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if matches!(event, TrayIconEvent::DoubleClick { .. }) {
                return Some(TrayCommand::Show);
            }
        }

        None
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
