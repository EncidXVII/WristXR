//! The VR session on Windows: OpenVR overlays, WGC capture, SendInput.
use dvr_capture_win::{Capture, Monitor};
use dvr_core::backend::VrBackend;
use dvr_core::chat::{ChatAction, ChatLine};
use dvr_core::engine::{Engine, Event, FrameInput};
use dvr_core::keyboard::{CHAT, KeyDef, SPACE};
use dvr_core::math::{Pose, Quat, Vec3};
use dvr_core::panel::{Hand, Panel, PixelRect, Source};
use dvr_core::raster;
use dvr_core::state::State;
use dvr_gfx_d3d11::{Gpu, raw};
use dvr_input_win::Injector;
use dvr_osc::{Chatbox, VRCHAT_ADDR};
use dvr_vr::{OpenVr, cursor_key};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;

const SAVE_DELAY: Duration = Duration::from_secs(1);
const CURSOR_LIFT_M: f32 = 0.003;
const STANDING_HEAD: Pose = Pose::new(Vec3::new(0.0, 1.6, 0.0), Quat::IDENTITY);
const LAYOUT_POLL: Duration = Duration::from_millis(500);
/// Characters of the chat line that fit on the space bar.
const SPACE_BAR_CHARS: usize = 28;

/// GPU side of a panel: its texture and, for visible monitor panels, a live capture.
struct Surface {
    tex: ID3D11Texture2D,
    capture: Option<Capture>,
    rect: Option<PixelRect>,
    needs_submit: bool,
}

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
    let (img, rect) = match panel.source {
        Source::Keyboard => (keyboard_image(chat), None),
        Source::Monitor(i) => match monitors.get(i as usize) {
            Some(m) => {
                return Ok(Surface {
                    tex: gpu.texture(m.rect.w, m.rect.h, None).map_err(err)?,
                    capture: None,
                    rect: Some(m.rect),
                    needs_submit: false,
                });
            }
            None => (raster::placeholder(1280, 720, "MONITOR NOT CONNECTED"), None),
        },
        Source::Window(_) => (raster::placeholder(1280, 720, "WINDOW CLOSED"), None),
    };
    Ok(Surface { tex: gpu.image(&img).map_err(err)?, capture: None, rect, needs_submit: true })
}

/// First run: primary monitor in front, keyboard below it.
fn first_layout(state: &mut State, head: Pose, monitors: &[Monitor]) {
    let primary = monitors.iter().position(|m| m.primary).unwrap_or(0);
    let aspect = monitors.get(primary).map_or(9.0 / 16.0, |m| m.rect.h as f32 / m.rect.w as f32);
    state.panels =
        vec![Panel::in_front_of(1, Source::Monitor(primary as u32), head, aspect), Panel::keyboard_below(2, head)];
}

pub fn run() -> Result<(), String> {
    dvr_capture_win::init_process();
    let state_path = State::default_path().ok_or("no config directory")?;
    let dir = state_path.parent().ok_or("no config directory")?.to_path_buf();
    let manifest = crate::bindings::install(&dir.join("input")).map_err(err)?;
    let mut state = State::load(&state_path);

    let mut vr = OpenVr::init(&manifest)?;
    let gpu = Gpu::new(vr.adapter_index()).map_err(err)?;
    let monitors = dvr_capture_win::monitors();
    if state.panels.is_empty() {
        first_layout(&mut state, vr.head_pose().unwrap_or(STANDING_HEAD), &monitors);
        state.save(&state_path).map_err(err)?;
    }

    let cursor = gpu.image(&raster::cursor(64)).map_err(err)?;
    for hand in Hand::BOTH {
        vr.set_texture_d3d11(cursor_key(hand), raw(&cursor))?;
    }
    let mut chat = ChatLine::default();
    let chat_text = |on: bool, line: &ChatLine| on.then(|| line.text().to_owned());
    let mut surfaces = HashMap::new();
    for p in &state.panels {
        surfaces.insert(p.id, surface(&gpu, p, &monitors, chat_text(state.chatbox, &chat).as_deref())?);
    }

    let chatbox = Chatbox::new(VRCHAT_ADDR.parse().map_err(err)?).map_err(err)?;
    let mut layout = dvr_input_win::layout_id();
    let mut layout_checked = Instant::now();
    let mut engine = Engine::new(state.panels.clone());
    let mut injector = Injector::new();
    let mut input = FrameInput::default();
    let mut events = Vec::with_capacity(64);
    let mut dirty: Option<Instant> = None;
    let start = Instant::now();

    while vr.poll(start.elapsed().as_millis() as u64, &mut input) {
        events.clear();
        engine.step(&input, &mut events);
        let poses = [input.hands[0].pose, input.hands[1].pose];
        let mut cursors: [Option<(u32, [f32; 2])>; 2] = [None; 2];
        let mut redraw_keyboard = false;
        for e in &events {
            let rect = |panel: u32| surfaces.get(&panel).and_then(|s: &Surface| s.rect);
            if let Event::Hover { hand, panel, uv, .. } = *e {
                cursors[hand.index()] = Some((panel, uv));
            }
            match *e {
                Event::Key { scancode, down } if state.chatbox => {
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
                    state.chatbox = !state.chatbox;
                    if !state.chatbox {
                        let _ = chatbox.typing(false);
                    }
                    redraw_keyboard = true;
                    dirty = Some(Instant::now());
                }
                Event::Haptic { hand } => vr.haptic(hand),
                Event::Moved { .. } => dirty = Some(Instant::now()),
                Event::MenuToggle => {
                    for p in engine.panels.iter_mut().filter(|p| p.source == Source::Keyboard) {
                        p.visible = !p.visible;
                    }
                    dirty = Some(Instant::now());
                }
            }
        }

        if layout_checked.elapsed() >= LAYOUT_POLL {
            layout_checked = Instant::now();
            let id = dvr_input_win::layout_id();
            redraw_keyboard |= id != layout;
            layout = id;
        }
        if redraw_keyboard {
            let img = keyboard_image(chat_text(state.chatbox, &chat).as_deref());
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
            if let (Source::Monitor(i), true, None) = (p.source, p.visible, &s.capture) {
                s.capture = monitors.get(i as usize).and_then(|m| Capture::monitor(&gpu.device, m.handle).ok());
            }
            if !p.visible {
                s.capture = None;
            }
            let fresh = match &s.capture {
                Some(c) => c.copy_latest(&gpu.context, &s.tex).unwrap_or(false),
                None => std::mem::take(&mut s.needs_submit),
            };
            if fresh {
                gpu.flush();
                if let Err(e) = vr.set_texture_d3d11(u64::from(p.id), raw(&s.tex)) {
                    tracing::warn!(%e, panel = p.id, "panel texture rejected");
                }
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
            state.panels.clone_from(&engine.panels);
            state.save(&state_path).map_err(err)?;
            dirty = None;
        }
        vr.wait_frame(20);
    }
    state.panels.clone_from(&engine.panels);
    state.save(&state_path).map_err(err)
}
