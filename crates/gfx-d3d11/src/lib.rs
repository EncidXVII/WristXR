// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! D3D11 device on SteamVR's adapter and the BGRA textures panels are drawn from.
#![cfg(windows)]
#![allow(unsafe_code)]

mod detail;

pub use detail::{Detail, SHARPEN, Sharpen, keeps, width_for};
use std::ffi::c_void;
use windows::Win32::Foundation::{E_POINTER, HMODULE};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_ASYNC_GETDATA_DONOTFLUSH, D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_QUERY_DATA_TIMESTAMP_DISJOINT, D3D11_QUERY_DESC, D3D11_QUERY_TIMESTAMP, D3D11_QUERY_TIMESTAMP_DISJOINT,
    D3D11_RESOURCE_MISC_SHARED, D3D11_SDK_VERSION, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Query, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter, IDXGIFactory1};
use windows::core::{Error, Interface, Result};
use wxr_core::raster::Image;

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

    /// Replaces a texture's pixels (tightly packed BGRA rows of `width`). For UI, which changes rarely.
    pub fn update(&self, tex: &ID3D11Texture2D, bgra: &[u8], width: u32) {
        // SAFETY: `bgra` covers the whole texture; row pitch matches.
        unsafe { self.context.UpdateSubresource(tex, 0, None, bgra.as_ptr().cast(), width * 4, 0) };
    }

    /// Submits queued GPU work so the compositor sees finished copies.
    pub fn flush(&self) {
        // SAFETY: immediate context owned by this device.
        unsafe { self.context.Flush() };
    }
}

/// GPU names for the self-test report (no identifiers beyond the model name).
pub fn adapter_names() -> Vec<String> {
    // SAFETY: plain factory/adapter enumeration; out-param valid.
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else { return Vec::new() };
        (0..)
            .map_while(|i| factory.EnumAdapters1(i).ok())
            .filter_map(|a| a.GetDesc1().ok())
            .map(|d| String::from_utf16_lossy(&d.Description).trim_end_matches('\0').to_owned())
            .collect()
    }
}

/// Raw `ID3D11Texture2D*` for OpenVR's `Texture_t`.
pub fn raw(tex: &ID3D11Texture2D) -> *mut c_void {
    tex.as_raw()
}

const TIMER_SETS: usize = 4;

/// Measures the GPU time of our own work (captures copies, UI uploads) with timestamp queries.
/// Results are read a few frames later without stalling; `last_ms` is a smoothed per-frame value.
pub struct GpuTimer {
    sets: Vec<[ID3D11Query; 3]>,
    pending: [bool; TIMER_SETS],
    next: usize,
    last_ms: f32,
}

impl GpuTimer {
    pub fn new(gpu: &Gpu) -> Result<Self> {
        let query = |kind| -> Result<ID3D11Query> {
            let mut q = None;
            // SAFETY: valid descriptor and out-param.
            unsafe { gpu.device.CreateQuery(&D3D11_QUERY_DESC { Query: kind, MiscFlags: 0 }, Some(&mut q))? };
            q.ok_or_else(|| Error::from(E_POINTER))
        };
        let sets = (0..TIMER_SETS)
            .map(|_| {
                Ok([
                    query(D3D11_QUERY_TIMESTAMP_DISJOINT)?,
                    query(D3D11_QUERY_TIMESTAMP)?,
                    query(D3D11_QUERY_TIMESTAMP)?,
                ])
            })
            .collect::<Result<_>>()?;
        Ok(Self { sets, pending: [false; TIMER_SETS], next: 0, last_ms: 0.0 })
    }

    /// Smoothed GPU milliseconds per frame spent on our work.
    pub fn last_ms(&self) -> f32 {
        self.last_ms
    }

    pub fn begin(&self, gpu: &Gpu) {
        let [disjoint, start, _] = &self.sets[self.next];
        // SAFETY: queries belong to this device.
        unsafe {
            gpu.context.Begin(disjoint);
            gpu.context.End(start);
        }
    }

    pub fn end(&mut self, gpu: &Gpu) {
        let [disjoint, _, stop] = &self.sets[self.next];
        // SAFETY: queries belong to this device.
        unsafe {
            gpu.context.End(stop);
            gpu.context.End(disjoint);
        }
        self.pending[self.next] = true;
        self.next = (self.next + 1) % TIMER_SETS;
        self.collect(gpu);
    }

    /// Reads finished sets without flushing; unfinished ones stay pending (sentinels stay zero).
    fn collect(&mut self, gpu: &Gpu) {
        for i in (0..TIMER_SETS).map(|k| (self.next + k) % TIMER_SETS) {
            if !self.pending[i] {
                continue;
            }
            let [disjoint, start, stop] = &self.sets[i];
            let mut d = D3D11_QUERY_DATA_TIMESTAMP_DISJOINT::default();
            let (mut t0, mut t1) = (0u64, 0u64);
            let flags = D3D11_ASYNC_GETDATA_DONOTFLUSH.0 as u32;
            // SAFETY: output buffers match each query's data size.
            unsafe {
                let read = |q: &ID3D11Query, p: *mut c_void, n: usize| gpu.context.GetData(q, Some(p), n as u32, flags);
                if read(disjoint, (&raw mut d).cast(), size_of::<D3D11_QUERY_DATA_TIMESTAMP_DISJOINT>()).is_err()
                    || d.Frequency == 0
                {
                    return;
                }
                let _ = read(start, (&raw mut t0).cast(), 8);
                let _ = read(stop, (&raw mut t1).cast(), 8);
            }
            if t0 == 0 || t1 == 0 {
                return;
            }
            self.pending[i] = false;
            if !d.Disjoint.as_bool() && t1 >= t0 {
                let ms = (t1 - t0) as f64 / d.Frequency as f64 * 1000.0;
                self.last_ms = self.last_ms * 0.9 + ms as f32 * 0.1;
            }
        }
    }
}
