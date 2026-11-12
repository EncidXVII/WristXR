//! Per-frame interaction logic: laser pointer, keyboard typing, grab/scale.
//! Rules: hover never clicks, trigger clicks, A/X right-clicks, grip only grabs, locked panels ignore grip.
//! A held panel keeps the spot you grabbed under the laser and turns to face you, level; it never copies the
//! twist of the wrist. Let go of it next to your other hand and it stays on that hand.
//! `step` does not allocate as long as `out` has spare capacity (reserve ~64).
use crate::keyboard::{self, Keyboard};
use crate::math::{Pose, Quat, Vec3};
use crate::panel::{
    ATTACH_REACH_M, Anchor, Anchors, HEAD, Hand, MAX_WIDTH_M, MIN_WIDTH_M, Panel, Source, level_facing,
};

/// Ray movement (uv) that turns a press into a drag.
pub const DRAG_UV: f32 = 0.01;
/// A second click this close in time and space reuses the first point (laser jitter).
pub const DOUBLE_CLICK_UV: f32 = 0.02;
pub const DOUBLE_CLICK_MS: u64 = 400;
pub const LONG_PRESS_MS: u64 = 600;
pub const MENU_DOUBLE_TAP_MS: u64 = 400;
pub const KEY_REPEAT_DELAY_MS: u64 = 500;
pub const KEY_REPEAT_INTERVAL_MS: u64 = 33;
pub const STICK_DEADZONE: f32 = 0.2;
pub const SCROLL_NOTCHES_PER_S: f32 = 12.0;
/// Drag to scroll: one wheel notch per this much of the panel's height the laser moves.
pub const DRAG_SCROLL_UV: f32 = 0.03;
pub const PUSH_PULL_M_PER_S: f32 = 1.5;
/// Grip + stick left/right resizes the held panel, at full tilt by this share of its width per second.
pub const RESIZE_PER_S: f32 = 0.8;
/// A grabbed panel turns to face you over about this long instead of snapping.
const FACE_EASE_S: f32 = 0.08;
const MIN_GRAB_DISTANCE_M: f32 = 0.1;
/// Laser smoothing: hand turns slower than this per frame are smoothed (tremor, tracking noise); faster ones
/// pass straight through, so aiming never lags.
pub const SMOOTH_FULL_RAD: f32 = 0.02;
/// Smoothing weight for the steadiest hand (share of the new ray taken each frame).
pub const SMOOTH_MIN: f32 = 0.25;
/// Window pop-out: grip a monitor panel and yank toward yourself this far, this straight, this fast.
pub const POP_OUT_PULL_M: f32 = 0.2;
pub const POP_OUT_LATERAL_M: f32 = 0.1;
pub const POP_OUT_WINDOW_MS: u64 = 800;

