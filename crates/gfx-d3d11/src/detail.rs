// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! Screens at the detail the headset can show. A captured screen is usually shown much smaller than its pixel
//! count (a 4K desktop a metre wide at arm's length spans perhaps a thousand headset pixels), and SteamVR samples
//! an overlay bilinearly, without mipmaps, so text would shimmer and smear. So the overlay gets a picture already at
//! about the size it is shown:
//!
//! - Shrunk by 2.5x or more: the capture's mip chain, then a Kaiser-windowed sinc (a proper low-pass, as SimulaVR
//!   filters its windows) from the nearest level straight to twice the shown width. SteamVR samples the overlay
//!   twice more (into the eye view, then for the lens), which from a picture exactly the shown width would take
//!   about a quarter of small text's detail; from twice it keeps nearly all of it. Letters keep smooth edges and
//!   there is no sharpen (so no halos).
//! - Shrunk less than that (close up): the capture as it is, with a light sharpen that offsets SteamVR's resample.
use crate::Gpu;
use std::cell::Cell;
use windows::Win32::Foundation::E_POINTER;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST, ID3DBlob};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_BOX, D3D11_BUFFER_DESC,
    D3D11_COMPARISON_NEVER, D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_RESOURCE_MISC_GENERATE_MIPS,
    D3D11_RESOURCE_MISC_SHARED, D3D11_SAMPLER_DESC, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE_ADDRESS_CLAMP,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_VIEWPORT, ID3D11Buffer, ID3D11PixelShader, ID3D11RenderTargetView,
    ID3D11SamplerState, ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11VertexShader,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R32_FLOAT, DXGI_SAMPLE_DESC};
use windows::core::{Error, PCSTR, Result, s};

/// Close up: the picture plus this share of its difference from its four neighbours' average. Light, so edges firm
/// up without halos.
pub const SHARPEN: f32 = 0.2;
/// Shrunk by at least this much, a screen goes through the Kaiser resample; less, it is shown as captured (the top of
/// `KEEP`, so the capture as it is never holds more than a resampled picture would).
const RESAMPLE_FROM: f32 = 2.5;
/// Further off, the picture is this many times the width the screen is shown at.
const MARGIN: f32 = 2.0;
/// A picture is kept while it is this many times the shown width (a new one, at `MARGIN`, past either end), so
/// resizing a screen or walking about seldom makes a new texture, and a shrinking screen keeps its detail longer.
const KEEP: std::ops::RangeInclusive<f32> = 1.5..=2.5;
/// Lobes of the windowed sinc on each side, and the Kaiser window's shape (higher is smoother, with less ringing).
const KAISER_LOBES: f32 = 2.0;
const KAISER_BETA: f32 = 4.0;
/// Entries in the weight table, from the centre out to `KAISER_LOBES` output pixels.
const WEIGHTS: usize = 1024;

const SHARPEN_SHADER: &str = r"
Texture2D src : register(t0);
SamplerState lin : register(s0);
cbuffer P : register(b0) { float level; float amount; float2 texel; };
struct V { float4 pos : SV_Position; float2 uv : TEXCOORD0; };
V vs(uint id : SV_VertexID) {
    V o;
    float2 uv = float2((id << 1) & 2, id & 2);
    o.uv = uv;
    o.pos = float4(uv * float2(2, -2) + float2(-1, 1), 0, 1);
    return o;
}
float4 ps(V i) : SV_Target {
    float4 c = src.SampleLevel(lin, i.uv, level);
    float3 n = src.SampleLevel(lin, i.uv + float2(0, -texel.y), level).rgb
             + src.SampleLevel(lin, i.uv + float2(0, texel.y), level).rgb
             + src.SampleLevel(lin, i.uv + float2(-texel.x, 0), level).rgb
             + src.SampleLevel(lin, i.uv + float2(texel.x, 0), level).rgb;
    return float4(saturate(c.rgb + amount * (c.rgb - n * 0.25)), c.a);
}
";

