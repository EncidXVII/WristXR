//! Tray icon on Linux (StatusNotifierItem, shown by KDE, GNOME with the AppIndicator extension, XFCE, ...): open the
//! menu in VR, hide or show everything, start with SteamVR, quit. The same commands as the Windows tray.
use dvr_ui::Strings;
use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::{Icon, MenuItem, ToolTip};
use std::sync::mpsc::{Receiver, Sender, channel};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayCommand {
    OpenMenu,
    HideAll,
    Update,
    AutoLaunch(bool),
    Quit,
}

struct Labels {
    tooltip: String,
    open: String,
    hide: String,
    autostart: String,
    quit: String,
}

struct Item {
    tx: Sender<TrayCommand>,
    labels: Labels,
    autostart: bool,
    update: Option<String>,
    icons: Vec<Icon>,
}

/// The app icon (the mark on its tile, which reads on light and dark panels alike) as ARGB32, network order.
fn icons() -> Vec<Icon> {
    [
        &include_bytes!("../../../packaging/brand/desktopvr-32.png")[..],
        &include_bytes!("../../../packaging/brand/desktopvr-64.png")[..],
    ]
    .into_iter()
    .filter_map(|bytes| {
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().ok()?;
        let mut buf = vec![0; reader.output_buffer_size()?];
        let info = reader.next_frame(&mut buf).ok()?;
        if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
            return None;
        }
        let data =
            buf[..info.buffer_size()].as_chunks::<4>().0.iter().flat_map(|[r, g, b, a]| [*a, *r, *g, *b]).collect();
        Some(Icon { width: info.width as i32, height: info.height as i32, data })
    })
    .collect()
}

impl ksni::Tray for Item {
    fn id(&self) -> String {
        "desktopvr".into()
    }

    fn title(&self) -> String {
        "DesktopVR".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        self.icons.clone()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip { title: self.labels.tooltip.clone(), ..Default::default() }
    }

    /// A click on the icon opens the menu in VR, like the first menu entry.
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.tx.send(TrayCommand::OpenMenu);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let send = |cmd: TrayCommand| {
            Box::new(move |t: &mut Self| {
                let _ = t.tx.send(cmd);
            })
        };
        let mut items: Vec<MenuItem<Self>> = Vec::new();
        if let Some(label) = &self.update {
            items.push(
                StandardItem { label: label.clone(), activate: send(TrayCommand::Update), ..Default::default() }.into(),
            );
        }
        items.push(
            StandardItem {
                label: self.labels.open.clone(),
                activate: send(TrayCommand::OpenMenu),
                ..Default::default()
            }
            .into(),
        );
        items.push(
            StandardItem {
                label: self.labels.hide.clone(),
                activate: send(TrayCommand::HideAll),
                ..Default::default()
            }
            .into(),
        );
        items.push(
            CheckmarkItem {
                label: self.labels.autostart.clone(),
                checked: self.autostart,
                activate: Box::new(|t: &mut Self| {
                    t.autostart = !t.autostart;
                    let _ = t.tx.send(TrayCommand::AutoLaunch(t.autostart));
                }),
                ..Default::default()
            }
            .into(),
        );
        items.push(MenuItem::Separator);
        items.push(
            StandardItem { label: self.labels.quit.clone(), activate: send(TrayCommand::Quit), ..Default::default() }
                .into(),
        );
        items
    }
}

pub struct Tray {
    handle: Handle<Item>,
    rx: Receiver<TrayCommand>,
}

impl Tray {
    pub fn new(s: &Strings, autostart: bool) -> Result<Self, String> {
        let (tx, rx) = channel();
        let labels = Labels {
            tooltip: s.get("tray-tooltip", &[]),
            open: s.get("tray-open", &[]),
            hide: s.get("tray-hide", &[]),
            autostart: s.get("tray-autostart", &[]),
            quit: s.get("tray-quit", &[]),
        };
        let item = Item { tx, labels, autostart, update: None, icons: icons() };
        let handle = item.spawn().map_err(|e| e.to_string())?;
        Ok(Self { handle, rx })
    }

    /// Linux panels draw the tile icon on any background; nothing to follow.
    pub fn follow_taskbar(&mut self) {}

    /// Adds the one-click "Download update" entry at the top (no silent install).
    pub fn offer_update(&mut self, s: &Strings, version: &str) {
        let label = s.get("tray-update", &[("version", version.to_owned())]);
        self.handle.update(|t| t.update = Some(label));
    }

    /// Keeps the "Start with SteamVR" tick in step with the in-VR setting.
    pub fn set_autostart(&self, on: bool) {
        self.handle.update(|t| t.autostart = on);
    }

    /// Next tray command, if any (non-blocking).
    pub fn poll(&self) -> Option<TrayCommand> {
        self.rx.try_recv().ok()
    }
}
