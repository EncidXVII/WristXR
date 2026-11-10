//! Per-frame interaction logic: laser pointer, keyboard typing, grab/scale.
//! Rules: hover never clicks, trigger clicks, grip only grabs, locked panels ignore grip.
//! `step` does not allocate as long as `out` has spare capacity (reserve ~64).
use crate::keyboard::{self, Keyboard};
use crate::math::{Pose, Quat, Vec3};
use crate::panel::{Anchor, Anchors, Hand, MAX_WIDTH_M, MIN_WIDTH_M, Panel, Source};

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
pub const PUSH_PULL_M_PER_S: f32 = 1.5;
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
        rel: Pose,
        popout: Option<PopOutWatch>,
    },
    Two {
        panel: u32,
        mid0: Vec3,
        dir0: Vec3,
        len0: f32,
        pose0: Pose,
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
    keyboard: Keyboard,
    hands: [HandState; 2],
    grab: Grab,
    last_ms: Option<u64>,
    mouse_hand: Hand,
}

impl Engine {
    pub fn new(panels: Vec<Panel>) -> Self {
        Self { panels, ..Self::default() }
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
        st.on_panel || holding || !matches!(st.press, Press::Idle)
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
            let rel = poses[hand.index()].inverse() * self.panels[i].world_pose(poses);
            self.grab = Grab::One { hand, panel, rel, popout: None };
        }
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
            if p.locked {
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
                    self.grab = Grab::One { hand, panel: p.id, rel: ray.inverse() * world, popout };
                }
                Grab::One { hand: other, panel, .. } if panel == p.id && other != hand => {
                    let (a, b) = (poses[0].pos, poses[1].pos);
                    let len = (b - a).length().max(1e-3);
                    self.grab = Grab::Two {
                        panel,
                        mid0: (a + b) * 0.5,
                        dir0: (b - a) * (1.0 / len),
                        len0: len,
                        pose0: world,
                        width0: p.width_m,
                    };
                }
                _ => return,
            }
            out.push(Event::Haptic { hand });
        } else if !cur.grip && prev.grip {
            match self.grab {
                Grab::One { hand: h, .. } if h == hand => self.grab = Grab::Idle,
                Grab::Two { panel, .. } => {
                    let other = hand.other();
                    self.grab = match self.index_of(panel) {
                        Some(i) => Grab::One {
                            hand: other,
                            panel,
                            rel: poses[other.index()].inverse() * self.panels[i].world_pose(poses),
                            popout: None,
                        },
                        None => Grab::Idle,
                    };
                }
                _ => {}
            }
        }
    }

    fn update_grab(&mut self, input: &FrameInput, poses: &Anchors, dt: f32, out: &mut Vec<Event>) {
        let mut pop = None;
        let (panel, world, width) = match &mut self.grab {
            Grab::Idle => return,
            Grab::One { hand, panel, rel, popout } => {
                if let Some(w) = *popout {
                    let d = poses[hand.index()].pos - w.hand0;
                    let along = d.dot(w.toward_user);
                    let lateral = (d - w.toward_user * along).length();
                    if input.now_ms.saturating_sub(w.t0) > POP_OUT_WINDOW_MS || lateral > POP_OUT_LATERAL_M {
                        *popout = None;
                    } else if along >= POP_OUT_PULL_M {
                        pop = Some((*hand, w.uv, w.world0));
                    }
                }
                let y = input.hands[hand.index()].stick[1];
                if y.abs() > STICK_DEADZONE {
                    let d = (rel.pos.length() + y * PUSH_PULL_M_PER_S * dt).max(MIN_GRAB_DISTANCE_M);
                    rel.pos = rel.pos.normalized() * d;
                }
                (*panel, poses[hand.index()] * *rel, None)
            }
            Grab::Two { panel, mid0, dir0, len0, pose0, width0 } => {
                let (a, b) = (poses[0].pos, poses[1].pos);
                let len = (b - a).length().max(1e-3);
                let s = len / *len0;
                let rot = Quat::from_arc(*dir0, (b - a) * (1.0 / len));
                let pose =
                    Pose::new((a + b) * 0.5 + rot.rotate((pose0.pos - *mid0) * s), (rot * pose0.rot).normalized());
                (*panel, pose, Some((*width0 * s).clamp(MIN_WIDTH_M, MAX_WIDTH_M)))
            }
        };
        let Some(i) = self.index_of(panel) else {
            self.grab = Grab::Idle;
            return;
        };
        let p = &mut self.panels[i];
        if let Some((hand, uv, world0)) = pop {
            p.set_world_pose(world0, poses);
            self.grab = Grab::Idle;
            out.push(Event::Moved { panel });
            out.push(Event::PopOut { hand, panel, uv });
            out.push(Event::Haptic { hand });
            return;
        }
        p.set_world_pose(world, poses);
        if let Some(w) = width {
            p.width_m = w;
        }
        out.push(Event::Moved { panel });
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
                Some((panel, _, uv)) if trigger_down => {
                    self.mouse_hand = hand;
                    Press::Pending { panel, uv, t0: now }
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
            Press::Pending { panel, uv: uv0, t0 } => {
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
                } else if now.saturating_sub(t0) >= LONG_PRESS_MS {
                    click(panel, uv0, MouseButton::Right, out);
                    out.push(Event::Haptic { hand });
                    st.last_click = None;
                    Press::Consumed
                } else {
                    match hit {
                        Some((p, _, uv)) if p == panel && dist(uv, uv0) <= DRAG_UV => {
                            Press::Pending { panel, uv: uv0, t0 }
                        }
                        _ => {
                            let uv = hit.filter(|h| h.0 == panel).map_or(uv0, |h| h.2);
                            out.push(Event::Mouse { panel, uv: uv0, button: MouseButton::Left, down: true });
                            out.push(Event::Hover { hand, panel, uv, cursor: true });
                            Press::Dragging { panel, uv }
                        }
                    }
                }
            }
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
}