/// One axis of the separable resample: each output pixel weighs the source pixels within `lobes` output pixels of
/// it by a Kaiser-windowed sinc (looked up in `weights`), reading mip `level` of `src`.
const KAISER_SHADER: &str = r"
Texture2D src : register(t0);
Texture2D<float> weights : register(t1);
SamplerState lin : register(s0);
cbuffer K : register(b0) { float scale; float lobes; float axis; float pad0; int2 size; int level; int pad1; };
struct V { float4 pos : SV_Position; float2 uv : TEXCOORD0; };
V vs(uint id : SV_VertexID) {
    V o;
    float2 uv = float2((id << 1) & 2, id & 2);
    o.uv = uv;
    o.pos = float4(uv * float2(2, -2) + float2(-1, 1), 0, 1);
    return o;
}
float4 ps(V i) : SV_Target {
    int2 p = int2(i.pos.xy);
    float support = lobes * max(scale, 1.0);
    float center = ((axis < 0.5) ? (p.x + 0.5) : (p.y + 0.5)) * scale;
    int lo = (int)floor(center - support);
    int hi = (int)ceil(center + support);
    float4 acc = 0;
    float sum = 0;
    [loop] for (int k = lo; k <= hi; k++) {
        float t = abs(k + 0.5 - center) / support;
        if (t >= 1.0) continue;
        float w = weights.SampleLevel(lin, float2(t, 0.5), 0);
        int2 q = (axis < 0.5) ? int2(clamp(k, 0, size.x - 1), p.y) : int2(p.x, clamp(k, 0, size.y - 1));
        acc += w * src.Load(int3(q, level));
        sum += w;
    }
    return saturate(acc / sum);
}
";

/// The width to show a `src_w` pixel wide picture at when it spans `needed` headset pixels: the full width close up
/// (shrunk less than `RESAMPLE_FROM`), otherwise `MARGIN` times `needed`.
pub fn width_for(src_w: u32, needed: f32) -> u32 {
    let needed = needed.max(1.0);
    if needed * RESAMPLE_FROM > src_w as f32 {
        return src_w;
    }
    ((needed * MARGIN).round() as u32).clamp(1, src_w.max(1))
}

/// True while a picture `current` wide still suits a screen spanning `needed` headset pixels: keep it, no new
/// texture.
pub fn keeps(current: u32, needed: f32) -> bool {
    KEEP.contains(&(current as f32 / needed.max(1.0)))
}

/// The mip level the resample reads for an output `out_w` wide: the smallest level still at least that wide, so it
/// shrinks by under 2x.
fn source_level(src_w: u32, out_w: u32) -> u32 {
    let mut k = 0;
    while k < 12 && (src_w >> (k + 1)) >= out_w {
        k += 1;
    }
    k
}

fn level_size(n: u32, level: u32) -> u32 {
    (n >> level).max(1)
}

fn compile(source: &str, name: PCSTR, entry: PCSTR, target: PCSTR) -> Result<ID3DBlob> {
    let mut code = None;
    let mut errors = None;
    // SAFETY: the source buffer and names outlive the call; out-params are valid.
    let r = unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            name,
            None,
            None,
            entry,
            target,
            0,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if let Err(e) = r {
        if let Some(b) = errors {
            tracing::warn!(msg = %String::from_utf8_lossy(bytes(&b)), "shader compile failed");
        }
        return Err(e);
    }
    out_of(code)
}

fn bytes(b: &ID3DBlob) -> &[u8] {
    // SAFETY: the blob owns this many bytes for its lifetime.
    unsafe { std::slice::from_raw_parts(b.GetBufferPointer().cast::<u8>(), b.GetBufferSize()) }
}

fn out_of<T>(v: Option<T>) -> Result<T> {
    v.ok_or_else(|| Error::from(E_POINTER))
}

/// The modified Bessel function I0 (series), for the Kaiser window.
fn bessel0(x: f64) -> f64 {
    let (mut sum, mut term, q) = (1.0, 1.0, x * x / 4.0);
    for k in 1..20 {
        term *= q / f64::from(k * k);
        sum += term;
    }
    sum
}

/// sinc(x) times the Kaiser window, for x = t * lobes, t in 0..=1.
fn weight_table() -> Vec<f32> {
    let (lobes, beta) = (f64::from(KAISER_LOBES), f64::from(KAISER_BETA));
    (0..WEIGHTS)
        .map(|i| {
            let t = i as f64 / (WEIGHTS - 1) as f64;
            let x = std::f64::consts::PI * t * lobes;
            let sinc = if x.abs() < 1e-9 { 1.0 } else { x.sin() / x };
            (sinc * bessel0(beta * (1.0 - t * t).max(0.0).sqrt()) / bessel0(beta)) as f32
        })
        .collect()
}