#[derive(Clone, Copy, Debug, Default)]
pub struct HandInput {
    /// Pointer pose; the laser runs along its forward (-Z).
    pub pose: Pose,
    pub tracked: bool,
    pub trigger: bool,
    pub grip: bool,
    pub menu: bool,
    /// A/X (the lower half of the trackpad on wands): right click where the laser points.
    pub right_click: bool,
    pub stick: [f32; 2],
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameInput {
    pub now_ms: u64,
    /// Indexed by `Hand::index()`.
    pub hands: [HandInput; 2],
    /// Headset pose (for head-locked panels).
    pub head: Pose,
}

impl FrameInput {
    /// Left hand, right hand, head.
    pub fn anchors(&self) -> Anchors {
        [self.hands[0].pose, self.hands[1].pose, self.head]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// Laser over a panel. `cursor`: this hand owns the OS cursor and should move it.
    Hover {
        hand: Hand,
        panel: u32,
        uv: [f32; 2],
        cursor: bool,
    },
    Mouse {
        panel: u32,
        uv: [f32; 2],
        button: MouseButton,
        down: bool,
    },
    Scroll {
        panel: u32,
        uv: [f32; 2],
        notches: i32,
    },
    Key {
        scancode: u16,
        down: bool,
    },
    Moved {
        panel: u32,
    },
    Haptic {
        hand: Hand,
    },
    /// Grip-and-yank on a monitor panel: the window under `uv` should become its own panel. The monitor panel is
    /// already back where it was; the app creates the window panel and calls `start_grab` to keep it in hand.
    PopOut {
        hand: Hand,
        panel: u32,
        uv: [f32; 2],
    },
    /// The keyboard's Chat key: toggle "send to VRChat chatbox".
    ChatToggle,
    /// Left menu button double-tapped: hide or show screens, windows and keyboard (as in XSOverlay).
    HideToggle,
    /// The keyboard's language key: switch to the next input language.
    NextLanguage,
}

#[derive(Clone, Copy, Debug, Default)]
enum Press {
    #[default]
    Idle,
    Pending {
        panel: u32,
        uv: [f32; 2],
        t0: u64,
        /// Moving it scrolls instead of dragging (drag to scroll, on a screen or window).
        scrolls: bool,
    },
    /// Drag to scroll: the page follows the laser, the cursor stays where the press began.
    Scrolling {
        panel: u32,
        at: [f32; 2],
        v: f32,
        acc: f32,
    },
    Dragging {
        panel: u32,
        uv: [f32; 2],
    },
    /// Long press fired a right click; wait for release.
    Consumed,
    Key {
        sc: u16,
        mods: u8,
        next_repeat: u64,
    },
}

#[derive(Clone, Copy, Debug, Default)]
struct HandState {
    prev: HandInput,
    press: Press,
    last_click: Option<(u32, [f32; 2], u64)>,
    scroll: f32,
    menu_tap: Option<u64>,
    /// Laser hit a panel this frame.
    on_panel: bool,
    /// Right button held on this panel, at this point.
    right: Option<(u32, [f32; 2])>,
    /// Smoothed laser ray.
    ray: Option<Pose>,
}

#[derive(Clone, Copy, Debug, Default)]
enum Grab {
    #[default]
    Idle,
    One {
        hand: Hand,
        panel: u32,
        /// The grabbed spot on the panel, which stays under the laser.
        uv: [f32; 2],
        /// How far along the laser that spot is (the stick pushes and pulls it).
        reach: f32,
        /// The panel is next to the other hand (letting go attaches it there).
        near: bool,
        popout: Option<PopOutWatch>,
    },
    Two {
        panel: u32,
        mid0: Vec3,
        len0: f32,
        pos0: Vec3,
        width0: f32,
    },
}

/// Watches the first moments of a monitor-panel grab for the pop-out yank.
#[derive(Clone, Copy, Debug)]
struct PopOutWatch {
    t0: u64,
    hand0: Vec3,
    toward_user: Vec3,
    uv: [f32; 2],
    world0: Pose,
}

#[derive(Debug, Default)]
pub struct Engine {
    pub panels: Vec<Panel>,
    /// Laser smoothing off: the ray follows the controller exactly.
    pub raw_laser: bool,
    /// Holding the trigger still right-clicks (for controllers without a free button).
    pub hold_right_click: bool,
    /// Trigger-drag on a screen or window scrolls it, like a touchscreen (as OVR Toolkit's touchscreen mode).
    pub drag_scroll: bool,
    keyboard: Keyboard,
    hands: [HandState; 2],
    grab: Grab,
    last_ms: Option<u64>,
    mouse_hand: Hand,
}

impl Engine {
    pub fn new(panels: Vec<Panel>) -> Self {
        Self { panels, hold_right_click: true, ..Self::default() }
    }

    pub fn keyboard(&self) -> &Keyboard {
        &self.keyboard
    }

    /// Each hand's (smoothed) laser ray as last stepped.
    pub fn rays(&self) -> [Option<Pose>; 2] {
        [self.hands[0].ray, self.hands[1].ray]
    }

    /// The hand is using the overlay: laser on a panel, a press or drag in progress, or holding a panel.
    /// The app gives such a hand's input to the overlay alone so the game never sees it.
    pub fn engaged(&self, hand: Hand) -> bool {
        let st = &self.hands[hand.index()];
        let holding = match self.grab {
            Grab::One { hand: h, .. } => h == hand,
            Grab::Two { .. } => true,
            Grab::Idle => false,
        };
        st.on_panel || holding || st.right.is_some() || !matches!(st.press, Press::Idle)
    }

    pub fn grabbed(&self) -> Option<u32> {
        match self.grab {
            Grab::Idle => None,
            Grab::One { panel, .. } | Grab::Two { panel, .. } => Some(panel),
        }
    }

