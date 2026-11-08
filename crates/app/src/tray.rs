//! Tray icon: open the menu in VR, hide or show everything, start with SteamVR, quit. Events arrive through the main thread's message pump.
use dvr_core::raster::{ACCENT, BG, Image};
use dvr_ui::Strings;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayCommand {
    OpenMenu,
    HideAll,
    Update,
    AutoLaunch(bool),
    Quit,
}

pub struct Tray {
    _icon: TrayIcon,
    menu: Menu,
    update: Option<MenuItem>,
    open: MenuItem,
    hide: MenuItem,
    autostart: CheckMenuItem,
    quit: MenuItem,
}

/// 32x32 accent tile with a dark "D", drawn with the built-in font (RGBA for the tray API).
fn icon() -> Result<Icon, String> {
    let mut img = Image::new(32, 32, ACCENT);
    img.text(16, 16, 3, "D", BG);
    let rgba: Vec<u8> = img.bgra.as_chunks::<4>().0.iter().flat_map(|[b, g, r, a]| [*r, *g, *b, *a]).collect();
    Icon::from_rgba(rgba, 32, 32).map_err(|e| e.to_string())
}

impl Tray {
    pub fn new(s: &Strings, autostart: bool) -> Result<Self, String> {
        let open = MenuItem::new(s.get("tray-open", &[]), true, None);
        let hide = MenuItem::new(s.get("tray-hide", &[]), true, None);
        let autostart = CheckMenuItem::new(s.get("tray-autostart", &[]), true, autostart, None);
        let quit = MenuItem::new(s.get("tray-quit", &[]), true, None);
        let menu = Menu::new();
        menu.append_items(&[&open, &hide, &autostart, &PredefinedMenuItem::separator(), &quit])
            .map_err(|e| e.to_string())?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu.clone()))
            .with_tooltip(s.get("tray-tooltip", &[]))
            .with_icon(icon()?)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { _icon: icon, menu, update: None, open, hide, autostart, quit })
    }

    /// Adds the one-click "Download update" entry at the top (no silent install).
    pub fn offer_update(&mut self, s: &Strings, version: &str) {
        let item = MenuItem::new(s.get("tray-update", &[("version", version.to_owned())]), true, None);
        if self.menu.prepend(&item).is_ok() {
            self.update = Some(item);
        }
    }

    /// Keeps the "Start with SteamVR" tick in step with the in-VR setting.
    pub fn set_autostart(&self, on: bool) {
        self.autostart.set_checked(on);
    }

    /// Next tray command, if any (non-blocking).
    pub fn poll(&self) -> Option<TrayCommand> {
        let event = MenuEvent::receiver().try_recv().ok()?;
        if self.update.as_ref().is_some_and(|u| event.id == *u.id()) {
            Some(TrayCommand::Update)
        } else if event.id == *self.open.id() {
            Some(TrayCommand::OpenMenu)
        } else if event.id == *self.hide.id() {
            Some(TrayCommand::HideAll)
        } else if event.id == *self.autostart.id() {
            Some(TrayCommand::AutoLaunch(self.autostart.is_checked()))
        } else if event.id == *self.quit.id() {
            Some(TrayCommand::Quit)
        } else {
            None
        }
    }
}