fn texture(gpu: &Gpu, w: u32, h: u32, mips: bool, shared: bool) -> Result<ID3D11Texture2D> {
    let mut misc = 0;
    if mips {
        misc |= D3D11_RESOURCE_MISC_GENERATE_MIPS.0 as u32;
    }
    // SteamVR opens overlay textures through their shared handle (which rules out mips there).
    if shared {
        misc |= D3D11_RESOURCE_MISC_SHARED.0 as u32;
    }
    let desc = D3D11_TEXTURE2D_DESC {
        Width: w,
        Height: h,
        MipLevels: if mips { 0 } else { 1 },
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
        CPUAccessFlags: 0,
        MiscFlags: misc,
    };
    let mut tex = None;
    // SAFETY: valid descriptor; out-param is valid.
    unsafe { gpu.device.CreateTexture2D(&desc, None, Some(&mut tex))? };
    out_of(tex)
}

fn rtv(gpu: &Gpu, tex: &ID3D11Texture2D) -> Result<ID3D11RenderTargetView> {
    let mut v = None;
    // SAFETY: a default view of a texture made on this device.
    unsafe { gpu.device.CreateRenderTargetView(tex, None, Some(&mut v))? };
    out_of(v)
}

fn srv(gpu: &Gpu, tex: &ID3D11Texture2D) -> Result<ID3D11ShaderResourceView> {
    let mut v = None;
    // SAFETY: a default view of a texture made on this device.
    unsafe { gpu.device.CreateShaderResourceView(tex, None, Some(&mut v))? };
    out_of(v)
}

/// The shared pipelines (compiled once): the close-up sharpen and the Kaiser resample, with its weight table.
pub struct Sharpen {
    vs: ID3D11VertexShader,
    sharpen: ID3D11PixelShader,
    kaiser: ID3D11PixelShader,
    sampler: ID3D11SamplerState,
    sharpen_params: ID3D11Buffer,
    kaiser_params: ID3D11Buffer,
    weights: ID3D11ShaderResourceView,
}

/// The resample's half-done (horizontal) picture.
struct Across {
    level: u32,
    rtv: ID3D11RenderTargetView,
    srv: ID3D11ShaderResourceView,
}

/// One screen: the capture with its mip chain, and what the overlay shows.
pub struct Detail {
    /// Where the capture copies to (mip 0).
    pub src: ID3D11Texture2D,
    srv: ID3D11ShaderResourceView,
    pub w: u32,
    pub h: u32,
    /// The overlay's texture, the capture's size for good: the picture fills its top-left `width` x `out_h`
    /// (`bounds`), so a new width never hands SteamVR a new texture (it showed one as a blink).
    pub out: ID3D11Texture2D,
    rtv: ID3D11RenderTargetView,
    pub width: u32,
    pub out_h: u32,
    /// Set when `width` is below the capture's: the picture goes through the resample.
    across: Option<Across>,
    /// The mip chain is older than the capture (rebuilt by the next draw that reads it).
    mips_stale: Cell<bool>,
}