    /// Puts `panel` in `hand` as if just gripped (used after a pop-out creates a window panel).
    pub fn start_grab(&mut self, hand: Hand, panel: u32, poses: &Anchors) {
        if let Some(i) = self.index_of(panel) {
            let reach = (self.panels[i].world_pose(poses).pos - poses[hand.index()].pos).length();
            self.grab = Grab::One { hand, panel, uv: [0.5, 0.5], reach, near: false, popout: None };
        }
    }

    fn ray(&self, hand: Hand, poses: &Anchors) -> Pose {
        self.hands[hand.index()].ray.unwrap_or(poses[hand.index()])
    }

    pub fn step(&mut self, input: &FrameInput, out: &mut Vec<Event>) {
        let dt = self.last_ms.map_or(0.0, |t| input.now_ms.saturating_sub(t) as f32 / 1000.0);
        self.last_ms = Some(input.now_ms);
        let poses = input.anchors();
        for hand in Hand::BOTH {
            let raw = input.hands[hand.index()].pose;
            let st = &mut self.hands[hand.index()];
            st.ray = Some(match st.ray {
                Some(prev) if !self.raw_laser => smooth(prev, raw),
                _ => raw,
            });
        }
        for hand in Hand::BOTH {
            self.step_grip(hand, input, &poses, out);
        }
        self.update_grab(input, &poses, dt, out);
        for hand in Hand::BOTH {
            self.step_pointer(hand, input, &poses, dt, out);
        }
        for hand in Hand::BOTH {
            self.hands[hand.index()].prev = input.hands[hand.index()];
        }
    }

    /// Nearest visible panel under this hand's (smoothed) laser. A hand never points at panels attached to itself.
    fn hit(&self, hand: Hand, poses: &Anchors) -> Option<(usize, [f32; 2])> {
        let ray = self.hands[hand.index()].ray.unwrap_or(poses[hand.index()]);
        let mut best: Option<(usize, f32, [f32; 2])> = None;
        for (i, p) in self.panels.iter().enumerate() {
            if !p.shown() || p.anchor == Anchor::Hand(hand) {
                continue;
            }
            if let Some((t, uv)) = p.hit(p.world_pose(poses), ray)
                && best.is_none_or(|b| t < b.1)
            {
                best = Some((i, t, uv));
            }
        }
        best.map(|(i, _, uv)| (i, uv))
    }

    fn index_of(&self, id: u32) -> Option<usize> {
        self.panels.iter().position(|p| p.id == id)
    }

    fn step_grip(&mut self, hand: Hand, input: &FrameInput, poses: &Anchors, out: &mut Vec<Event>) {
        let cur = input.hands[hand.index()];
        let prev = self.hands[hand.index()].prev;
        if cur.grip && !prev.grip && cur.tracked {
            let Some((i, uv)) = self.hit(hand, poses) else { return };
            let p = self.panels[i];
            // The watch stays on its wrist.
            if p.locked || p.source == Source::Wrist {
                return;
            }
            let world = p.world_pose(poses);
            match self.grab {
                Grab::Idle => {
                    let ray = poses[hand.index()];
                    let popout = matches!(p.source, Source::Monitor(_)).then_some(PopOutWatch {
                        t0: input.now_ms,
                        hand0: ray.pos,
                        toward_user: -ray.forward(),
                        uv,
                        world0: world,
                    });
                    let spot = world.transform_point(p.local_point(uv));
                    let reach = (spot - self.ray(hand, poses).pos).length().max(MIN_GRAB_DISTANCE_M);
                    self.grab = Grab::One { hand, panel: p.id, uv, reach, near: false, popout };
                }
                Grab::One { hand: other, panel, .. } if panel == p.id && other != hand => {
                    let (a, b) = (poses[0].pos, poses[1].pos);
                    self.grab = Grab::Two {
                        panel,
                        mid0: (a + b) * 0.5,
                        len0: (b - a).length().max(1e-3),
                        pos0: world.pos,
                        width0: p.width_m,
                    };
                }
                _ => return,
            }
            out.push(Event::Haptic { hand });
        } else if !cur.grip && prev.grip {
            match self.grab {
                Grab::One { hand: h, panel, .. } if h == hand => {
                    self.grab = Grab::Idle;
                    self.let_go(hand, panel, poses, out);
                }
                Grab::Two { panel, .. } => {
                    let other = hand.other();
                    self.grab = match self.index_of(panel) {
                        Some(i) => Grab::One {
                            hand: other,
                            panel,
                            uv: [0.5, 0.5],
                            reach: (self.panels[i].world_pose(poses).pos - self.ray(other, poses).pos).length(),
                            near: false,
                            popout: None,
                        },
                        None => Grab::Idle,
                    };
                }
                _ => {}
            }
        }
    }

