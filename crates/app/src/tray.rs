// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tray icon: open the menu in VR, hide or show everything, start with SteamVR, quit. Events arrive through the
//! main thread's message pump.
use std::time::{Duration, Instant};

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use wxr_core::raster::{ACCENT, BG, Image};
use wxr_sys::light_taskbar;
use wxr_ui::Strings;

/// How often the tray checks whether the taskbar turned light or dark.
const THEME_CHECK: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayCommand {
    OpenMenu,
    HideAll,
    Update,
    AutoLaunch(bool),
    Quit,
}

pub struct Tray {
    icon: TrayIcon,
    light: bool,
    checked: Instant,
    menu: Menu,
    update: Option<MenuItem>,
    open: MenuItem,
    hide: MenuItem,
    autostart: CheckMenuItem,
    quit: MenuItem,
}

/// The WristXR mark from the exe's icons (see build.rs) at the tray's size: lime on a dark taskbar, dark ink on
/// a light one, where lime would be too faint. A build without the icons gets a drawn accent tile instead.
fn icon(light: bool) -> Result<Icon, String> {
    let size = wxr_sys::small_icon_px();
    Icon::from_resource(if light { 3 } else { 2 }, Some((size, size))).or_else(|_| {
        let mut img = Image::new(32, 32, ACCENT);
        img.text(16, 16, 3, "D", BG);
        let rgba: Vec<u8> = img.bgra.as_chunks::<4>().0.iter().flat_map(|[b, g, r, a]| [*r, *g, *b, *a]).collect();
        Icon::from_rgba(rgba, 32, 32).map_err(|e| e.to_string())
    })
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
        let light = light_taskbar();
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu.clone()))
            .with_tooltip(s.get("tray-tooltip", &[]))
            .with_icon(icon(light)?)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { icon, light, checked: Instant::now(), menu, update: None, open, hide, autostart, quit })
    }

    /// Swaps the icon when the taskbar turns light or dark (checked every couple of seconds).
    pub fn follow_taskbar(&mut self) {
        if self.checked.elapsed() < THEME_CHECK {
            return;
        }
        self.checked = Instant::now();
        let light = light_taskbar();
        if light != self.light
            && let Ok(icon) = icon(light)
            && self.icon.set_icon(Some(icon)).is_ok()
        {
            self.light = light;
        }
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