impl Sharpen {
    pub fn new(gpu: &Gpu) -> Result<Self> {
        let vsb = compile(SHARPEN_SHADER, s!("detail"), s!("vs"), s!("vs_5_0"))?;
        let sharpen_b = compile(SHARPEN_SHADER, s!("detail"), s!("ps"), s!("ps_5_0"))?;
        let kaiser_b = compile(KAISER_SHADER, s!("kaiser"), s!("ps"), s!("ps_5_0"))?;
        let (mut vs, mut sharpen, mut kaiser, mut sampler) = (None, None, None, None);
        let (mut sharpen_params, mut kaiser_params, mut table, mut weights) = (None, None, None, None);
        let sampler_desc = D3D11_SAMPLER_DESC {
            Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
            AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
            MaxAnisotropy: 1,
            ComparisonFunc: D3D11_COMPARISON_NEVER,
            MaxLOD: f32::MAX,
            ..Default::default()
        };
        let buffer = |bytes: u32| D3D11_BUFFER_DESC {
            ByteWidth: bytes,
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            ..Default::default()
        };
        let lut = weight_table();
        let lut_desc = D3D11_TEXTURE2D_DESC {
            Width: WEIGHTS as u32,
            Height: 1,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R32_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            ..Default::default()
        };
        let init = D3D11_SUBRESOURCE_DATA {
            pSysMem: lut.as_ptr().cast(),
            SysMemPitch: (WEIGHTS * 4) as u32,
            SysMemSlicePitch: 0,
        };
        // SAFETY: valid bytecode, descriptors and initial data (`lut` outlives the call); out-params are valid.
        unsafe {
            gpu.device.CreateVertexShader(bytes(&vsb), None, Some(&mut vs))?;
            gpu.device.CreatePixelShader(bytes(&sharpen_b), None, Some(&mut sharpen))?;
            gpu.device.CreatePixelShader(bytes(&kaiser_b), None, Some(&mut kaiser))?;
            gpu.device.CreateSamplerState(&sampler_desc, Some(&mut sampler))?;
            gpu.device.CreateBuffer(&buffer(16), None, Some(&mut sharpen_params))?;
            gpu.device.CreateBuffer(&buffer(32), None, Some(&mut kaiser_params))?;
            gpu.device.CreateTexture2D(&lut_desc, Some(&init), Some(&mut table))?;
            gpu.device.CreateShaderResourceView(&out_of(table)?, None, Some(&mut weights))?;
        }
        Ok(Self {
            vs: out_of(vs)?,
            sharpen: out_of(sharpen)?,
            kaiser: out_of(kaiser)?,
            sampler: out_of(sampler)?,
            sharpen_params: out_of(sharpen_params)?,
            kaiser_params: out_of(kaiser_params)?,
            weights: out_of(weights)?,
        })
    }

    /// A new capture landed in `d.src`: its mip chain is rebuilt when a draw next reads it. Not here, since the
    /// picture's width (and so whether it reads a mip level) can change before the next capture.
    pub fn new_frame(&self, _gpu: &Gpu, d: &Detail) {
        d.mips_stale.set(true);
    }

    /// One full-screen triangle from `from` into `to` (`size` pixels), with `shader` and its constants.
    #[allow(clippy::too_many_arguments)]
    fn run(
        &self,
        gpu: &Gpu,
        shader: &ID3D11PixelShader,
        params: &ID3D11Buffer,
        constants: &[u8],
        from: &ID3D11ShaderResourceView,
        to: &ID3D11RenderTargetView,
        size: (u32, u32),
    ) {
        let c = &gpu.context;
        // SAFETY: every bound object belongs to this device; `constants` matches its buffer's size; the views are
        // unbound again before the textures are next copied to or handed to SteamVR.
        unsafe {
            c.UpdateSubresource(params, 0, None, constants.as_ptr().cast(), 0, 0);
            c.IASetInputLayout(None);
            c.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            c.VSSetShader(&self.vs, None);
            c.PSSetShader(shader, None);
            c.PSSetShaderResources(0, Some(&[Some(from.clone()), Some(self.weights.clone())]));
            c.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            c.PSSetConstantBuffers(0, Some(&[Some(params.clone())]));
            c.OMSetRenderTargets(Some(&[Some(to.clone())]), None);
            c.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: size.0 as f32,
                Height: size.1 as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            c.Draw(3, 0);
            c.PSSetShaderResources(0, Some(&[None, None]));
            c.OMSetRenderTargets(Some(&[None]), None);
        }
    }

    /// One axis of the resample: `from` (mip `level`, `size` pixels) into `to`, `out` pixels.
    #[allow(clippy::too_many_arguments)]
    fn resample(
        &self,
        gpu: &Gpu,
        from: &ID3D11ShaderResourceView,
        level: u32,
        size: (u32, u32),
        to: &ID3D11RenderTargetView,
        out: (u32, u32),
        down: bool,
    ) {
        let scale = if down { size.1 as f32 / out.1 as f32 } else { size.0 as f32 / out.0 as f32 };
        let mut k = [0u8; 32];
        for (i, v) in [scale, KAISER_LOBES, f32::from(u8::from(down)), 0.0].iter().enumerate() {
            k[i * 4..i * 4 + 4].copy_from_slice(&v.to_ne_bytes());
        }
        for (i, v) in [size.0 as i32, size.1 as i32, level as i32, 0].iter().enumerate() {
            k[16 + i * 4..20 + i * 4].copy_from_slice(&v.to_ne_bytes());
        }
        self.run(gpu, &self.kaiser, &self.kaiser_params, &k, from, to, out);
    }