    /// `hand` let go of `panel`: next to the other hand it goes onto that hand; a panel that was on a hand and
    /// is let go away from it stays in the room.
    fn let_go(&mut self, hand: Hand, panel: u32, poses: &Anchors, out: &mut Vec<Event>) {
        let Some(i) = self.index_of(panel) else { return };
        let p = &mut self.panels[i];
        let world = p.world_pose(poses);
        let near = |h: Hand| (world.pos - poses[h.index()].pos).length() < ATTACH_REACH_M;
        let other = hand.other();
        if near(other) {
            p.hold_in(other, poses);
            out.push(Event::Moved { panel });
            out.push(Event::Haptic { hand: other });
        } else if let Anchor::Hand(on) = p.anchor
            && !near(on)
        {
            p.anchor = Anchor::World;
            p.set_world_pose(world, poses);
            out.push(Event::Moved { panel });
        }
    }

    fn update_grab(&mut self, input: &FrameInput, poses: &Anchors, dt: f32, out: &mut Vec<Event>) {
        let Some(id) = self.grabbed() else { return };
        let Some(i) = self.index_of(id) else {
            self.grab = Grab::Idle;
            return;
        };
        let rays = self.rays();
        let eye = poses[HEAD].pos;
        let ease = 1.0 - (-dt / FACE_EASE_S).exp();
        let p = &mut self.panels[i];
        let world = p.world_pose(poses);
        match &mut self.grab {
            Grab::Idle => return,
            Grab::One { hand, uv, reach, near, popout, .. } => {
                if let Some(w) = *popout {
                    let d = poses[hand.index()].pos - w.hand0;
                    let along = d.dot(w.toward_user);
                    let lateral = (d - w.toward_user * along).length();
                    if input.now_ms.saturating_sub(w.t0) > POP_OUT_WINDOW_MS || lateral > POP_OUT_LATERAL_M {
                        *popout = None;
                    } else if along >= POP_OUT_PULL_M {
                        let (hand, uv) = (*hand, w.uv);
                        p.set_world_pose(w.world0, poses);
                        self.grab = Grab::Idle;
                        out.push(Event::Moved { panel: id });
                        out.push(Event::PopOut { hand, panel: id, uv });
                        out.push(Event::Haptic { hand });
                        return;
                    }
                }
                // Stick up/down pushes and pulls, left/right resizes: whichever way it leans further.
                let [x, y] = input.hands[hand.index()].stick;
                if y.abs() > STICK_DEADZONE && y.abs() >= x.abs() {
                    *reach = (*reach + y * PUSH_PULL_M_PER_S * dt).max(MIN_GRAB_DISTANCE_M);
                } else if x.abs() > STICK_DEADZONE {
                    p.width_m = (p.width_m * (x * RESIZE_PER_S * dt).exp()).clamp(MIN_WIDTH_M, MAX_WIDTH_M);
                }
                let ray = rays[hand.index()].unwrap_or(poses[hand.index()]);
                let spot = ray.pos + ray.forward() * *reach;
                // Faces you from its middle (where it was last frame), so a grab near an edge never skews it.
                let rot = turn_toward(world.rot, world.pos - eye, ease);
                let center = spot - rot.rotate(p.local_point(*uv));
                let other = hand.other();
                let close = (center - poses[other.index()].pos).length() < ATTACH_REACH_M;
                if close && !*near {
                    out.push(Event::Haptic { hand: other });
                }
                *near = close;
                p.set_world_pose(Pose::new(center, rot), poses);
            }
            Grab::Two { mid0, len0, pos0, width0, .. } => {
                let (a, b) = (poses[0].pos, poses[1].pos);
                let s = (b - a).length().max(1e-3) / *len0;
                let center = (a + b) * 0.5 + (*pos0 - *mid0) * s;
                p.width_m = (*width0 * s).clamp(MIN_WIDTH_M, MAX_WIDTH_M);
                p.set_world_pose(Pose::new(center, turn_toward(world.rot, center - eye, ease)), poses);
            }
        }
        out.push(Event::Moved { panel: id });
    }

