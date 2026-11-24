// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the VR session needs from the platform, on Windows: Direct3D 11 textures, Windows Graphics Capture,
//! SendInput. Linux has the same names in `plat_linux.rs`, so `run.rs` is the same file for both.
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use wxr_core::panel::PixelRect;
pub use wxr_vr::OpenVr as Vr;

/// WristXR runs in SteamVR (always, on Windows).
pub fn on_steamvr(_vr: &Vr) -> bool {
    true
}

pub use wxr_capture_win::{Frame, Monitor};
pub use wxr_gfx_d3d11::{Detail, Gpu, GpuTimer, Sharpen, keeps, width_for};
pub use wxr_input_win::{
    Injector, admin_window_at, caps_lock, key_char, layout_id, layout_name, layouts, next_layout, select_layout,
};

/// A panel texture.
pub type Tex = ID3D11Texture2D;

/// Windows always lets WristXR click and type (see `admin_window_at` for the one exception).
pub fn view_only() -> bool {
    false
}

/// The process name of SteamVR's server, watched so WristXR leaves with it.
pub const STEAMVR_SERVER: &str = "vrserver.exe";

/// On the adapter SteamVR renders on. (Mutable for Linux, where an OpenXR session starts here.)
pub fn gpu(vr: &mut Vr) -> Result<Gpu, String> {
    Gpu::new(vr.adapter_index()).map_err(|e| e.to_string())
}

/// Waits until the GPU has drawn everything queued (see `Gpu::finish`).
pub fn finish(gpu: &Gpu) {
    gpu.finish();
}

/// Hands a texture to an overlay. Call after `Gpu::flush`.
pub fn submit(_gpu: &Gpu, vr: &mut Vr, key: u64, tex: &Tex) -> Result<(), String> {
    vr.set_texture_d3d11(key, wxr_gfx_d3d11::raw(tex))
}

pub fn monitors() -> Vec<Monitor> {
    wxr_capture_win::monitors()
}

/// Windows the user can add, front to back.
pub fn windows() -> Vec<(u64, String)> {
    wxr_capture_win::windows().into_iter().map(|(h, title)| (h as u64, title)).collect()
}

pub fn window_rect(id: u64) -> Option<PixelRect> {
    wxr_capture_win::window_rect(id as isize)
}

pub fn window_at(p: (i32, i32)) -> Option<u64> {
    wxr_capture_win::window_at(p).map(|h| h as u64)
}

pub fn focus(id: u64) {
    wxr_capture_win::focus(id as isize);
}

/// The GPU names, for the self-test report.
pub fn adapter_names() -> Vec<String> {
    wxr_gfx_d3d11::adapter_names()
}

/// A live capture of a screen or window, copied into a panel texture.
pub struct Capture(wxr_capture_win::Capture);

impl Capture {
    pub fn monitor(gpu: &Gpu, m: &Monitor) -> Result<Self, String> {
        wxr_capture_win::Capture::monitor(&gpu.device, m.handle).map(Self).map_err(|e| e.to_string())
    }

    pub fn window(gpu: &Gpu, id: u64) -> Result<Self, String> {
        wxr_capture_win::Capture::window(&gpu.device, id as isize).map(Self).map_err(|e| e.to_string())
    }

    pub fn copy_latest(&mut self, gpu: &Gpu, target: &Tex) -> Result<Frame, String> {
        self.0.copy_latest(&gpu.context, target).map_err(|e| e.to_string())
    }
}