    /// Draws `d.src` into `d.out`: resampled to `d.width`, or close up, sharpened as it is.
    pub fn draw(&self, gpu: &Gpu, d: &Detail) {
        match &d.across {
            Some(a) => {
                if a.level > 0 && d.mips_stale.replace(false) {
                    // SAFETY: the view belongs to this device and covers every level.
                    unsafe { gpu.context.GenerateMips(&d.srv) };
                }
                let (lw, lh) = (level_size(d.w, a.level), level_size(d.h, a.level));
                self.resample(gpu, &d.srv, a.level, (lw, lh), &a.rtv, (d.width, lh), false);
                self.resample(gpu, &a.srv, 0, (d.width, lh), &d.rtv, (d.width, d.out_h), true);
                // SteamVR's bilinear sample at the picture's right and bottom edge reaches one texel past it: that
                // texel repeats the edge, as a texture's own edge would.
                let copy = |(x, y): (u32, u32), (left, top, right, bottom): (u32, u32, u32, u32)| {
                    let b = D3D11_BOX { left, top, front: 0, right, bottom, back: 1 };
                    // SAFETY: both regions lie inside the texture and do not overlap.
                    unsafe { gpu.context.CopySubresourceRegion(&d.out, 0, x, y, 0, &d.out, 0, Some(&b)) };
                };
                if d.width < d.w {
                    copy((d.width, 0), (d.width - 1, 0, d.width, d.out_h));
                }
                if d.out_h < d.h {
                    copy((0, d.out_h), (0, d.out_h - 1, (d.width + 1).min(d.w), d.out_h));
                }
            }
            None => {
                let mut p = [0u8; 16];
                for (i, v) in [0.0, SHARPEN, 1.0 / d.w as f32, 1.0 / d.h as f32].iter().enumerate() {
                    p[i * 4..i * 4 + 4].copy_from_slice(&v.to_ne_bytes());
                }
                self.run(gpu, &self.sharpen, &self.sharpen_params, &p, &d.srv, &d.rtv, (d.w, d.h));
            }
        }
    }
}

impl Detail {
    /// A `w` x `h` screen shown `width` wide (`w` for as captured).
    pub fn new(gpu: &Gpu, w: u32, h: u32, width: u32) -> Result<Self> {
        let src = texture(gpu, w, h, true, false)?;
        let srv = srv(gpu, &src)?;
        let out = texture(gpu, w, h, false, true)?;
        let rtv = rtv(gpu, &out)?;
        let (width, out_h, across) = Self::picture(gpu, w, h, width)?;
        Ok(Self { src, srv, w, h, out, rtv, width, out_h, across, mips_stale: Cell::new(true) })
    }

    /// The picture's size, and the resample's half-done texture when it goes through one.
    fn picture(gpu: &Gpu, w: u32, h: u32, width: u32) -> Result<(u32, u32, Option<Across>)> {
        let width = width.clamp(1, w.max(1));
        if width >= w {
            return Ok((w, h, None));
        }
        let out_h = ((width as u64 * h as u64).div_ceil(w as u64) as u32).max(1);
        let level = source_level(w, width);
        let mid = texture(gpu, width, level_size(h, level), false, false)?;
        Ok((width, out_h, Some(Across { level, rtv: rtv(gpu, &mid)?, srv: srv(gpu, &mid)? })))
    }

    /// Shows the screen `width` wide from now on, in the same overlay texture: draw, then hand SteamVR the new
    /// `bounds`.
    pub fn set_width(&mut self, gpu: &Gpu, width: u32) -> Result<()> {
        (self.width, self.out_h, self.across) = Self::picture(gpu, self.w, self.h, width)?;
        Ok(())
    }