    fn step_pointer(&mut self, hand: Hand, input: &FrameInput, poses: &Anchors, dt: f32, out: &mut Vec<Event>) {
        let i = hand.index();
        let cur = input.hands[i];
        let mut st = self.hands[i];
        let now = input.now_ms;

        if hand == Hand::Left && cur.menu && !st.prev.menu {
            if st.menu_tap.is_some_and(|t| now.saturating_sub(t) <= MENU_DOUBLE_TAP_MS) {
                out.push(Event::HideToggle);
                st.menu_tap = None;
            } else {
                st.menu_tap = Some(now);
            }
        }

        let grabbing = match self.grab {
            Grab::One { hand: h, .. } => h == hand,
            Grab::Two { .. } => true,
            Grab::Idle => false,
        };
        let hit = if cur.tracked && !grabbing {
            self.hit(hand, poses).map(|(idx, uv)| (self.panels[idx].id, self.panels[idx].source, uv))
        } else {
            None
        };
        st.on_panel = hit.is_some();
        let trigger_down = cur.trigger && !st.prev.trigger;

        // A/X: the right button, held as long as the button is (so right-drag works too). Screens and windows
        // only; the overlay's own panels have no right click.
        match (st.right, hit) {
            (Some((panel, last)), _) => {
                let uv = hit.filter(|h| h.0 == panel).map_or(last, |h| h.2);
                if cur.right_click && cur.tracked {
                    if uv != last {
                        out.push(Event::Hover { hand, panel, uv, cursor: true });
                    }
                    st.right = Some((panel, uv));
                } else {
                    out.push(Event::Mouse { panel, uv, button: MouseButton::Right, down: false });
                    st.right = None;
                }
            }
            (None, Some((panel, Source::Monitor(_) | Source::Window(_), uv)))
                if cur.right_click && !st.prev.right_click && matches!(st.press, Press::Idle) =>
            {
                self.mouse_hand = hand;
                out.push(Event::Hover { hand, panel, uv, cursor: true });
                out.push(Event::Mouse { panel, uv, button: MouseButton::Right, down: true });
                out.push(Event::Haptic { hand });
                st.right = Some((panel, uv));
            }
            _ => {}
        }

        st.press = match st.press {
            Press::Idle => match hit {
                Some((panel, Source::Keyboard, uv)) => {
                    out.push(Event::Hover { hand, panel, uv, cursor: false });
                    match keyboard::key_at(uv) {
                        Some(key) if trigger_down => {
                            let mods = self.keyboard.press(key.sc, out);
                            out.push(Event::Haptic { hand });
                            Press::Key { sc: key.sc, mods, next_repeat: now + KEY_REPEAT_DELAY_MS }
                        }
                        _ => Press::Idle,
                    }
                }
                Some((panel, source, uv)) if trigger_down => {
                    self.mouse_hand = hand;
                    let scrolls = self.drag_scroll && matches!(source, Source::Monitor(_) | Source::Window(_));
                    Press::Pending { panel, uv, t0: now, scrolls }
                }
                Some((panel, _, uv)) => {
                    out.push(Event::Hover { hand, panel, uv, cursor: self.mouse_hand == hand });
                    st.scroll = scroll(st.scroll, cur.stick[1], dt, panel, uv, out);
                    Press::Idle
                }
                None => {
                    st.scroll = 0.0;
                    Press::Idle
                }
            },
            Press::Pending { panel, uv: uv0, t0, scrolls } => {
                if !cur.trigger {
                    let uv = match st.last_click {
                        Some((p, last, t))
                            if p == panel
                                && now.saturating_sub(t) <= DOUBLE_CLICK_MS
                                && dist(last, uv0) <= DOUBLE_CLICK_UV =>
                        {
                            last
                        }
                        _ => uv0,
                    };
                    click(panel, uv, MouseButton::Left, out);
                    out.push(Event::Haptic { hand });
                    st.last_click = Some((panel, uv, now));
                    Press::Idle
                } else if self.hold_right_click && now.saturating_sub(t0) >= LONG_PRESS_MS {
                    click(panel, uv0, MouseButton::Right, out);
                    out.push(Event::Haptic { hand });
                    st.last_click = None;
                    Press::Consumed
                } else {
                    match hit {
                        Some((p, _, uv)) if p == panel && dist(uv, uv0) <= DRAG_UV => {
                            Press::Pending { panel, uv: uv0, t0, scrolls }
                        }
                        _ if scrolls => Press::Scrolling { panel, at: uv0, v: uv0[1], acc: 0.0 },
                        _ => {
                            let uv = hit.filter(|h| h.0 == panel).map_or(uv0, |h| h.2);
                            out.push(Event::Mouse { panel, uv: uv0, button: MouseButton::Left, down: true });
                            out.push(Event::Hover { hand, panel, uv, cursor: true });
                            Press::Dragging { panel, uv }
                        }
                    }
                }
            }
            Press::Scrolling { panel, at, v, acc } if cur.trigger => {
                let now_v = hit.filter(|h| h.0 == panel).map_or(v, |h| h.2[1]);
                // Pulling the page down shows what is above: the wheel turns up.
                let acc = acc + (now_v - v) / DRAG_SCROLL_UV;
                let n = acc.trunc();
                if n != 0.0 {
                    out.push(Event::Scroll { panel, uv: at, notches: n as i32 });
                }
                Press::Scrolling { panel, at, v: now_v, acc: acc - n }
            }
            Press::Scrolling { .. } => Press::Idle,
            Press::Dragging { panel, uv: last } => {
                let uv = hit.filter(|h| h.0 == panel).map_or(last, |h| h.2);
                if uv != last {
                    out.push(Event::Hover { hand, panel, uv, cursor: true });
                }
                if cur.trigger {
                    Press::Dragging { panel, uv }
                } else {
                    out.push(Event::Mouse { panel, uv, button: MouseButton::Left, down: false });
                    Press::Idle
                }
            }
            Press::Consumed if cur.trigger => Press::Consumed,
            Press::Consumed => Press::Idle,
            Press::Key { sc, mods, .. } if !cur.trigger || !cur.tracked => {
                self.keyboard.release(sc, mods, out);
                Press::Idle
            }
            Press::Key { sc, mods, next_repeat } if keyboard::repeats(sc) && now >= next_repeat => {
                out.push(Event::Key { scancode: sc, down: true });
                Press::Key { sc, mods, next_repeat: next_repeat + KEY_REPEAT_INTERVAL_MS }
            }
            held @ Press::Key { .. } => held,
        };
        self.hands[i] = st;
    }
}

/// One easing step of a held panel's turn toward facing the eyes along `away`: the way it faces moves a share
/// `t` of the way there, and it is rebuilt level every step, so it never rolls on the way.
fn turn_toward(rot: Quat, away: Vec3, t: f32) -> Quat {
    let now = rot.rotate(Vec3::new(0.0, 0.0, -1.0));
    let to = away.normalized();
    level_facing(now + (to - now) * t)
}

/// One smoothing step from the last ray toward the raw one: the faster the hand turns, the less smoothing.
fn smooth(prev: Pose, raw: Pose) -> Pose {
    let turn = prev.forward().dot(raw.forward()).clamp(-1.0, 1.0).acos();
    let moved = (raw.pos - prev.pos).length();
    let t = (turn / SMOOTH_FULL_RAD).max(moved / 0.02).clamp(SMOOTH_MIN, 1.0);
    Pose::new(prev.pos + (raw.pos - prev.pos) * t, prev.rot.nlerp(raw.rot, t))
}

fn click(panel: u32, uv: [f32; 2], button: MouseButton, out: &mut Vec<Event>) {
    out.push(Event::Mouse { panel, uv, button, down: true });
    out.push(Event::Mouse { panel, uv, button, down: false });
}

fn scroll(acc: f32, y: f32, dt: f32, panel: u32, uv: [f32; 2], out: &mut Vec<Event>) -> f32 {
    if y.abs() <= STICK_DEADZONE {
        return 0.0;
    }
    let acc = acc + y * SCROLL_NOTCHES_PER_S * dt;
    let n = acc.trunc();
    if n != 0.0 {
        out.push(Event::Scroll { panel, uv, notches: n as i32 });
    }
    acc - n
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{Quat, Vec3};
    use crate::panel::Anchor;

    #[test]
    fn smoothing_calms_tremor_but_never_lags_real_moves() {
        let base = Pose::aim(Vec3::new(0.0, 1.2, 0.0), Vec3::new(0.0, 1.2, -1.0));
        let wobble = |a: f32| Pose::new(base.pos, base.rot * Quat::from_axis_angle(Vec3::Y, a));
        // Tremor of +-0.3 degrees: the smoothed ray moves much less than the raw one.
        let mut ray = base;
        let mut worst = 0.0f32;
        for i in 0..90 {
            let raw = wobble(if i % 2 == 0 { 0.005 } else { -0.005 });
            ray = smooth(ray, raw);
            worst = worst.max(ray.forward().dot(base.forward()).clamp(-1.0, 1.0).acos());
        }
        assert!(worst < 0.004, "tremor not smoothed: {worst} rad");
        // A real flick (10 degrees in one frame) arrives at once.
        let flick = wobble(0.17);
        let after = smooth(base, flick);
        assert!(after.forward().dot(flick.forward()) > 0.9999, "flick lagged");
    }

    const EYE: Vec3 = Vec3::new(0.0, 1.6, 0.0);

    fn frame(now_ms: u64, hands: [HandInput; 2]) -> FrameInput {
        FrameInput { now_ms, hands, head: Pose::new(EYE, Quat::IDENTITY) }
    }

    fn hand_at(pos: Vec3, at: Vec3) -> HandInput {
        HandInput { pose: Pose::aim(pos, at), tracked: true, ..HandInput::default() }
    }

    fn screen() -> Engine {
        let pose = Pose::new(Vec3::new(0.0, 1.4, -1.0), Quat::IDENTITY);
        let mut e = Engine::new(vec![Panel::new(1, Source::Monitor(0), pose, 1.0, 0.5)]);
        e.raw_laser = true;
        e
    }

    /// Steps `frames` frames of 11 ms from `t`, returning the time after.
    fn run(e: &mut Engine, t: u64, frames: u64, hands: [HandInput; 2], out: &mut Vec<Event>) -> u64 {
        for i in 0..frames {
            e.step(&frame(t + i * 11, hands), out);
        }
        t + frames * 11
    }

    #[test]
    fn a_held_screen_stays_level_when_the_wrist_rolls() {
        let mut e = screen();
        let mut out = Vec::new();
        let rest = hand_at(Vec3::new(-0.3, 1.2, -0.2), Vec3::new(-0.3, 1.2, -1.0));
        let mut r = hand_at(Vec3::new(0.2, 1.3, -0.2), Vec3::new(0.2, 1.4, -1.0));
        r.grip = true;
        let t = run(&mut e, 0, 2, [rest, r], &mut out);
        assert_eq!(e.grabbed(), Some(1));
        // Roll the wrist 60 degrees about the laser: the screen does not tip.
        r.pose.rot = r.pose.rot * Quat::from_axis_angle(Vec3::FORWARD, 1.0);
        run(&mut e, t, 60, [rest, r], &mut out);
        let p = e.panels[0];
        assert!(p.pose.rot.rotate(Vec3::X).y.abs() < 1e-3, "rolled: {:?}", p.pose.rot.rotate(Vec3::X));
        let to_eye = (EYE - p.pose.pos).normalized();
        assert!(p.pose.rot.rotate(Vec3::Z).dot(to_eye) > 0.999, "faces the eyes");
        // The grabbed spot is still under the laser.
        let ray = r.pose;
        let (_, uv) = p.hit(p.pose, ray).expect("laser on the screen");
        assert!((uv[0] - 0.7).abs() < 0.02 && (uv[1] - 0.5).abs() < 0.02, "spot moved to {uv:?}");
    }

    #[test]
    fn grip_and_stick_sideways_resize_smoothly() {
        let mut e = screen();
        let mut out = Vec::new();
        let rest = hand_at(Vec3::new(-0.3, 1.2, -0.2), Vec3::new(-0.3, 1.2, -1.0));
        let mut r = hand_at(Vec3::new(0.0, 1.4, -0.2), Vec3::new(0.0, 1.4, -1.0));
        r.grip = true;
        let t = run(&mut e, 0, 1, [rest, r], &mut out);
        r.stick = [1.0, 0.0];
        let mut last = e.panels[0].width_m;
        let mut t = t;
        for _ in 0..45 {
            t = run(&mut e, t, 1, [rest, r], &mut out);
            let w = e.panels[0].width_m;
            assert!(w > last && w - last < 0.02, "each frame grows a little: {last} -> {w}");
            last = w;
        }
        assert!((last - (0.8f32 * 0.495).exp()).abs() < 0.05, "about 0.8 per second: {last}");
    }

    #[test]
    fn let_go_by_the_other_hand_and_it_stays_there() {
        let mut e = screen();
        let mut out = Vec::new();
        let left = hand_at(Vec3::new(-0.25, 1.1, -0.35), Vec3::new(-0.25, 1.1, -1.0));
        let mut r = hand_at(Vec3::new(0.0, 1.4, -0.2), Vec3::new(0.0, 1.4, -1.0));
        r.grip = true;
        let mut t = run(&mut e, 0, 1, [left, r], &mut out);
        // Aim at the left hand and pull it in until it is there (0.8 m out to 0.4 m: 0.27 s of full stick).
        r.pose = Pose::aim(Vec3::new(0.0, 1.3, -0.1), left.pose.pos);
        r.stick = [0.0, -1.0];
        t = run(&mut e, t, 24, [left, r], &mut out);
        out.clear();
        r.stick = [0.0; 2];
        r.grip = false;
        run(&mut e, t, 1, [left, r], &mut out);
        let p = e.panels[0];
        assert_eq!(p.anchor, Anchor::Hand(Hand::Left), "on the left hand");
        assert!(out.contains(&Event::Haptic { hand: Hand::Left }), "the hand feels it land");
        // Taken away with the right hand and let go in the room: back in the room.
        let mut e2 = e;
        let mut r =
            hand_at(Vec3::new(0.3, 1.3, -0.1), p.world_pose(&[left.pose, r.pose, Pose::new(EYE, Quat::IDENTITY)]).pos);
        r.grip = true;
        let t = run(&mut e2, 0, 1, [left, r], &mut out);
        r.stick = [0.0, 1.0];
        let t = run(&mut e2, t, 40, [left, r], &mut out);
        r.grip = false;
        run(&mut e2, t, 1, [left, r], &mut out);
        assert_eq!(e2.panels[0].anchor, Anchor::World, "let go away from the hand");
    }

    #[test]
    fn drag_to_scroll_pulls_the_page_instead_of_dragging() {
        for on in [false, true] {
            let mut e = screen();
            e.drag_scroll = on;
            let mut out = Vec::new();
            let rest = hand_at(Vec3::new(-0.3, 1.2, -0.2), Vec3::new(-0.3, 1.2, -1.0));
            let mut r = hand_at(Vec3::new(0.0, 1.4, -0.2), Vec3::new(0.0, 1.45, -1.0));
            r.trigger = true;
            let mut t = run(&mut e, 0, 1, [rest, r], &mut out);
            // Pull down a fifth of the screen's height over a few frames, then let go.
            for i in 1..=5 {
                r.pose = Pose::aim(r.pose.pos, Vec3::new(0.0, 1.45 - 0.02 * i as f32, -1.0));
                t = run(&mut e, t, 1, [rest, r], &mut out);
            }
            r.trigger = false;
            run(&mut e, t, 1, [rest, r], &mut out);
            let notches: i32 =
                out.iter().map(|e| if let Event::Scroll { notches, .. } = e { *notches } else { 0 }).sum();
            let left = out.iter().filter(|e| matches!(e, Event::Mouse { button: MouseButton::Left, .. })).count();
            if on {
                assert!(notches >= 5 && left == 0, "scrolls up, no click: {notches} notches, {left} left");
            } else {
                assert!(notches == 0 && left == 2, "drags: {notches} notches, {left} left");
            }
        }
    }

    #[test]
    fn a_or_x_right_clicks_where_the_laser_points() {
        let mut e = screen();
        let mut out = Vec::new();
        let rest = hand_at(Vec3::new(-0.3, 1.2, -0.2), Vec3::new(-0.3, 1.2, -1.0));
        let mut r = hand_at(Vec3::new(0.0, 1.4, -0.2), Vec3::new(0.0, 1.4, -1.0));
        let t = run(&mut e, 0, 1, [rest, r], &mut out);
        out.clear();
        r.right_click = true;
        let t = run(&mut e, t, 3, [rest, r], &mut out);
        let downs: Vec<_> =
            out.iter().filter(|e| matches!(e, Event::Mouse { button: MouseButton::Right, down: true, .. })).collect();
        assert_eq!(downs.len(), 1, "one press: {out:?}");
        assert!(e.engaged(Hand::Right), "the game never sees it");
        r.right_click = false;
        out.clear();
        run(&mut e, t, 1, [rest, r], &mut out);
        assert!(out.iter().any(|e| matches!(e, Event::Mouse { button: MouseButton::Right, down: false, .. })));
        assert!(!out.iter().any(|e| matches!(e, Event::Mouse { button: MouseButton::Left, .. })), "never a left click");
    }
}
