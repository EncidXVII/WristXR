//! The VR session on Windows: OpenVR overlays, WGC capture, SendInput, Slint menu and wrist.
use crate::bindings::APP_KEY;
use crate::session::{Effect, MonitorInfo, Page, PointerInput, Session, TAB_INBOX, WINDOW_NEARER};
use crate::tray::{Tray, TrayCommand};
use dvr_capture_win::{Capture, Frame, Monitor};
use dvr_core::backend::VrBackend;
use dvr_core::chat::{ChatAction, ChatLine};
use dvr_core::engine::{Claims, Engine, Event, FrameInput, MouseButton};
use dvr_core::keyboard::{CAPS, CHAT, LALT, LANG, LCTRL, LSHIFT, LWIN, ROWS, RSHIFT, SPACE};
use dvr_core::math::{Pose, Quat, Vec3};
use dvr_core::panel::{BANNER_ID, DEFAULT_WIDTH_M, Glance, Hand, MIN_WIDTH_M, Panel, PixelRect, Source};
use dvr_core::raster;
use dvr_core::state::NotifyFrom;
use dvr_core::state::State;
use dvr_gfx_d3d11::{Detail, Gpu, GpuTimer, Sharpen, level_for, raw};
use dvr_input_win::Injector;
use dvr_notify_api::{Listener, Toast};
use dvr_osc::{Chatbox, VRCHAT_ADDR};
use dvr_sys::{AppToasts, Media, MediaCommand, MediaInfo, ToastAccess};
use dvr_ui::{
    BANNER_SIZE, Banner, KEYBOARD_SIZE, KeyFace, Keyboard, LOW_BATTERY, MENU_SIZE, Menu, Strings, TOOLBAR_SIZE,
    Toolbar, WRIST_SIZE, Wrist, WristAdd, WristInfo, WristInput, WristLoad, WristMedia, WristWindow,
};
use dvr_vr::{OpenVr, cursor_key};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;

const SAVE_DELAY: Duration = Duration::from_secs(1);
const CURSOR_LIFT_M: f32 = 0.003;
const STANDING_HEAD: Pose = Pose::new(Vec3::new(0.0, 1.6, 0.0), Quat::IDENTITY);
const LAYOUT_POLL: Duration = Duration::from_millis(500);
const STATS_PERIOD: Duration = Duration::from_secs(1);
/// Cleanup after quitting gets this long before the process is ended anyway.
const EXIT_GRACE: Duration = Duration::from_secs(3);
/// A popped-out window appears this far in front of the hand that pulled it.
const POP_OUT_DISTANCE_M: f32 = 0.3;
/// Characters of the chat line that fit on the space bar.
const SPACE_BAR_CHARS: usize = 24;
/// The watch stays expanded this long after the laser leaves it.
const WRIST_LINGER_MS: u64 = 2500;
/// How long a notification from another app (Discord, ...) stays on the watch.
const APP_TOAST_S: f32 = 6.0;
/// The banner comes in over about this long and goes in a little less (critically damped: no bounce).
const BANNER_IN_S: f32 = 0.35;
const BANNER_OUT_S: f32 = 0.25;
/// Pointing at the banner keeps it; once the laser leaves, it stays at least this long.
const BANNER_LINGER_S: f32 = 2.0;
/// How often the watch's playing mark steps while it moves.
const BEAT_STEP: Duration = Duration::from_millis(22);

/// A screen goes to a coarser level only after wanting it this long (a finer one comes at once), so walking about
/// or a panel in hand never flips it back and forth.
const COARSER_AFTER: Duration = Duration::from_millis(600);

/// GPU side of a panel: its texture and, for visible monitor panels, a live capture.
struct Surface {
    /// What SteamVR shows (for a screen or window, its `detail` output).
    tex: ID3D11Texture2D,
    /// Screens and windows: the capture with its mip chain, shown at the level their size in view needs.
    detail: Option<Detail>,
    /// A coarser level wanted since then.
    coarser: Option<(u32, Instant)>,
    capture: Option<Capture>,
    rect: Option<PixelRect>,
    needs_submit: bool,
    /// Source is gone (window closed): showing a placeholder, no capture attempts.
    dead: bool,
}

const WINDOW_CLOSED: &str = "WINDOW CLOSED";

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Key labels in the active OS layout, in the case they will type (Shift latched / Caps Lock). The language key
/// shows the current language.
fn layout_labels(shift: bool, caps: bool) -> Vec<String> {
    let lang = dvr_input_win::layout_name();
    ROWS.iter()
        .flat_map(|r| r.iter())
        .map(|key| {
            if key.sc == LANG {
                return lang.clone();
            }
            let typed = (key.label.chars().count() == 1)
                .then(|| dvr_input_win::key_char(key.sc, shift, caps))
                .flatten()
                .filter(|c| !c.is_whitespace());
            typed.map_or_else(|| key.label.to_owned(), |c| c.to_string())
        })
        .collect()
}

/// What each key shows: label (space bar = chat line in chat mode), lit, laser hover.
fn key_faces(
    faces: &mut Vec<KeyFace>,
    labels: &[String],
    chat: Option<&str>,
    lit: impl Fn(u16) -> bool,
    hover: &[Option<u16>; 2],
) {
    faces.clear();
    for (key, label) in ROWS.iter().flat_map(|r| r.iter()).zip(labels) {
        let label = match (key.sc, chat) {
            (SPACE, Some(text)) => {
                let skip = text.chars().count().saturating_sub(SPACE_BAR_CHARS);
                text.chars().skip(skip).collect()
            }
            _ => label.clone(),
        };
        faces.push(KeyFace { label, lit: lit(key.sc), hover: hover.contains(&Some(key.sc)) });
    }
}