    /// The part of `out` the picture fills, as SteamVR's texture bounds: u and v, min then max.
    pub fn bounds(&self) -> [f32; 4] {
        [0.0, 0.0, self.width as f32 / self.w as f32, self.out_h as f32 / self.h as f32]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_up_keeps_the_capture_and_further_off_gets_twice_the_shown_width() {
        // Shrunk less than 2.5x: as captured.
        assert_eq!(width_for(3840, 3000.0), 3840);
        assert_eq!(width_for(3840, 1537.0), 3840);
        // Further: twice the shown width.
        assert_eq!(width_for(3840, 1536.0), 3072);
        assert_eq!(width_for(3840, 1000.0), 2000);
        assert_eq!(width_for(3840, 640.4), 1281);
        assert!(width_for(3840, 0.0) >= 1);
        // A picture stays from 1.5x to 2.5x the shown width; past that a new one is made.
        assert!(keeps(2000, 1000.0) && keeps(2000, 1333.0) && keeps(2000, 800.0));
        assert!(!keeps(2000, 1400.0) && !keeps(2000, 790.0));
        // Close up the full capture stays (width_for asks for it again).
        assert_eq!(width_for(3840, 2500.0), 3840);
    }

    #[test]
    fn the_resample_reads_the_smallest_level_still_as_wide_as_its_output() {
        assert_eq!(source_level(3840, 1900), 1);
        assert_eq!(source_level(3840, 1000), 1);
        assert_eq!(source_level(3840, 960), 2);
        assert_eq!(source_level(3840, 3000), 0);
    }

    #[test]
    fn the_weights_are_a_low_pass_with_its_first_zero_one_output_pixel_out() {
        let w = weight_table();
        assert!((w[0] - 1.0).abs() < 1e-6);
        let one = (WEIGHTS - 1) / KAISER_LOBES as usize;
        assert!(w[one].abs() < 1e-3, "zero at one pixel: {}", w[one]);
        assert!(w[one / 2] > 0.5 && w[WEIGHTS - 1].abs() < 1e-3);
    }

    /// Reads a texture back (BGRA rows).
    fn read(gpu: &Gpu, tex: &ID3D11Texture2D, w: u32, h: u32) -> Vec<u8> {
        use windows::Win32::Graphics::Direct3D11::{D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_USAGE_STAGING};
        let desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            ..Default::default()
        };
        let mut staging = None;
        let mut px = vec![0u8; (w * h * 4) as usize];
        // SAFETY: test-only readback of a texture's top-left w x h; the map covers h rows of RowPitch bytes.
        unsafe {
            gpu.device.CreateTexture2D(&desc, None, Some(&mut staging)).expect("staging");
            let staging = staging.expect("staging");
            let b = D3D11_BOX { left: 0, top: 0, front: 0, right: w, bottom: h, back: 1 };
            gpu.context.CopySubresourceRegion(&staging, 0, 0, 0, 0, tex, 0, Some(&b));
            let mut m = Default::default();
            gpu.context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut m)).expect("map");
            for y in 0..h as usize {
                let row = std::slice::from_raw_parts(m.pData.cast::<u8>().add(y * m.RowPitch as usize), w as usize * 4);
                px[y * w as usize * 4..][..w as usize * 4].copy_from_slice(row);
            }
            gpu.context.Unmap(&staging, 0);
        }
        px
    }

    #[test]
    fn a_screen_moved_away_between_captures_still_shows_the_capture() {
        let Ok(gpu) = Gpu::new(None) else { return };
        let pass = Sharpen::new(&gpu).expect("shaders compile");
        // Close up (the capture as it is), one frame, then moved far off before the screen changes again: the new
        // picture reads a mip level, which must hold that frame, not an empty or older one.
        let (w, h) = (96u32, 48u32);
        let d = &mut Detail::new(&gpu, w, h, w).expect("detail");
        gpu.update(&d.src, &[200u8, 200, 200, 255].repeat((w * h) as usize), w);
        pass.new_frame(&gpu, d);
        pass.draw(&gpu, d);
        d.set_width(&gpu, 12).expect("narrower");
        pass.draw(&gpu, d);
        for p in read(&gpu, &d.out, d.width, d.out_h).chunks(4) {
            assert!((p[0] as i32 - 200).abs() <= 2, "the capture, not a stale level: {p:?}");
        }
    }

    #[test]
    fn a_new_detail_level_keeps_the_texture_and_says_which_part_it_fills() {
        let Ok(gpu) = Gpu::new(None) else { return };
        let pass = Sharpen::new(&gpu).expect("shaders compile");
        let (w, h) = (96u32, 48u32);
        let d = &mut Detail::new(&gpu, w, h, w).expect("detail");
        let first = windows::core::Interface::as_raw(&d.out);
        assert_eq!(d.bounds(), [0.0, 0.0, 1.0, 1.0]);
        gpu.update(&d.src, &[90u8, 90, 90, 255].repeat((w * h) as usize), w);
        pass.new_frame(&gpu, d);
        d.set_width(&gpu, 24).expect("narrower");
        pass.draw(&gpu, d);
        assert_eq!(
            windows::core::Interface::as_raw(&d.out),
            first,
            "SteamVR keeps the texture it has (a new one showed as a blink)"
        );
        assert_eq!(d.bounds(), [0.0, 0.0, 0.25, 0.25]);
        // The texel past the picture's right and bottom edge repeats it, for SteamVR's bilinear sample there.
        let px = read(&gpu, &d.out, d.width + 1, d.out_h + 1);
        let at = |x: u32, y: u32| px[((y * (d.width + 1) + x) * 4) as usize];
        assert_eq!(
            (at(d.width, 3), at(5, d.out_h), at(d.width, d.out_h)),
            (at(d.width - 1, 3), at(5, d.out_h - 1), 90)
        );
        gpu.finish();
    }

    #[test]
    fn a_far_screen_is_resampled_and_flat_colour_stays_flat() {
        let Ok(gpu) = Gpu::new(None) else { return };
        let pass = Sharpen::new(&gpu).expect("shaders compile");
        // A one-pixel black and white checker, shown at a third of its width: an even grey.
        let (w, h) = (96u32, 48u32);
        let checker: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                let v = if (i % w + i / w) % 2 == 0 { 255 } else { 0 };
                [v, v, v, 255]
            })
            .collect();
        let d = Detail::new(&gpu, w, h, 32).expect("detail");
        assert_eq!((d.width, d.out_h), (32, 16));
        gpu.update(&d.src, &checker, w);
        pass.new_frame(&gpu, &d);
        pass.draw(&gpu, &d);
        let px = read(&gpu, &d.out, 32, 16);
        for p in px.chunks(4) {
            assert!((p[0] as i32 - 128).abs() <= 3 && p[3] == 255, "grey, opaque: {p:?}");
        }
    }

    /// What the overlay gets for a picture (`WXR_TEXT_SRC`, BGRA, `WXR_TEXT_W` x `WXR_TEXT_H`) shown 1000 and 640
    /// pixels wide, written to `WXR_TEXT_OUT`, for looking at side by side.
    #[test]
    #[ignore = "writes comparison images; run on demand"]
    fn text_through_the_pass() {
        let var = |k: &str| std::env::var(k).expect(k);
        let (w, h): (u32, u32) = (var("WXR_TEXT_W").parse().expect("w"), var("WXR_TEXT_H").parse().expect("h"));
        let px = std::fs::read(var("WXR_TEXT_SRC")).expect("source");
        let out = std::path::PathBuf::from(var("WXR_TEXT_OUT"));
        let gpu = Gpu::new(None).expect("gpu");
        let pass = Sharpen::new(&gpu).expect("shaders");
        for shown in [1000u32, 640] {
            let d = Detail::new(&gpu, w, h, width_for(w, shown as f32)).expect("detail");
            gpu.update(&d.src, &px, w);
            pass.new_frame(&gpu, &d);
            pass.draw(&gpu, &d);
            let name = format!("pass-{shown}-{}x{}.bgra", d.width, d.out_h);
            std::fs::write(out.join(name), read(&gpu, &d.out, d.width, d.out_h)).expect("write");
        }
    }

    /// GPU time for one captured 4K frame through the pass, at several distances. Prints; run on demand.
    #[test]
    #[ignore = "measures this GPU; run on demand"]
    fn pass_cost_for_a_4k_frame() {
        let gpu = Gpu::new(None).expect("gpu");
        let pass = Sharpen::new(&gpu).expect("shaders");
        let mut timer = crate::GpuTimer::new(&gpu).expect("timer");
        for shown in [3000u32, 1900, 1000, 640, 400] {
            let d = Detail::new(&gpu, 3840, 2160, width_for(3840, shown as f32)).expect("detail");
            let mut ms = Vec::new();
            for i in 0..400 {
                timer.begin(&gpu);
                pass.new_frame(&gpu, &d);
                pass.draw(&gpu, &d);
                timer.end(&gpu);
                gpu.flush();
                std::thread::sleep(std::time::Duration::from_millis(1));
                if i >= 100 {
                    ms.push(timer.last_ms());
                }
            }
            ms.sort_by(f32::total_cmp);
            println!("shown {shown:>4} px -> {} px wide: {:.3} ms GPU", d.width, ms[ms.len() / 2]);
        }
    }
}
