//! The VR session on Windows: OpenVR overlays, WGC capture, SendInput, Slint menu and wrist.
use crate::bindings::APP_KEY;
use crate::session::{Effect, MonitorInfo, Session};
use crate::tray::{Tray, TrayCommand};
use dvr_capture_win::{Capture, Frame, Monitor};
use dvr_core::backend::VrBackend;
use dvr_core::chat::{ChatAction, ChatLine};
use dvr_core::engine::{Engine, Event, FrameInput, MouseButton};
use dvr_core::keyboard::{CAPS, CHAT, LALT, LCTRL, LSHIFT, ROWS, RSHIFT, SPACE};
use dvr_core::math::{Pose, Quat, Vec3};
use dvr_core::panel::{Hand, MIN_WIDTH_M, Panel, PixelRect, Source};
use dvr_core::raster;
use dvr_core::state::State;
use dvr_gfx_d3d11::{Gpu, GpuTimer, raw};
use dvr_input_win::Injector;
use dvr_notify_api::{Listener, Toast};
use dvr_osc::{Chatbox, VRCHAT_ADDR};
use dvr_sys::{Media, MediaCommand, MediaInfo};
use dvr_ui::{
    KEYBOARD_SIZE, KeyFace, Keyboard, LOW_BATTERY, MENU_SIZE, Menu, Strings, WRIST_SIZE, Wrist, WristInfo, WristInput,
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
/// A popped-out window appears this far in front of the hand that pulled it.
const POP_OUT_DISTANCE_M: f32 = 0.3;
/// Characters of the chat line that fit on the space bar.
const SPACE_BAR_CHARS: usize = 28;

/// GPU side of a panel: its texture and, for visible monitor panels, a live capture.
struct Surface {
    tex: ID3D11Texture2D,
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

/// Key labels in the active OS layout, in the case they will type (Shift latched / Caps Lock).
fn layout_labels(shift: bool, caps: bool) -> Vec<String> {
    ROWS.iter()
        .flat_map(|r| r.iter())
        .map(|key| {
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

fn surface(gpu: &Gpu, panel: &Panel, monitors: &[Monitor]) -> Result<Surface, String> {
    let blank = |size: (u32, u32)| -> Result<Surface, String> {
        let tex = gpu.texture(size.0, size.1, None).map_err(err)?;
        Ok(Surface { tex, capture: None, rect: None, needs_submit: false, dead: false })
    };
    let (img, rect) = match panel.source {
        Source::Keyboard => return blank(KEYBOARD_SIZE),
        Source::Monitor(i) => match monitors.get(i as usize) {
            Some(m) => return Ok(Surface { rect: Some(m.rect), ..blank((m.rect.w, m.rect.h))? }),
            None => (raster::placeholder(1280, 720, "MONITOR NOT CONNECTED"), None),
        },
        Source::Window(h) => match dvr_capture_win::window_rect(h as isize) {
            Some(r) => return Ok(Surface { rect: Some(r), ..blank((r.w, r.h))? }),
            None => {
                let img = raster::placeholder(1280, 720, WINDOW_CLOSED);
                let tex = gpu.image(&img).map_err(err)?;
                return Ok(Surface { tex, capture: None, rect: None, needs_submit: true, dead: true });
            }
        },
        Source::Wrist => return blank(WRIST_SIZE),
        Source::Menu => return blank(MENU_SIZE),
    };
    Ok(Surface { tex: gpu.image(&img).map_err(err)?, capture: None, rect, needs_submit: true, dead: false })
}

fn wrist_info(
    vr: &OpenVr,
    load: &dvr_sys::Load,
    own_gpu_ms: f32,
    toast: Option<&(Toast, Instant)>,
    media: Option<&MediaInfo>,
) -> WristInfo {
    let (hour, minute) = dvr_sys::local_time();
    let stats = vr.stats();
    WristInfo {
        hour,
        minute,
        game_fps: stats.game_fps,
        controllers: stats.controllers,
        lowest_tracker: stats.lowest_tracker,
        cpu: load.cpu,
        ram: load.ram,
        gpu: load.gpu,
        own_gpu_ms,
        own_cpu_percent: load.own_cpu_percent,
        toast: toast.map(|(t, _)| (t.title.clone(), t.content.clone())),
        media: media.map(|m| (m.title.clone(), m.artist.clone(), m.playing)),
    }
}

pub fn run() -> Result<(), String> {
    let state_path = State::default_path().ok_or("no config directory")?;
    let dir = state_path.parent().ok_or("no config directory")?.to_path_buf();
    let manifest = crate::bindings::install(&dir.join("input")).map_err(err)?;

    let mut vr = OpenVr::init(&manifest)?;
    let gpu = Gpu::new(vr.adapter_index()).map_err(err)?;
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

    // SteamVR knows us as an app (dashboard overlay) so it can start us with itself; first run opts in.
    let app = std::env::current_exe()
        .map_err(err)
        .and_then(|exe| crate::bindings::install_vrmanifest(&dir, &exe).map_err(err))
        .and_then(|m| vr.register_app(&m, APP_KEY));
    match app {
        Ok(()) if !session.state.onboarded => {
            if let Err(e) = vr.set_auto_launch(APP_KEY, true) {
                tracing::warn!(%e, "auto-launch not set");
            }
        }
        Ok(()) => {}
        Err(e) => tracing::warn!(%e, "SteamVR app registration failed"),
    }
    session.persist(&panels);
    session.state.save(&state_path).map_err(err)?;

    let cursor = gpu.image(&raster::cursor(64)).map_err(err)?;
    for hand in Hand::BOTH {
        vr.set_texture_d3d11(cursor_key(hand), raw(&cursor))?;
    }
    let mut chat = ChatLine::default();
    let mut surfaces = HashMap::new();
    for p in &panels {
        surfaces.insert(p.id, surface(&gpu, p, &monitors)?);
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
    let mut events = Vec::with_capacity(64);
    let mut dirty: Option<Instant> = None;
    let mut resized: Vec<(u32, u32, u32)> = Vec::with_capacity(8);
    let mut wrist = Wrist::new()?;
    let media = Media::start();
    let mut now_playing: Option<MediaInfo> = None;
    let mut wrist_hovered = false;
    let mut menu = Menu::new()?;
    let mut sampler = dvr_sys::Sampler::new();
    let mut load = dvr_sys::Load::default();
    let mut timer = GpuTimer::new(&gpu).map_err(err)?;
    // Another app (e.g. XSOverlay) may own the port; the overlay still runs without toasts.
    let mut notifications =
        Listener::bind(dvr_notify_api::PORT).inspect_err(|e| tracing::warn!(%e, "notification API unavailable")).ok();
    session.notifications_ok = notifications.is_some();
    let mut toast: Option<(Toast, Instant)> = None;
    let mut keyboard = Keyboard::new()?;
    let mut faces: Vec<KeyFace> = Vec::with_capacity(80);
    let mut held: Vec<u16> = Vec::with_capacity(8);
    let mut labels = layout_labels(false, dvr_input_win::caps_lock());
    let mut key_state = None;
    let mut battery_low = [false; 2];
    let mut stats_at = Instant::now() - STATS_PERIOD;
    let start = Instant::now();

    let updates = crate::update::check_in_background();
    let mut release = None;
    'frames: while vr.poll(start.elapsed().as_millis() as u64, &mut input) {
        dvr_sys::pump_messages();
        if let Ok(r) = updates.try_recv() {
            if let Some(t) = tray.as_mut() {
                t.offer_update(&strings, &r.version);
            }
            release = Some(r);
        }
        let mut tray_open_menu = false;
        while let Some(cmd) = tray.as_ref().and_then(Tray::poll) {
            match cmd {
                TrayCommand::OpenMenu => tray_open_menu = !session.menu_open(&engine.panels),
                TrayCommand::AutoLaunch(on) => {
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
        events.clear();
        engine.step(&input, &mut events);
        vr.set_exclusive(Hand::BOTH.map(|h| engine.engaged(h)));
        let poses = [input.hands[0].pose, input.hands[1].pose];
        let mut cursors: [Option<(u32, [f32; 2])>; 2] = [None; 2];
        let mut redraw_keyboard = false;
        let mut key_hover: [Option<u16>; 2] = [None; 2];
        // Popped-out windows can move on the desktop; input maps through their current rectangle.
        for p in &engine.panels {
            if let (Source::Window(h), Some(s)) = (p.source, surfaces.get_mut(&p.id))
                && !s.dead
            {
                s.rect = dvr_capture_win::window_rect(h as isize);
                if s.rect.is_none() {
                    s.dead = true;
                    s.capture = None;
                    s.tex = gpu.image(&raster::placeholder(1280, 720, WINDOW_CLOSED)).map_err(err)?;
                    s.needs_submit = true;
                }
            }
        }
        let mut pop = None;
        let selected = session.selected;
        session.pointer(&events, &menu);
        // The other hand's laser on the wrist drives its media buttons.
        let on_wrist = |id: u32| engine.panels.iter().any(|p| p.id == id && p.source == Source::Wrist);
        let mut over_wrist = false;
        for e in &events {
            match *e {
                Event::Hover { panel, uv, .. } if on_wrist(panel) => {
                    over_wrist = true;
                    wrist.pointer_move(uv);
                }
                Event::Mouse { panel, uv, button: MouseButton::Left, down } if on_wrist(panel) => {
                    over_wrist = true;
                    wrist.pointer_button(uv, down);
                }
                _ => {}
            }
        }
        if wrist_hovered && !over_wrist {
            wrist.pointer_leave();
        }
        wrist_hovered = over_wrist;
        for input in wrist.take_inputs() {
            media.send(match input {
                WristInput::Previous => MediaCommand::Previous,
                WristInput::PlayPause => MediaCommand::PlayPause,
                WristInput::Next => MediaCommand::Next,
            });
        }
        let mut menu_dirty = tray_open_menu || session.selected != selected;
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
                Event::Haptic { hand } => vr.haptic(hand),
                Event::Moved { .. } => dirty = Some(Instant::now()),
                Event::MenuToggle => {
                    session.toggle_menu(&mut engine.panels, head(&vr));
                    menu_dirty = true;
                }
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
            surfaces.insert(id, surface(&gpu, &p, &monitors)?);
            engine.panels.push(p);
            engine.start_grab(hand, id, &poses);
            session.selected = Some(id);
            menu_dirty = true;
            dirty = Some(Instant::now());
        }

        for click in menu.take_inputs() {
            for fx in session.input(click, &mut engine.panels, head(&vr), &poses, &screens) {
                match fx {
                    Effect::Added(id) => {
                        if let Some(p) = engine.panels.iter().find(|p| p.id == id) {
                            surfaces.insert(id, surface(&gpu, p, &monitors)?);
                        }
                    }
                    Effect::Removed(id) => {
                        surfaces.remove(&id);
                        vr.remove_panel(id);
                    }
                    Effect::RedrawKeyboard => redraw_keyboard = true,
                    Effect::Quit => break 'frames,
                }
            }
            dirty = Some(Instant::now());
            menu_dirty = true;
        }

        if layout_checked.elapsed() >= LAYOUT_POLL {
            layout_checked = Instant::now();
            let id = dvr_input_win::layout_id();
            redraw_keyboard |= id != layout;
            layout = id;
        }

        let mut refresh_wrist = false;
        if stats_at.elapsed() >= STATS_PERIOD {
            stats_at = Instant::now();
            load = sampler.sample();
            refresh_wrist = true;
            let stats = vr.stats();
            for hand in Hand::BOTH {
                let low = stats.controllers[hand.index()].is_some_and(|b| b < LOW_BATTERY);
                if low && !battery_low[hand.index()] {
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
                let name = if m.artist.is_empty() { m.title.clone() } else { format!("{} – {}", m.title, m.artist) };
                if let Err(e) = chatbox.send_quiet(&strings.get("chat-now-playing", &[("track", name)])) {
                    tracing::warn!(%e, "now-playing send failed");
                }
            }
            now_playing = playing;
        }
        if let Some(t) = notifications.as_mut().and_then(Listener::poll) {
            toast = Some((t, Instant::now()));
            vr.haptic(session.state.dominant.other());
            refresh_wrist = true;
        }
        if toast.as_ref().is_some_and(|(t, at)| at.elapsed().as_secs_f32() >= t.timeout_s) {
            toast = None;
            refresh_wrist = true;
        }
        if refresh_wrist {
            wrist.set(&wrist_info(&vr, &load, timer.last_ms(), toast.as_ref(), now_playing.as_ref()));
        }
        // Rebuilt only on change (the cost line updates with the stats tick), never per frame.
        if session.menu_open(&engine.panels) && (menu_dirty || refresh_wrist) {
            menu.set(&session.model(&engine.panels, &screens, &strings, (timer.last_ms(), load.own_cpu_percent)));
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
        if let Some(px) = wrist.render() {
            upload(px, Source::Wrist, WRIST_SIZE.0, &mut surfaces);
        }
        if let Some(px) = menu.render() {
            upload(px, Source::Menu, MENU_SIZE.0, &mut surfaces);
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
            };
            let chat_line = session.state.chatbox.then_some(chat.text());
            key_faces(&mut faces, &labels, chat_line, lit, &key_hover);
            keyboard.set(&faces);
            key_state = Some(state);
        }
        if let Some(px) = keyboard.render() {
            upload(px, Source::Keyboard, KEYBOARD_SIZE.0, &mut surfaces);
        }
        for p in &engine.panels {
            vr.place_panel(p, p.world_pose(&poses));
            let Some(s) = surfaces.get_mut(&p.id) else { continue };
            if p.visible && s.capture.is_none() && !s.dead {
                s.capture = match p.source {
                    Source::Monitor(i) => {
                        monitors.get(i as usize).and_then(|m| Capture::monitor(&gpu.device, m.handle).ok())
                    }
                    Source::Window(h) => Capture::window(&gpu.device, h as isize).ok(),
                    _ => None,
                };
            }
            if !p.visible {
                s.capture = None;
            }
            match s.capture.as_mut().map(|c| c.copy_latest(&gpu.context, &s.tex)) {
                Some(Ok(Frame::Copied)) => s.needs_submit = true,
                Some(Ok(Frame::Resized(w, h))) => {
                    s.tex = gpu.texture(w, h, None).map_err(err)?;
                    resized.push((p.id, w, h));
                }
                _ => {}
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
            session.persist(&engine.panels);
            session.state.save(&state_path).map_err(err)?;
            dirty = None;
        }
        vr.wait_frame(20);
    }
    session.persist(&engine.panels);
    session.state.save(&state_path).map_err(err)
}