/// `sharp`: screens and windows get their detail pass (see `Detail`).
fn surface(gpu: &Gpu, sharp: bool, panel: &Panel, monitors: &[Monitor]) -> Result<Surface, String> {
    let plain = |tex: ID3D11Texture2D| Surface {
        tex,
        detail: None,
        coarser: None,
        capture: None,
        rect: None,
        needs_submit: false,
        dead: false,
    };
    let blank =
        |size: (u32, u32)| -> Result<Surface, String> { Ok(plain(gpu.texture(size.0, size.1, None).map_err(err)?)) };
    let screen = |r: PixelRect| -> Result<Surface, String> {
        if !sharp {
            return Ok(Surface { rect: Some(r), ..blank((r.w, r.h))? });
        }
        let d = Detail::new(gpu, r.w, r.h, 0).map_err(err)?;
        Ok(Surface { rect: Some(r), ..plain(d.out.clone()) }.with_detail(d))
    };
    let (img, rect) = match panel.source {
        Source::Keyboard => return blank(KEYBOARD_SIZE),
        Source::Monitor(i) => match monitors.get(i as usize) {
            Some(m) => return screen(m.rect),
            None => (raster::placeholder(1280, 720, "MONITOR NOT CONNECTED"), None),
        },
        Source::Window(h) => match dvr_capture_win::window_rect(h as isize) {
            Some(r) => return screen(r),
            None => {
                let img = raster::placeholder(1280, 720, WINDOW_CLOSED);
                let tex = gpu.image(&img).map_err(err)?;
                return Ok(Surface { needs_submit: true, dead: true, ..plain(tex) });
            }
        },
        Source::Wrist => return blank(WRIST_SIZE),
        Source::Menu => return blank(MENU_SIZE),
        Source::Toolbar => return blank(TOOLBAR_SIZE),
        Source::Banner => return blank(BANNER_SIZE),
    };
    Ok(Surface { rect, needs_submit: true, ..plain(gpu.image(&img).map_err(err)?) })
}

impl Surface {
    fn with_detail(mut self, d: Detail) -> Self {
        self.detail = Some(d);
        self
    }
}

/// What the watch shows, filtered by the Wrist settings.
fn wrist_info(
    vr: &OpenVr,
    state: &State,
    expanded: bool,
    load: &dvr_sys::Load,
    own_gpu_ms: f32,
    toast: Option<&(Toast, Instant)>,
    media: Option<&MediaInfo>,
) -> WristInfo {
    let (hour, minute) = dvr_sys::local_time();
    let (weekday, day, month) = dvr_sys::local_date();
    let stats = vr.stats();
    let battery = state.wrist_battery;
    let layout = state.wrist_layout;
    WristInfo {
        hour,
        minute,
        weekday,
        day,
        month,
        layout,
        controllers: stats.controllers.map(|b| b.level.filter(|_| battery)),
        controllers_on: stats.controllers.map(|b| b.on && battery),
        charging: stats.controllers.map(|b| b.charging),
        lowest_tracker: stats.lowest_tracker.filter(|_| battery),
        expanded,
        // The Performance and Game layouts show the load whatever the FPS and load switch says.
        load: (state.wrist_load || layout.shows_load()).then_some(WristLoad {
            game_fps: stats.game_fps,
            target_fps: stats.display_hz,
            cpu: load.cpu,
            gpu: load.gpu,
            own_gpu_ms,
        }),
        toast: toast.map(|(t, _)| (t.source_app.clone(), t.title.clone(), t.content.clone())),
        // The position only shows (and changes) while the watch is open.
        media: media.filter(|_| state.wrist_media).map(|m| WristMedia {
            title: m.title.clone(),
            artist: m.artist.clone(),
            app: m.app.clone(),
            playing: m.playing,
            position: if expanded { m.position_now() } else { 0.0 },
            duration: m.duration,
            shuffle: m.shuffle,
        }),
        ..WristInfo::default()
    }
}

/// Windows the user can add, front to back.
fn list_windows() -> Vec<(u64, String)> {
    dvr_capture_win::windows().into_iter().map(|(h, title)| (h as u64, title)).collect()
}

/// The watch's Add list: the first windows, marked when already shown.
fn wrist_windows(windows: &[(u64, String)], panels: &[Panel]) -> Vec<WristWindow> {
    windows
        .iter()
        .map(|(h, title)| WristWindow {
            id: *h,
            title: title.clone(),
            shown: panels.iter().any(|p| p.source == Source::Window(*h)),
        })
        .collect()
}

/// "Title – Artist" for the VRChat chatbox.
fn now_playing_line(m: &MediaInfo) -> String {
    if m.artist.is_empty() { m.title.clone() } else { format!("{} – {}", m.title, m.artist) }
}

/// A window shown as its own panel in front of the user, at the physical scale it has on the main screen.
fn window_panel(id: u32, hwnd: isize, head: Pose, screens: &[MonitorInfo]) -> Option<Panel> {
    let wr = dvr_capture_win::window_rect(hwnd)?;
    let main_w = screens.iter().find(|m| m.primary).or(screens.first()).map_or(1920, |m| m.rect.w);
    let mut p = Panel::in_front_of(id, Source::Window(hwnd as u64), head, wr.h as f32 / wr.w as f32);
    p.width_m = (DEFAULT_WIDTH_M * wr.w as f32 / main_w as f32).clamp(0.3, DEFAULT_WIDTH_M * 1.5);
    p.pose.pos = head.pos + (p.pose.pos - head.pos) * WINDOW_NEARER;
    Some(p)
}

