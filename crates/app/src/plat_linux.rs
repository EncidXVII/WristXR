// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the VR session needs from the platform, on Linux: Vulkan textures, X11 or Wayland-portal capture and
//! input. The same names as `plat_win.rs`, so `run.rs` is the same file for both.
use wxr_vr::OpenVr;

pub use wxr_desktop_linux::{
    Injector, Monitor, admin_window_at, caps_lock, focus, key_char, layout_id, layout_name, layouts, monitors,
    next_layout, select_layout, window_at, window_rect, windows,
};
pub use wxr_gfx_vk::{Gpu, GpuTimer, Tex};

/// The process name of SteamVR's server, watched so WristXR leaves with it.
pub const STEAMVR_SERVER: &str = "vrserver";

/// On the GPU the headset is on.
pub fn gpu(vr: &OpenVr) -> Result<Gpu, String> {
    Gpu::new(vr)
}

/// Hands a texture to an overlay. Call after `Gpu::flush`.
/// Nothing to wait for: Linux screens never change detail level (see `Detail`).
pub fn finish(_gpu: &Gpu) {}

pub fn submit(gpu: &Gpu, vr: &mut OpenVr, key: u64, tex: &Tex) -> Result<(), String> {
    gpu.submit(vr, key, tex)
}

/// The GPU names, for the self-test report (the self-test runs without a headset, so none is opened).
pub fn adapter_names() -> Vec<String> {
    Vec::new()
}

/// Result of polling a capture once per frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    Unchanged,
    Copied,
    /// The source changed size: recreate the target texture at this size; frames follow.
    Resized(u32, u32),
}

/// A live capture of a screen or window, copied into a panel texture.
pub struct Capture(wxr_desktop_linux::Capture);

impl Capture {
    pub fn monitor(_gpu: &Gpu, m: &Monitor) -> Result<Self, String> {
        wxr_desktop_linux::Capture::monitor(m).map(Self)
    }

    pub fn window(_gpu: &Gpu, id: u64) -> Result<Self, String> {
        wxr_desktop_linux::Capture::window(id).map(Self)
    }

    pub fn copy_latest(&mut self, gpu: &Gpu, target: &Tex) -> Result<Frame, String> {
        match self.0.grab() {
            None => Ok(Frame::Unchanged),
            Some((w, h, _)) if (w, h) != target.size() => Ok(Frame::Resized(w, h)),
            Some((w, _, px)) => {
                gpu.update(target, px, w);
                Ok(Frame::Copied)
            }
        }
    }
}

// The detail pass (mip levels and a sharpen shader, see wxr-gfx-d3d11) is Direct3D only for now: on Linux screens
// are shown as captured. These stand-ins keep `run.rs` one file; `Sharpen::new` failing turns the pass off there.

pub struct Sharpen;

impl Sharpen {
    pub fn new(_gpu: &Gpu) -> Result<Self, String> {
        Err("the screen detail pass is not built for Vulkan yet".into())
    }
    pub fn new_frame(&self, _gpu: &Gpu, _d: &Detail) {}
    pub fn draw(&self, _gpu: &Gpu, _d: &Detail) {}
}

pub struct Detail {
    pub src: Tex,
    pub out: Tex,
    pub width: u32,
    pub w: u32,
}

impl Detail {
    pub fn new(_gpu: &Gpu, _w: u32, _h: u32, _width: u32) -> Result<Self, String> {
        Err("the screen detail pass is not built for Vulkan yet".into())
    }
    pub fn set_width(&mut self, _gpu: &Gpu, _width: u32) -> Result<(), String> {
        Ok(())
    }
    pub fn bounds(&self) -> [f32; 4] {
        [0.0, 0.0, 1.0, 1.0]
    }
}

pub fn width_for(width: u32, _pixels_across: f32) -> u32 {
    width
}

pub fn keeps(_current: u32, _needed: f32) -> bool {
    true
}
