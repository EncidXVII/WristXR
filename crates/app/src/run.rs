//! The VR session on Windows: OpenVR overlays, WGC capture, SendInput, Slint menu and wrist.
use crate::session::{Effect, MonitorInfo, Session};
use dvr_capture_win::{Capture, Frame, Monitor};
use dvr_core::backend::VrBackend;
use dvr_core::chat::{ChatAction, ChatLine};
use dvr_core::engine::{Engine, Event, FrameInput};
use dvr_core::keyboard::{CHAT, KeyDef, SPACE};
use dvr_core::math::{Pose, Quat, Vec3};
use dvr_core::panel::{Hand, MIN_WIDTH_M, Panel, PixelRect, Source};
use dvr_core::raster;
use dvr_core::state::State;
use dvr_gfx_d3d11::{Gpu, GpuTimer, raw};
use dvr_input_win::Injector;
use dvr_notify_api::{Listener, Toast};
use dvr_osc::{Chatbox, VRCHAT_ADDR};
use dvr_ui::{MENU_SIZE, Menu, Strings, WRIST_SIZE, Wrist, WristInfo};
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

/// Keys show the active OS layout; in chat mode the Chat key is lit and the space bar shows the line being typed.
fn keyboard_image(chat: Option<&str>) -> raster::Image {
    let label = |key: &KeyDef| {
        if let (SPACE, Some(text)) = (key.sc, chat) {
            let shown: String = text.chars().filter(|c| raster::drawable(&c.to_string())).collect();
            let skip = shown.chars().count().saturating_sub(SPACE_BAR_CHARS);
            return shown.chars().skip(skip).collect();
        }
        if key.label.chars().count() == 1
            && let Some(l) = dvr_input_win::key_label(key.sc).filter(|l| raster::drawable(l))
        {
            return l;
        }
        key.label.to_owned()
    };
    raster::keyboard(label, if chat.is_some() { &[CHAT] } else { &[] })
}

fn surface(gpu: &Gpu, panel: &Panel, monitors: &[Monitor], chat: Option<&str>) -> Result<Surface, String> {
    let blank = |size: (u32, u32)| -> Result<Surface, String> {
        let tex = gpu.texture(size.0, size.1, None).map_err(err)?;
        Ok(Surface { tex, capture: None, rect: None, needs_submit: false, dead: false })
    };
    let (img, rect) = match panel.source {
        Source::Keyboard => (keyboard_image(chat), None),
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

fn wrist_info(vr: &OpenVr, load: &dvr_sys::Load, own_gpu_ms: f32, toast: Option<&(Toast, Instant)>) -> WristInfo {
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
    session.persist(&panels);
    session.state.save(&state_path).map_err(err)?;

    let cursor = gpu.image(&raster::cursor(64)).map_err(err)?;
    for hand in Hand::BOTH {
        vr.set_texture_d3d11(cursor_key(hand), raw(&cursor))?;
    }
    let mut chat = ChatLine::default();
    let chat_text = |on: bool, line: &ChatLine| on.then(|| line.text().to_owned());
    let mut surfaces = HashMap::new();
    for p in &panels {
        surfaces.insert(p.id, surface(&gpu, p, &monitors, chat_text(session.state.chatbox, &chat).as_deref())?);
    }

    let strings = Strings::english()?;
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
    let mut menu = Menu::new()?;
    let mut sampler = dvr_sys::Sampler::new();
    let mut load = dvr_sys::Load::default();
    let mut timer = GpuTimer::new(&gpu).map_err(err)?;
    // Another app (e.g. XSOverlay) may own the port; the overlay still runs without toasts.
    let mut notifications =
        Listener::bind(dvr_notify_api::PORT).inspect_err(|e| tracing::warn!(%e, "notification API unavailable")).ok();
    let mut toast: Option<(Toast, Instant)> = None;
    let mut stats_at = Instant::now() - STATS_PERIOD;
    let start = Instant::now();

    'frames: while vr.poll(start.elapsed().as_millis() as u64, &mut input) {
        events.clear();
        engine.step(&input, &mut events);
        let poses = [input.hands[0].pose, input.hands[1].pose];
        let mut cursors: [Option<(u32, [f32; 2])>; 2] = [None; 2];
        let mut redraw_keyboard = false;
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
        let mut menu_dirty = session.selected != selected;
        for e in &events {
            let rect = |panel: u32| surfaces.get(&panel).and_then(|s: &Surface| s.rect);
            if let Event::Hover { hand, panel, uv, .. } = *e {
                cursors[hand.index()] = Some((panel, uv));
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
            surfaces.insert(id, surface(&gpu, &p, &monitors, None)?);
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
                            surfaces.insert(id, surface(&gpu, p, &monitors, None)?);
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
            wrist.set(&wrist_info(&vr, &load, timer.last_ms(), toast.as_ref()));
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
        if redraw_keyboard {
            let img = keyboard_image(chat_text(session.state.chatbox, &chat).as_deref());
            for p in engine.panels.iter().filter(|p| p.source == Source::Keyboard) {
                if let Some(s) = surfaces.get_mut(&p.id) {
                    s.tex = gpu.image(&img).map_err(err)?;
                    s.needs_submit = true;
                }
            }
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
