// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! Windows.Graphics.Capture: monitor frames stay on the GPU (no CPU readback).
//! WGC only produces frames when content changes; dropping a `Capture` stops it (hidden panel = no cost).
#![cfg(windows)]
#![allow(unsafe_code)]

use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, TRUE};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_TEXTURE2D_DESC, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GA_ROOT, GW_OWNER, GWL_EXSTYLE, GetAncestor, GetClassNameW, GetWindow, GetWindowLongW,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible,
    SetForegroundWindow, WS_EX_TOOLWINDOW, WindowFromPoint,
};
use windows::core::{BOOL, Interface, Result, factory};
use wxr_core::panel::PixelRect;

/// Per-monitor DPI awareness (all rects and input in physical pixels) and WinRT apartment.
pub fn init_process() {
    // SAFETY: process-wide setup calls; failures (already set) are harmless.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
}

pub struct Monitor {
    pub handle: HMONITOR,
    pub rect: PixelRect,
    pub primary: bool,
}

pub fn monitors() -> Vec<Monitor> {
    unsafe extern "system" fn add(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        // SAFETY: `data` is the &mut Vec passed below, alive for the enumeration.
        let list = unsafe { &mut *(data.0 as *mut Vec<Monitor>) };
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        // SAFETY: valid monitor handle from the enumeration; cbSize set.
        if unsafe { GetMonitorInfoW(m, &mut info) }.as_bool() {
            let r = info.rcMonitor;
            list.push(Monitor {
                handle: m,
                rect: PixelRect { x: r.left, y: r.top, w: (r.right - r.left) as u32, h: (r.bottom - r.top) as u32 },
                primary: info.dwFlags & 1 != 0,
            });
        }
        TRUE
    }
    let mut list: Vec<Monitor> = Vec::new();
    // SAFETY: callback only touches `list` during this call.
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(add), LPARAM(&mut list as *mut Vec<Monitor> as isize));
    }
    list
}

pub struct Capture {
    _item: GraphicsCaptureItem,
    device: IDirect3DDevice,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    size: SizeInt32,
}

/// Result of polling a capture once per frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    Unchanged,
    Copied,
    /// The source changed size (window resized): recreate the target texture at this size; frames follow.
    Resized(u32, u32),
}

/// Top-level window under a desktop pixel, skipping the desktop and taskbar. Returned as a raw handle.
pub fn window_at(p: (i32, i32)) -> Option<isize> {
    // SAFETY: read-only window queries.
    unsafe {
        let hwnd = GetAncestor(WindowFromPoint(POINT { x: p.0, y: p.1 }), GA_ROOT);
        if hwnd.is_invalid() {
            return None;
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..n.min(class.len())]);
        (!SHELL_CLASSES.contains(&class.as_str())).then_some(hwnd.0 as isize)
    }
}

const SHELL_CLASSES: [&str; 4] = ["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd"];

/// Windows a user would pick from a list (what Alt+Tab shows): visible, titled, not cloaked, not tool windows,
/// not minimized, not this process. Front to back, as (handle, title).
pub fn windows() -> Vec<(isize, String)> {
    unsafe extern "system" fn collect(hwnd: HWND, list: LPARAM) -> BOOL {
        // SAFETY: `list` is the Vec passed to EnumWindows below, alive for the whole enumeration.
        let list = unsafe { &mut *(list.0 as *mut Vec<(isize, String)>) };
        if let Some(title) = pickable(hwnd) {
            list.push((hwnd.0 as isize, title));
        }
        TRUE
    }
    let mut list: Vec<(isize, String)> = Vec::new();
    // SAFETY: the callback only runs during this call, while `list` is borrowed mutably here.
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM((&raw mut list) as isize));
    }
    list
}

fn pickable(hwnd: HWND) -> Option<String> {
    // SAFETY: read-only queries on a handle EnumWindows just gave us; buffers are sized for the calls.
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return None;
        }
        if GetWindow(hwnd, GW_OWNER).is_ok_and(|o| !o.is_invalid()) {
            return None;
        }
        if GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
            return None;
        }
        let mut cloaked = 0u32;
        let ok = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, (&raw mut cloaked).cast(), size_of::<u32>() as u32);
        if ok.is_ok() && cloaked != 0 {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == std::process::id() {
            return None;
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut class) as usize;
        if SHELL_CLASSES.contains(&String::from_utf16_lossy(&class[..n.min(class.len())]).as_str()) {
            return None;
        }
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return None;
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, &mut buf) as usize;
        let title = String::from_utf16_lossy(&buf[..n.min(buf.len())]);
        (!title.trim().is_empty()).then_some(title)
    }
}

