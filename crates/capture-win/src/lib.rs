//! Windows.Graphics.Capture: monitor frames stay on the GPU (no CPU readback).
//! WGC only produces frames when content changes; dropping a `Capture` stops it (hidden panel = no cost).
#![cfg(windows)]
#![allow(unsafe_code)]

use dvr_core::panel::PixelRect;
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_TEXTURE2D_DESC, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows::core::{BOOL, Interface, Result, factory};

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
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
}

impl Capture {
    pub fn monitor(device: &ID3D11Device, monitor: HMONITOR) -> Result<Self> {
        let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        // SAFETY: valid monitor handle.
        let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(monitor)? };
        let dxgi: IDXGIDevice = device.cast()?;
        // SAFETY: wraps our D3D11 device for WinRT; no ownership transfer.
        let d3d: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? }.cast()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &d3d,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            item.Size()?,
        )?;
        let session = pool.CreateCaptureSession(&item)?;
        // Windows 11 only; older systems keep the yellow border.
        let _ = session.SetIsBorderRequired(false);
        session.StartCapture()?;
        Ok(Self { _item: item, pool, session })
    }

    /// Copies the newest frame into `target` on the GPU. Returns false when nothing changed.
    pub fn copy_latest(&self, ctx: &ID3D11DeviceContext, target: &ID3D11Texture2D) -> Result<bool> {
        let mut latest = None;
        while let Ok(frame) = self.pool.TryGetNextFrame() {
            latest = Some(frame);
        }
        let Some(frame) = latest else { return Ok(false) };
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
        Ok(true)
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}
