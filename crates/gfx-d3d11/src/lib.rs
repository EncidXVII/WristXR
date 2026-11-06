//! D3D11 device on SteamVR's adapter and the BGRA textures panels are drawn from.
#![cfg(windows)]
#![allow(unsafe_code)]

use dvr_core::raster::Image;
use std::ffi::c_void;
use windows::Win32::Foundation::{E_POINTER, HMODULE};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_RESOURCE_MISC_SHARED, D3D11_SDK_VERSION,
    D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11CreateDevice, ID3D11Device,
    ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter, IDXGIFactory1};
use windows::core::{Error, Interface, Result};

pub struct Gpu {
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
}

impl Gpu {
    /// `adapter_index`: SteamVR's DXGI adapter (OpenVR requires textures from it).
    pub fn new(adapter_index: Option<u32>) -> Result<Self> {
        let adapter: Option<IDXGIAdapter> = match adapter_index {
            Some(i) => {
                // SAFETY: plain factory/adapter enumeration.
                let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1()? };
                Some(unsafe { factory.EnumAdapters1(i)? }.cast()?)
            }
            None => None,
        };
        let driver = if adapter.is_some() { D3D_DRIVER_TYPE_UNKNOWN } else { D3D_DRIVER_TYPE_HARDWARE };
        let (mut device, mut context) = (None, None);
        // SAFETY: out-pointers are valid for the call.
        unsafe {
            D3D11CreateDevice(
                adapter.as_ref(),
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
        }
        Ok(Self {
            device: device.ok_or_else(|| Error::from(E_POINTER))?,
            context: context.ok_or_else(|| Error::from(E_POINTER))?,
        })
    }

    /// BGRA8 texture, optionally initialized from `bgra` (tightly packed rows).
    pub fn texture(&self, w: u32, h: u32, bgra: Option<&[u8]>) -> Result<ID3D11Texture2D> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: D3D11_RESOURCE_MISC_SHARED.0 as u32,
        };
        let init = bgra.map(|d| D3D11_SUBRESOURCE_DATA {
            pSysMem: d.as_ptr().cast(),
            SysMemPitch: w * 4,
            SysMemSlicePitch: 0,
        });
        let mut tex = None;
        // SAFETY: `init` (if any) points at w*h*4 bytes that outlive the call.
        unsafe { self.device.CreateTexture2D(&desc, init.as_ref().map(|i| i as *const _), Some(&mut tex))? };
        tex.ok_or_else(|| Error::from(E_POINTER))
    }

    pub fn image(&self, img: &Image) -> Result<ID3D11Texture2D> {
        self.texture(img.w, img.h, Some(&img.bgra))
    }

    /// Submits queued GPU work so the compositor sees finished copies.
    pub fn flush(&self) {
        // SAFETY: immediate context owned by this device.
        unsafe { self.context.Flush() };
    }
}

/// Raw `ID3D11Texture2D*` for OpenVR's `Texture_t`.
pub fn raw(tex: &ID3D11Texture2D) -> *mut c_void {
    tex.as_raw()
}
