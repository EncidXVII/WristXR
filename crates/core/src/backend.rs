use crate::engine::FrameInput;
use crate::math::Pose;
use crate::panel::{Hand, Panel};

/// The VR runtime seen by the app. OpenVR today; OpenXR fits behind the same calls.
pub trait VrBackend {
    /// Fills `input` (poses in standing space, buttons). Returns false when the runtime asks us to quit.
    fn poll(&mut self, now_ms: u64, input: &mut FrameInput) -> bool;
    fn head_pose(&self) -> Option<Pose>;
    fn haptic(&mut self, hand: Hand);
    /// Creates the panel's overlay on first use, then updates size, opacity, transform and visibility.
    fn place_panel(&mut self, panel: &Panel, world: Pose);
    fn remove_panel(&mut self, id: u32);
    /// Laser dot for a hand; `None` hides it.
    fn place_cursor(&mut self, hand: Hand, at: Option<Pose>);
}