pub fn run() -> Result<(), String> {
    let state_path = State::default_path().ok_or("no config directory")?;
    let dir = state_path.parent().ok_or("no config directory")?.to_path_buf();
    let manifest = crate::bindings::install(&dir.join("input")).map_err(err)?;

    let mut vr = OpenVr::connect()?;
    // SteamVR knows us as an app (dashboard overlay) so it can start us with itself; first run opts in. This comes
    // before the actions, so even the very first launch gets its buttons.
    let app = std::env::current_exe()
        .map_err(err)
        .and_then(|exe| crate::bindings::install_vrmanifest(&dir, &exe).map_err(err))
        .and_then(|m| vr.register_app(&m, APP_KEY));
    vr.load_actions(&manifest)?;
    let gpu = Gpu::new(vr.adapter_index()).map_err(err)?;
    // Screens at the detail the headset shows; without a shader compiler they are shown as captured.
    let sharpen = Sharpen::new(&gpu).inspect_err(|e| tracing::warn!(%e, "screen detail pass unavailable")).ok();
    let sharp = sharpen.is_some();
    let monitors = dvr_capture_win::monitors();
    let screens: Vec<MonitorInfo> = monitors.iter().map(|m| MonitorInfo { rect: m.rect, primary: m.primary }).collect();
    let head = |vr: &OpenVr| vr.head_pose().unwrap_or(STANDING_HEAD);

    let mut state = State::load(&state_path);
    let imported = if state.onboarded { None } else { crate::import::detect() };
    if let Some(imp) = &imported {
        crate::import::apply(&mut state, imp);
    }
    let mut panels = state.panels.clone();
    let mut session = Session::new(state, &mut panels, head(&vr));
    session.imported = imported.is_some();
    session.language = dvr_input_win::layout_name();
    session.languages = dvr_input_win::layouts();
    // Names for windows kept from last time (toolbar chip, "Open now").
    for (h, title) in dvr_capture_win::windows() {
        if panels.iter().any(|p| p.source == Source::Window(h as u64)) {
            session.window_names.insert(h as u64, title);
        }
    }

    match app {
        Ok(()) if !session.state.onboarded => {
            if let Err(e) = vr.set_auto_launch(APP_KEY, true) {
                tracing::warn!(%e, "auto-launch not set");
            }
        }
        Ok(()) => {}
        Err(e) => tracing::warn!(%e, "SteamVR app registration failed"),
    }
    session.auto_launch = vr.auto_launch(APP_KEY);
    session.persist(&panels, None);
    session.state.save(&state_path).map_err(err)?;

    let mut cursor = gpu.image(&raster::cursor(64, session.state.accent.rgb())).map_err(err)?;
    for hand in Hand::BOTH {
        vr.set_texture_d3d11(cursor_key(hand), raw(&cursor))?;
    }
    let mut chat = ChatLine::default();
    let mut surfaces = HashMap::new();
    for p in &panels {
        surfaces.insert(p.id, surface(&gpu, sharp, p, &monitors)?);
    }

    let strings = Strings::english()?;
    // No tray (e.g. no shell) is not fatal: the menu in VR has everything.
    let mut tray =
        Tray::new(&strings, vr.auto_launch(APP_KEY)).inspect_err(|e| tracing::warn!(%e, "tray unavailable")).ok();
    let chatbox = Chatbox::new(VRCHAT_ADDR.parse().map_err(err)?).map_err(err)?;
    let mut layout = dvr_input_win::layout_id();
    let mut layout_checked = Instant::now();
    let mut engine = Engine::new(panels);
    let mut injector = Injector::new();
    let mut input = FrameInput::default();
    let mut claims = [Claims::default(); 2];
    let mut events = Vec::with_capacity(64);
    let mut dirty: Option<Instant> = None;
    let mut resized: Vec<(u32, u32, u32)> = Vec::with_capacity(8);
    // Follow Windows' "Animation effects" setting: the watch grows, switches slide, the menu fades in.
    let motion = dvr_sys::animations_enabled();
    dvr_ui::set_motion(motion);
    session.motion = motion;
    let mut wrist = Wrist::new()?;
    let media = Media::start();
    let mut now_playing: Option<MediaInfo> = None;
    let mut wrist_hovered = false;
    let mut menu = Menu::new()?;
    let mut toolbar = Toolbar::new()?;
    // Toolbar view shows this panel's buttons (rebuilt when the panel or its settings change).
    let mut toolbar_for: Option<u32> = None;
    let mut toolbar_size: Option<(u32, i32)> = None;
    let mut wrist_seen: Option<u64> = None;
    let mut wrist_expanded = false;
    let mut wrist_flags = (false, false);
    let mut last_ms = 0u64;
    let mut glance = Glance::default();
    let mut sampler = dvr_sys::Sampler::new();
    let mut load = dvr_sys::Load::default();
    let mut timer = GpuTimer::new(&gpu).map_err(err)?;
    // Another app (e.g. XSOverlay) may own the port; the overlay still runs without toasts.
    let mut notifications =
        Listener::bind(dvr_notify_api::PORT).inspect_err(|e| tracing::warn!(%e, "notification API unavailable")).ok();
    session.notifications_ok = notifications.is_some();
    let mut toast: Option<(Toast, Instant)> = None;
    // Administrator windows already explained (see `admin_window_at`).
    let mut admin_told: Vec<String> = Vec::new();
    // Windows notifications from other apps; started only when wanted (Windows may ask for access once).
    let mut app_toasts = (session.state.notify_from != NotifyFrom::Off).then(AppToasts::start);
    let mut keyboard = Keyboard::new()?;
    let accent = session.state.accent.rgb();
    wrist.set_accent(accent);
    menu.set_accent(accent);
    toolbar.set_accent(accent);
    keyboard.set_accent(accent);
    // The open watch's Add list is showing (closes with the watch).
    let mut wrist_adding = false;
    let mut faces: Vec<KeyFace> = Vec::with_capacity(80);
    let mut held: Vec<u16> = Vec::with_capacity(8);
    let mut labels = layout_labels(false, dvr_input_win::caps_lock());
    let mut key_state = None;
    let mut battery_low = [false; 2];
    let mut stats_at = Instant::now() - STATS_PERIOD;
    let start = Instant::now();
    // The head while the headset is worn (the layout is saved around it), and whether the saved layout has been
    // brought to the wearer yet this run.
    let mut worn_head: Option<Pose> = None;
    let mut beat_at = Instant::now();
    let mut banner = Banner::new()?;
    // How far the banner has come in (0..1) and how fast it is moving.
    let mut banner_shown = (0.0f32, 0.0f32);

    let updates =
        if session.state.update_check { crate::update::check_in_background() } else { std::sync::mpsc::channel().1 };
    let mut release = None;
    // SteamVR normally says goodbye with a quit event; if its server dies instead, we notice here and leave too.
    let steamvr = dvr_sys::ProcessWatch::by_name("vrserver.exe");
    // Backstop: even if a SteamVR call never returns once SteamVR is gone, the process still ends.
    if let Some(w) = dvr_sys::ProcessWatch::by_name("vrserver.exe") {
        w.exit_when_gone(Duration::from_secs(5));
    }
    'frames: while vr.poll(start.elapsed().as_millis() as u64, &mut input) {
        dvr_sys::pump_messages();
        if let Ok(r) = updates.try_recv() {
            if let Some(t) = tray.as_mut() {
                t.offer_update(&strings, &r.version);
            }
            release = Some(r);
        }
        if let Some(t) = tray.as_mut() {
            t.follow_taskbar();
        }
        let mut tray_open_menu = false;
        while let Some(cmd) = tray.as_ref().and_then(Tray::poll) {
            match cmd {
                TrayCommand::OpenMenu => tray_open_menu = !session.menu_open(&engine.panels),
                TrayCommand::HideAll => session.toggle_hidden(&mut engine.panels, head(&vr)),
                TrayCommand::AutoLaunch(on) => {
                    session.auto_launch = on;
                    if let Err(e) = vr.set_auto_launch(APP_KEY, on) {
                        tracing::warn!(%e, "auto-launch change failed");
                    }
                }
                TrayCommand::Update => {
                    if let Some(r) = &release {
                        crate::update::open(&r.url);
                    }
                }
                TrayCommand::Quit => break 'frames,
            }
        }
        if tray_open_menu {
            session.toggle_menu(&mut engine.panels, head(&vr));
        }
        let poses = input.anchors();
        let now = input.now_ms;
        let dt = now.saturating_sub(last_ms) as f32 / 1000.0;
        last_ms = now;
        // The watch shows while you look at it (or point at it, or a notification is up).
        let head_now = head(&vr);
        // How much detail the headset shows per radian (SteamVR's resolution can change while running).
        let ppr = vr.pixels_per_radian();
        if vr.worn()
            && let Some(h) = vr.head_pose()
        {
            if worn_head.is_none() {
                session.arrive(&mut engine.panels, h);
            }
            worn_head = Some(h);
        }
        let watch = match engine.panels.iter().find(|p| p.source == Source::Wrist) {
            Some(w) if session.state.wrist_fade && !wrist_expanded && toast.is_none() => {
                f32::from(u8::from(glance.update(head_now, w.world_pose(&poses), dt)))
            }
            _ => 1.0,
        };
        session.update_dims(&mut engine.panels, watch, dt);
        engine.raw_laser = !session.state.smoothing;
        engine.hold_right_click = session.state.hold_right_click;
        engine.drag_scroll = session.state.drag_scroll;
        let grabbed = engine.grabbed();
        session.ease_sizes(&mut engine.panels, grabbed, dt);
        session.place_toolbar(&mut engine.panels, &poses);
        events.clear();
        // The engine sees only the controls this hand has taken from the game; the claims for the next frame
        // follow what it now points at or holds, handed over only while those controls are let go.
        engine.step(&input.claimed(claims), &mut events);
        claims = Hand::BOTH.map(|h| claims[h.index()].settle(engine.wants(h), &input.hands[h.index()]));
        vr.set_claims(claims);
        let mut cursors: [Option<(u32, [f32; 2])>; 2] = [None; 2];
        let mut key_hover: [Option<u16>; 2] = [None; 2];
        // Popped-out windows can move on the desktop; input maps through their current rectangle. A window that
        // was closed takes its panel with it.
        let mut closed = Vec::new();
        for p in &engine.panels {
            if let (Source::Window(h), Some(s)) = (p.source, surfaces.get_mut(&p.id)) {
                s.rect = dvr_capture_win::window_rect(h as isize);
                if s.rect.is_none() {
                    closed.push(p.id);
                }
            }
        }
        for id in closed {
            engine.panels.retain(|p| p.id != id);
            surfaces.remove(&id);
            vr.remove_panel(id);
            dirty = Some(Instant::now());
        }
        let mut pop = None;
        let seen = PointerInput { events: &events, now, rays: engine.rays(), anchors: poses };
        session.pointer(seen, &engine.panels, &menu, &toolbar);
        session.place_toolbar(&mut engine.panels, &poses);
        // The other hand's laser on the wrist drives its media buttons.
        let on_wrist = |id: u32| engine.panels.iter().any(|p| p.id == id && p.source == Source::Wrist);
        let mut over_wrist = false;
        let mut holds_open = false;
        for e in &events {
            match *e {
                Event::Hover { panel, uv, .. } if on_wrist(panel) => {
                    over_wrist = true;
                    holds_open |= wrist.opens_at(uv, wrist_expanded);
                    wrist.pointer_move(uv);
                }
                Event::Mouse { panel, uv, button: MouseButton::Left, down } if on_wrist(panel) => {
                    over_wrist = true;
                    holds_open |= wrist.opens_at(uv, wrist_expanded);
                    wrist.pointer_button(uv, down);
                }
                _ => {}
            }
        }
        if wrist_hovered && !over_wrist {
            wrist.pointer_leave();
        }
        wrist_hovered = over_wrist;
        // Only the time row opens the watch, so the glance strip's pause button works without opening it.
        if holds_open {
            wrist_seen = Some(now);
        }
        let expanded = wrist_seen.is_some_and(|t| now.saturating_sub(t) <= WRIST_LINGER_MS);
        let mut refresh_wrist = expanded != wrist_expanded;
        wrist_expanded = expanded;
        if !expanded {
            wrist_adding = false;
        }
        let mut wrist_effects: Vec<Effect> = Vec::new();
        let mut menu_dirty = tray_open_menu;
        let mut redraw_keyboard = false;
        for input in wrist.take_inputs() {
            match input {
                WristInput::Menu => {
                    session.toggle_menu(&mut engine.panels, head_now);
                    menu_dirty = true;
                }
                WristInput::Keyboard => {
                    session.wrist_keyboard(&mut engine.panels, head_now);
                    redraw_keyboard = true;
                    dirty = Some(Instant::now());
                }
                WristInput::HideAll => session.toggle_hidden(&mut engine.panels, head_now),
                WristInput::Inbox => {
                    session.open_menu_at(Page::Tab(TAB_INBOX), &mut engine.panels, head_now);
                    menu_dirty = true;
                }
                WristInput::Previous => media.send(MediaCommand::Previous),
                WristInput::PlayPause => media.send(MediaCommand::PlayPause),
                WristInput::Next => media.send(MediaCommand::Next),
                WristInput::Shuffle => media.send(MediaCommand::ToggleShuffle),
                WristInput::Mute => media.send(MediaCommand::ToggleMute),
                WristInput::Volume(f) => media.send(MediaCommand::Volume(f)),
                WristInput::Seek(f) => {
                    if let Some(m) = &now_playing {
                        media.send(MediaCommand::Seek(f * m.duration));
                    }
                }
                WristInput::OpenAdd => {
                    session.windows = list_windows();
                    wrist_adding = true;
                }
                WristInput::AddBack => wrist_adding = false,
                WristInput::AddScreen => {
                    wrist_effects.extend(session.add_next_screen(&mut engine.panels, head_now, &screens));
                    wrist_adding = false;
                    dirty = Some(Instant::now());
                }
                WristInput::AddWindow(h) => {
                    wrist_effects.extend(session.add_window(h, &mut engine.panels, head_now, &screens));
                    wrist_adding = false;
                    dirty = Some(Instant::now());
                }
                WristInput::AddMore => {
                    session.windows = list_windows();
                    session.open_menu_at(Page::Windows(0), &mut engine.panels, head_now);
                    wrist_adding = false;
                    menu_dirty = true;
                }
                WristInput::Share => {
                    if let Some(m) = media.info() {
                        let name = now_playing_line(&m);
                        if let Err(e) = chatbox.send_quiet(&strings.get("chat-now-playing", &[("track", name)])) {
                            tracing::warn!(%e, "now-playing send failed");
                        }
                    }
                }
            }
            refresh_wrist = true;
        }
        for e in &events {
            let rect = |panel: u32| surfaces.get(&panel).and_then(|s: &Surface| s.rect);
            match *e {
                Event::Hover { hand, panel, uv, .. } => {
                    cursors[hand.index()] = Some((panel, uv));
                    if engine.panels.iter().any(|p| p.id == panel && p.source == Source::Keyboard) {
                        key_hover[hand.index()] = dvr_core::keyboard::key_at(uv).map(|k| k.sc);
                    }
                }
                Event::Key { scancode, down: true } if !held.contains(&scancode) => held.push(scancode),
                Event::Key { scancode, down: false } => held.retain(|k| *k != scancode),
                _ => {}
            }
            if let Event::Mouse { panel, down: true, .. } = *e
                && let Some(Source::Window(h)) = engine.panels.iter().find(|p| p.id == panel).map(|p| p.source)
            {
                dvr_capture_win::focus(h as isize);
            }
            // A program running as administrator drops our clicks without a word (Windows' UIPI): say why, once
            // per window, instead of leaving the user clicking at nothing.
            if let Event::Mouse { panel, uv, down: true, .. } = *e
                && let Some(r) = rect(panel)
                && let Some(title) = dvr_input_win::admin_window_at(r.pixel_at(uv))
                && !admin_told.contains(&title)
            {
                admin_told.push(title.clone());
                let app = if title.is_empty() { strings.get("admin-this-window", &[]) } else { title };
                let (head, body) =
                    (strings.get("admin-title", &[("app", app.clone())]), strings.get("admin-body", &[("app", app)]));
                banner.set("DesktopVR", &head, &body, &strings.get("wrist-now", &[]));
                let t = Toast { title: head, content: body, source_app: "DesktopVR".into(), timeout_s: APP_TOAST_S };
                toast = Some((t, Instant::now()));
                refresh_wrist = true;
            }
            match *e {
                Event::PopOut { hand, panel, uv } => pop = Some((hand, panel, uv)),
                Event::Key { scancode, down } if session.key(scancode, down, dvr_input_win::key_char) => {
                    menu_dirty = true;
                }
                Event::Key { scancode, down } if session.state.chatbox => {
                    let sent = match chat.key(scancode, down, dvr_input_win::key_char) {
                        ChatAction::None => Ok(()),
                        ChatAction::Edited => {
                            redraw_keyboard = true;
                            chatbox.typing(!chat.text().is_empty())
                        }
                        ChatAction::Send(text) => {
                            redraw_keyboard = true;
                            chatbox.send(&text)
                        }
                    };
                    if let Err(e) = sent {
                        tracing::warn!(%e, "chatbox send failed");
                    }
                }
                Event::Hover { .. } | Event::Mouse { .. } | Event::Scroll { .. } | Event::Key { .. } => {
                    injector.apply(e, rect);
                }
                Event::ChatToggle => {
                    menu_dirty = true;
                    session.state.chatbox = !session.state.chatbox;
                    if !session.state.chatbox {
                        let _ = chatbox.typing(false);
                    }
                    redraw_keyboard = true;
                    dirty = Some(Instant::now());
                }
                Event::Haptic { hand } if session.state.haptics => vr.haptic(hand),
                Event::Haptic { .. } => {}
                Event::HideToggle => {
                    session.toggle_hidden(&mut engine.panels, head_now);
                    if session.state.haptics {
                        vr.haptic(Hand::Left);
                    }
                    menu_dirty = true;
                }
                Event::NextLanguage => {
                    dvr_input_win::next_layout();
                }
                Event::Moved { .. } => dirty = Some(Instant::now()),
            }
            if session.state.typing_sounds && matches!(*e, Event::Key { down: true, .. }) {
                dvr_sys::play_click();
            }
        }

        if let Some((hand, panel, uv)) = pop
            && let Some(from) = engine.panels.iter().find(|p| p.id == panel).copied()
            && let Some(rect) = surfaces.get(&panel).and_then(|s| s.rect)
            && let Some(h) = dvr_capture_win::window_at(rect.pixel_at(uv))
            && let Some(wr) = dvr_capture_win::window_rect(h)
        {
            // Same physical scale it had on the monitor, in front of the hand that pulled it.
            let id = crate::session::next_id(&engine.panels);
            let width = (from.width_m * wr.w as f32 / rect.w as f32).max(MIN_WIDTH_M);
            let at = poses[hand.index()];
            let pose = Pose::new(at.pos + at.forward() * POP_OUT_DISTANCE_M, from.world_pose(&poses).rot);
            let p = Panel::new(id, Source::Window(h as u64), pose, width, wr.h as f32 / wr.w as f32);
            surfaces.insert(id, surface(&gpu, sharp, &p, &monitors)?);
            engine.panels.push(p);
            engine.start_grab(hand, id, &poses);
            if let Some((_, title)) = dvr_capture_win::windows().into_iter().find(|(w, _)| *w == h) {
                session.window_names.insert(h as u64, title);
            }
            menu_dirty = true;
            dirty = Some(Instant::now());
        }

        let mut effects: Vec<Effect> = wrist_effects;
        for click in menu.take_inputs() {
            effects.extend(session.input(click, &mut engine.panels, head_now, &poses, &screens));
            dirty = Some(Instant::now());
            menu_dirty = true;
            // Watch settings (layout, what it shows) show on the watch at once.
            refresh_wrist = true;
        }
        let mut toolbar_dirty = session.toolbar_target() != toolbar_for;
        for click in toolbar.take_inputs() {
            effects.extend(session.toolbar_input(click, &mut engine.panels, &poses, head_now));
            dirty = Some(Instant::now());
            toolbar_dirty = true;
        }
        for fx in effects {
            match fx {
                Effect::Added(id) => {
                    if let Some(p) = engine.panels.iter().find(|p| p.id == id) {
                        surfaces.insert(id, surface(&gpu, sharp, p, &monitors)?);
                    }
                }
                Effect::Removed(id) => {
                    surfaces.remove(&id);
                    vr.remove_panel(id);
                }
                Effect::RedrawKeyboard => redraw_keyboard = true,
                Effect::Quit => break 'frames,
                Effect::ListWindows => session.windows = list_windows(),
                Effect::Rebind => {
                    if let Err(e) = vr.open_bindings(APP_KEY) {
                        tracing::warn!(%e, "binding screen not opened");
                    }
                }
                Effect::Accent => {
                    let accent = session.state.accent.rgb();
                    wrist.set_accent(accent);
                    menu.set_accent(accent);
                    toolbar.set_accent(accent);
                    keyboard.set_accent(accent);
                    cursor = gpu.image(&raster::cursor(64, accent)).map_err(err)?;
                    for hand in Hand::BOTH {
                        vr.set_texture_d3d11(cursor_key(hand), raw(&cursor))?;
                    }
                    redraw_keyboard = true;
                    toolbar_dirty = true;
                    refresh_wrist = true;
                }
                Effect::AddWindow(h) => {
                    let id = crate::session::next_id(&engine.panels);
                    if let Some(p) = window_panel(id, h as isize, head_now, &screens) {
                        surfaces.insert(id, surface(&gpu, sharp, &p, &monitors)?);
                        engine.panels.push(p);
                    }
                }
                Effect::NextLanguage => {
                    dvr_input_win::next_layout();
                }
                Effect::Language(i) => {
                    dvr_input_win::select_layout(i);
                }
                Effect::NotifyFrom(from) => {
                    if from != NotifyFrom::Off && app_toasts.is_none() {
                        app_toasts = Some(AppToasts::start());
                    }
                }
                Effect::AutoLaunch(on) => {
                    if let Err(e) = vr.set_auto_launch(APP_KEY, on) {
                        tracing::warn!(%e, "auto-launch change failed");
                    }
                    if let Some(t) = &tray {
                        t.set_autostart(on);
                    }
                }
            }
        }

        if layout_checked.elapsed() >= LAYOUT_POLL {
            layout_checked = Instant::now();
            let id = dvr_input_win::layout_id();
            if id != layout {
                redraw_keyboard = true;
                session.language = dvr_input_win::layout_name();
                session.languages = dvr_input_win::layouts();
                menu_dirty = true;
                toolbar_dirty = true;
            }
            layout = id;
        }

        let size = session.toolbar_size(&engine.panels);
        if toolbar_dirty || size != toolbar_size {
            toolbar.set(&session.toolbar_model(&engine.panels, &strings));
            toolbar_for = session.toolbar_target();
            toolbar_size = size;
        }

        // Watch buttons show hide / keyboard state as soon as it changes.
        let keyboard_out = engine.panels.iter().any(|p| p.source == Source::Keyboard && p.visible);
        if (session.hidden, keyboard_out) != wrist_flags {
            wrist_flags = (session.hidden, keyboard_out);
            refresh_wrist = true;
            menu_dirty = true;
        }
        if stats_at.elapsed() >= STATS_PERIOD {
            stats_at = Instant::now();
            load = sampler.sample();
            refresh_wrist = true;
            if steamvr.as_ref().is_some_and(|w| !w.alive()) {
                tracing::info!("SteamVR server is gone; quitting");
                break 'frames;
            }
            let stats = vr.stats();
            for hand in Hand::BOTH {
                let b = stats.controllers[hand.index()];
                let low = !b.charging && b.level.is_some_and(|l| l < LOW_BATTERY);
                if low && !battery_low[hand.index()] && session.state.haptics {
                    vr.haptic(hand);
                }
                battery_low[hand.index()] = low;
            }
            let playing = media.info();
            let track = |m: &MediaInfo| (m.title.clone(), m.artist.clone());
            if session.state.media_to_chatbox
                && let Some(m) = playing.as_ref().filter(|m| m.playing)
                && now_playing.as_ref().map(track) != Some(track(m))
            {
                let name = now_playing_line(m);
                if let Err(e) = chatbox.send_quiet(&strings.get("chat-now-playing", &[("track", name)])) {
                    tracing::warn!(%e, "now-playing send failed");
                }
            }
            now_playing = playing;
        }
        if let Some(t) = notifications.as_mut().and_then(Listener::poll) {
            session.notified(&t.source_app, &t.title, &t.content);
            menu_dirty = true;
            banner.set(&t.source_app, &t.title, &t.content, &strings.get("wrist-now", &[]));
            toast = Some((t, Instant::now()));
            if session.state.haptics {
                vr.haptic(session.state.dominant.other());
            }
            refresh_wrist = true;
        }
        while let Some(t) = app_toasts.as_ref().and_then(AppToasts::poll) {
            if !session.state.notify_from.accepts(&t.app) {
                continue;
            }
            // Discord's first line already names the sender; other apps get their name in front.
            let title =
                if t.app.to_lowercase().contains("discord") { t.title } else { format!("{}: {}", t.app, t.title) };
            session.notified(&t.app, &title, &t.body);
            menu_dirty = true;
            if session.state.haptics {
                vr.haptic(session.state.dominant.other());
            }
            banner.set(&t.app, &title, &t.body, &strings.get("wrist-now", &[]));
            toast = Some((Toast { title, content: t.body, source_app: t.app, timeout_s: APP_TOAST_S }, Instant::now()));
            refresh_wrist = true;
        }
        let blocked =
            app_toasts.as_ref().is_some_and(|a| matches!(a.access(), ToastAccess::Denied | ToastAccess::Unavailable));
        if blocked != session.notify_blocked {
            session.notify_blocked = blocked;
            menu_dirty = true;
        }
        // Pointing at the banner keeps it up; after the laser leaves it stays a little longer.
        if events.iter().any(|e| matches!(e, Event::Hover { panel: BANNER_ID, .. }))
            && let Some((t, at)) = toast.as_mut()
        {
            let keep = Duration::from_secs_f32((t.timeout_s - BANNER_LINGER_S).max(0.0));
            *at = (*at).max(Instant::now().checked_sub(keep).unwrap_or_else(Instant::now));
        }
        if toast.as_ref().is_some_and(|(t, at)| at.elapsed().as_secs_f32() >= t.timeout_s) {
            toast = None;
            refresh_wrist = true;
        }
        if refresh_wrist {
            let info = wrist_info(
                &vr,
                &session.state,
                wrist_expanded,
                &load,
                timer.last_ms(),
                toast.as_ref(),
                now_playing.as_ref(),
            );
            let volume = media.volume().filter(|_| wrist_expanded);
            let unread = session.unread();
            let add = WristAdd {
                adding: wrist_adding,
                screen: Session::next_screen(&engine.panels, &screens),
                windows: if wrist_adding { wrist_windows(&session.windows, &engine.panels) } else { Vec::new() },
            };
            wrist.set(&WristInfo { hidden: session.hidden, keyboard: keyboard_out, unread, volume, add, ..info });
        }
        // Rebuilt only on change (the cost line updates with the stats tick), never per frame.
        if session.menu_open(&engine.panels) && (menu_dirty || refresh_wrist) {
            menu.set(&session.model(&engine.panels, &screens, &strings, (timer.last_ms(), load.own_cpu_percent)));
        }

        // The banner follows the notification on a critically damped spring: it rises and fades in, and goes
        // back down the same way. Without animations it simply appears and goes.
        let goal = f32::from(u8::from(toast.is_some()));
        banner_shown = if motion {
            let (x, v) = banner_shown;
            let omega = std::f32::consts::TAU / if goal > x { BANNER_IN_S } else { BANNER_OUT_S };
            let h = dt.min(0.05);
            let v = v + (omega * omega * (goal - x) - 2.0 * omega * v) * h;
            let x = (x + v * h).clamp(0.0, 1.0);
            if (goal - x).abs() < 0.002 && v.abs() < 0.01 { (goal, 0.0) } else { (x, v) }
        } else {
            (goal, 0.0)
        };
        if let Some(b) = engine.panels.iter_mut().find(|p| p.id == BANNER_ID) {
            *b = Panel::banner(b.aspect, banner_shown.0);
        }
        // All GPU work back to back inside the timer; one flush; then hand textures to SteamVR.
        timer.begin(&gpu);
        let upload = |px: &[u8], source: Source, width: u32, surfaces: &mut HashMap<u32, Surface>| {
            for p in engine.panels.iter().filter(|p| p.source == source) {
                if let Some(s) = surfaces.get_mut(&p.id) {
                    gpu.update(&s.tex, px, width);
                    s.needs_submit = true;
                }
            }
        };
        // The playing mark moves while the song plays and the watch is seen, at 45 steps a second: smooth for its
        // slow waves, and the watch is redrawn no more often than that.
        let seen = engine.panels.iter().any(|p| p.source == Source::Wrist && p.shown());
        let wave =
            (motion && seen && now_playing.as_ref().is_some_and(|m| m.playing)).then(|| start.elapsed().as_secs_f32());
        if wave.is_none() || beat_at.elapsed() >= BEAT_STEP {
            beat_at = Instant::now();
            wrist.beat(wave);
        }
        if let Some(px) = wrist.render() {
            upload(px, Source::Wrist, WRIST_SIZE.0, &mut surfaces);
        }
        if let Some(px) = menu.render() {
            upload(px, Source::Menu, MENU_SIZE.0, &mut surfaces);
        }
        if let Some(px) = toolbar.render() {
            upload(px, Source::Toolbar, TOOLBAR_SIZE.0, &mut surfaces);
        }
        // Keyboard view: rebuilt only when what it shows changes (labels, lit keys, hover, chat line).
        let latched = engine.keyboard().latched();
        let caps = dvr_input_win::caps_lock();
        let state = (
            key_hover,
            latched,
            caps,
            session.state.chatbox,
            chat.text().len(),
            layout,
            held.len(),
            held.first().copied(),
        );
        if redraw_keyboard || key_state != Some(state) {
            if redraw_keyboard
                || key_state.is_none_or(|k: (_, u8, bool, _, _, _, _, _)| (k.1 & 1) != (latched & 1) || k.2 != caps)
            {
                labels = layout_labels(latched & 1 != 0, caps);
            }
            let lit = |sc: u16| {
                held.contains(&sc)
                    || (sc == CHAT && session.state.chatbox)
                    || (sc == CAPS && caps)
                    || (matches!(sc, LSHIFT | RSHIFT) && latched & 1 != 0)
                    || (sc == LCTRL && latched & 2 != 0)
                    || (sc == LALT && latched & 4 != 0)
                    || (sc == LWIN && latched & 8 != 0)
            };
            let chat_line = session.state.chatbox.then_some(chat.text());
            key_faces(&mut faces, &labels, chat_line, lit, &key_hover);
            keyboard.set(&faces);
            key_state = Some(state);
        }
        if let Some(px) = keyboard.render() {
            upload(px, Source::Keyboard, KEYBOARD_SIZE.0, &mut surfaces);
        }
        if let Some(px) = banner.render() {
            upload(px, Source::Banner, BANNER_SIZE.0, &mut surfaces);
        }
        for p in &engine.panels {
            let world = p.world_pose(&poses);
            // Leaning into a screen fades it out rather than letting the near clip slice it.
            let fade = p.near_fade(world, head_now.pos);
            vr.place_panel(&Panel { opacity: p.opacity * fade, ..*p }, world);
            let Some(s) = surfaces.get_mut(&p.id) else { continue };
            // Hidden or faded-out panels stop capturing (no GPU or CPU cost while hidden).
            if p.shown() && s.capture.is_none() && !s.dead {
                s.capture = match p.source {
                    Source::Monitor(i) => {
                        monitors.get(i as usize).and_then(|m| Capture::monitor(&gpu.device, m.handle).ok())
                    }
                    Source::Window(h) => Capture::window(&gpu.device, h as isize).ok(),
                    _ => None,
                };
            }
            if !p.shown() {
                s.capture = None;
            }
            let target = s.detail.as_ref().map_or(&s.tex, |d| &d.src);
            let mut draw = false;
            match s.capture.as_mut().map(|c| c.copy_latest(&gpu.context, target)) {
                Some(Ok(Frame::Copied)) => {
                    if let (Some(d), Some(sh)) = (&s.detail, &sharpen) {
                        sh.new_frame(&gpu, d);
                        draw = true;
                    }
                    s.needs_submit = true;
                }
                Some(Ok(Frame::Resized(w, h))) => {
                    if let Some(d) = &mut s.detail {
                        *d = Detail::new(&gpu, w, h, d.level).map_err(err)?;
                        s.tex = d.out.clone();
                    } else {
                        s.tex = gpu.texture(w, h, None).map_err(err)?;
                    }
                    resized.push((p.id, w, h));
                }
                _ => {}
            }
            // The level its size in view needs: finer at once, coarser once it has stayed wanted a moment.
            if let (Some(d), Some(sh), Some(ppr)) = (&mut s.detail, &sharpen, ppr)
                && p.shown()
            {
                let distance = (world.pos - head_now.pos).length().max(0.1);
                let want = level_for(d.w, p.width_m / distance * ppr);
                let switch = match s.coarser {
                    _ if want < d.level => true,
                    Some((l, since)) if want > d.level && l == want => since.elapsed() >= COARSER_AFTER,
                    _ => {
                        s.coarser = (want > d.level).then(|| (want, Instant::now()));
                        false
                    }
                };
                if switch {
                    s.coarser = None;
                    d.set_level(&gpu, want).map_err(err)?;
                    s.tex = d.out.clone();
                    draw = true;
                    s.needs_submit = true;
                }
                if draw {
                    sh.draw(&gpu, d);
                }
            }
        }
        timer.end(&gpu);
        gpu.flush();
        for (id, w, h) in resized.drain(..) {
            if let Some(p) = engine.panels.iter_mut().find(|p| p.id == id) {
                p.aspect = h as f32 / w as f32;
                dirty = Some(Instant::now());
            }
        }
        for p in &engine.panels {
            if let Some(s) = surfaces.get_mut(&p.id)
                && std::mem::take(&mut s.needs_submit)
                && let Err(e) = vr.set_texture_d3d11(u64::from(p.id), raw(&s.tex))
            {
                tracing::warn!(%e, panel = p.id, "panel texture rejected");
            }
        }

        for hand in Hand::BOTH {
            let at = cursors[hand.index()].and_then(|(id, uv)| {
                let p = engine.panels.iter().find(|p| p.id == id)?;
                let world = p.world_pose(&poses);
                Some(Pose::new(world.transform_point(p.local_point(uv) + Vec3::Z * CURSOR_LIFT_M), world.rot))
            });
            vr.place_cursor(hand, at);
        }

        if dirty.is_some_and(|t| t.elapsed() >= SAVE_DELAY) {
            session.persist(&engine.panels, worn_head);
            session.state.save(&state_path).map_err(err)?;
            dirty = None;
        }
        vr.wait_frame(20);
    }
    dvr_sys::exit_watchdog(EXIT_GRACE);
    session.persist(&engine.panels, worn_head);
    session.state.save(&state_path).map_err(err)
}
