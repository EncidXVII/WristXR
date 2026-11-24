// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the VR session needs from the platform, on Linux: Vulkan textures, X11 or Wayland-portal capture and
//! input. The same names as `plat_win.rs`, so `run.rs` is the same file for both.
pub use wxr_vr::Vr;

/// WristXR runs in SteamVR, not as an OpenXR overlay (Monado, WiVRn).
pub fn on_steamvr(vr: &Vr) -> bool {
    matches!(vr, Vr::OpenVr(_))
}

use std::os::fd::AsFd;
use wxr_desktop_linux::Pixels;
pub use wxr_desktop_linux::{
    Injector, Monitor, admin_window_at, caps_lock, focus, key_char, layout_id, layout_name, layouts, monitors,
    next_layout, select_layout, view_only, window_at, window_rect, windows,
};
use wxr_gfx_vk::DmaBuf;
pub use wxr_gfx_vk::{Gpu, GpuTimer, Tex};

/// The process name of SteamVR's server, watched so WristXR leaves with it.
pub const STEAMVR_SERVER: &str = "vrserver";

/// On the GPU the headset is on (under OpenXR, the session starts here too). With `WRISTXR_DMABUF=1`, Wayland
/// captures offer the compositor GPU buffers (DMA-BUF) the GPU can import, so frames skip the CPU; experimental
/// until tested on more GPUs and desktops.
pub fn gpu(vr: &mut Vr) -> Result<Gpu, String> {
    let gpu = Gpu::new(vr)?;
    if std::env::var_os("WRISTXR_DMABUF").is_some_and(|v| v == "1") {
        let modifiers = gpu.dmabuf_modifiers();
        tracing::info!(n = modifiers.len(), "DMA-BUF capture");
        wxr_desktop_linux::set_dmabuf_modifiers(modifiers);
    }
    Ok(gpu)
}

/// Hands a texture to an overlay. Call after `Gpu::flush`.
/// Nothing to wait for: Linux screens never change detail level (see `Detail`).
pub fn finish(_gpu: &Gpu) {}

pub fn submit(gpu: &Gpu, vr: &mut Vr, key: u64, tex: &Tex) -> Result<(), String> {
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
        let mut import_failed = false;
        let frame = self.0.grab(|w, h, px| {
            if (w, h) != target.size() {
                return Frame::Resized(w, h);
            }
            match px {
                Pixels::Bgra(bgra) => {
                    gpu.update(target, bgra, w);
                    Frame::Copied
                }
                Pixels::DmaBuf(f) => {
                    let buf = DmaBuf {
                        fd: f.fd.as_fd(),
                        offset: f.offset,
                        stride: f.stride,
                        modifier: f.modifier,
                        w: f.w,
                        h: f.h,
                        x: f.crop.x,
                        y: f.crop.y,
                    };
                    match gpu.copy_dmabuf(target, &buf) {
                        Ok(()) => Frame::Copied,
                        Err(e) => {
                            tracing::warn!(%e, "DMA-BUF frame not imported; asking for shared memory");
                            import_failed = true;
                            Frame::Unchanged
                        }
                    }
                }
            }
        });
        if import_failed {
            // Off for every capture from now on, and this one renegotiates.
            wxr_desktop_linux::set_dmabuf_modifiers(Vec::new());
            self.0.dmabuf_failed();
        }
        if let Some(Frame::Resized(..)) = frame {
            // That frame's pixels were not copied: the next poll takes them, into the new texture.
            self.0.again();
        }
        Ok(frame.unwrap_or(Frame::Unchanged))
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