/// On-screen rectangle of a window as DWM draws it (what capture shows); `None` once the window is gone.
pub fn window_rect(hwnd: isize) -> Option<PixelRect> {
    let hwnd = HWND(hwnd as *mut _);
    let mut r = RECT::default();
    // SAFETY: plain queries; the out-param matches DWMWA_EXTENDED_FRAME_BOUNDS.
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return None;
        }
        DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, (&raw mut r).cast(), size_of::<RECT>() as u32).ok()?;
    }
    Some(PixelRect { x: r.left, y: r.top, w: (r.right - r.left).max(1) as u32, h: (r.bottom - r.top).max(1) as u32 })
}

/// Brings a window to the front so injected clicks reach it rather than whatever covers it (best effort).
pub fn focus(hwnd: isize) {
    // SAFETY: plain window call; failure is harmless.
    unsafe {
        let _ = SetForegroundWindow(HWND(hwnd as *mut _));
    }
}

impl Capture {
    pub fn monitor(device: &ID3D11Device, monitor: HMONITOR) -> Result<Self> {
        let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        // SAFETY: valid monitor handle.
        let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(monitor)? };
        Self::start(device, item)
    }

    pub fn window(device: &ID3D11Device, hwnd: isize) -> Result<Self> {
        let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        // SAFETY: the handle came from window_at/state; WGC fails cleanly if it is no longer valid.
        let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(HWND(hwnd as *mut _))? };
        Self::start(device, item)
    }

    fn start(device: &ID3D11Device, item: GraphicsCaptureItem) -> Result<Self> {
        let dxgi: IDXGIDevice = device.cast()?;
        // SAFETY: wraps our D3D11 device for WinRT; no ownership transfer.
        let d3d: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? }.cast()?;
        let size = item.Size()?;
        let pool =
            Direct3D11CaptureFramePool::CreateFreeThreaded(&d3d, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size)?;
        let session = pool.CreateCaptureSession(&item)?;
        // Windows 11 only; older systems keep the yellow border.
        let _ = session.SetIsBorderRequired(false);
        session.StartCapture()?;
        Ok(Self { _item: item, device: d3d, pool, session, size })
    }

    /// Copies the newest frame into `target` on the GPU.
    pub fn copy_latest(&mut self, ctx: &ID3D11DeviceContext, target: &ID3D11Texture2D) -> Result<Frame> {
        let mut latest = None;
        while let Ok(frame) = self.pool.TryGetNextFrame() {
            latest = Some(frame);
        }
        let Some(frame) = latest else { return Ok(Frame::Unchanged) };
        let content = frame.ContentSize()?;
        if content != self.size {
            self.size = content;
            self.pool.Recreate(&self.device, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, content)?;
            return Ok(Frame::Resized(content.Width.max(1) as u32, content.Height.max(1) as u32));
        }
        let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
        // SAFETY: the surface is a D3D11 texture created on our device.
        let src: ID3D11Texture2D = unsafe { access.GetInterface()? };
        let (mut a, mut b) = (D3D11_TEXTURE2D_DESC::default(), D3D11_TEXTURE2D_DESC::default());
        // SAFETY: plain descriptor queries and a bounded GPU copy between same-format textures.
        unsafe {
            src.GetDesc(&mut a);
            target.GetDesc(&mut b);
            let region = D3D11_BOX {
                left: 0,
                top: 0,
                front: 0,
                right: a.Width.min(b.Width),
                bottom: a.Height.min(b.Height),
                back: 1,
            };
            ctx.CopySubresourceRegion(target, 0, 0, 0, 0, &src, 0, Some(&region));
        }
        Ok(Frame::Copied)
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn window_list_has_titles_and_live_handles() {
        let list = super::windows();
        for (h, title) in &list {
            assert!(!title.trim().is_empty());
            assert!(super::window_rect(*h).is_some(), "{title}: listed window must have a rectangle");
        }
        eprintln!("{} pickable windows: {:?}", list.len(), list.iter().map(|w| &w.1).take(8).collect::<Vec<_>>());
    }
}
