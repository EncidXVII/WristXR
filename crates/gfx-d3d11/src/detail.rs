//! Screens at the detail the headset can show. A captured screen is usually shown much smaller than its pixel
//! count (a 4K desktop a metre wide at arm's length spans perhaps a thousand headset pixels), and SteamVR samples
//! an overlay without mipmaps, so text shimmers and smears. The capture goes into a private texture with a full mip
//! chain; the overlay gets the level that matches its size in view, drawn with a light sharpen that offsets the
//! compositor's own resampling.
use crate::Gpu;
use windows::Win32::Foundation::E_POINTER;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST, ID3DBlob};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_BUFFER_DESC,
    D3D11_COMPARISON_NEVER, D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_RESOURCE_MISC_GENERATE_MIPS,
    D3D11_RESOURCE_MISC_SHARED, D3D11_SAMPLER_DESC, D3D11_TEXTURE_ADDRESS_CLAMP, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11_VIEWPORT, ID3D11Buffer, ID3D11PixelShader, ID3D11RenderTargetView, ID3D11SamplerState,
    ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11VertexShader,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::core::{Error, PCSTR, Result, s};

/// Unsharp amount: the picture plus this share of its difference from its four neighbours' average. Light, so
/// edges firm up without halos.
pub const SHARPEN: f32 = 0.2;
/// A level keeps at least this many of its pixels per headset pixel: a little over one, so text stays crisp.
const MARGIN: f32 = 1.25;

const SHADER: &str = r"
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

/// The mip level to show a `src_w` pixel wide picture at when it spans `needed` headset pixels: the smallest one
/// still at least `MARGIN` times as wide.
pub fn level_for(src_w: u32, needed: f32) -> u32 {
    let mut k = 0;
    while k < 12 && ((src_w >> (k + 1)) as f32) >= needed * MARGIN {
        k += 1;
    }
    k
}

/// The shared pipeline (compiled once).
pub struct Sharpen {
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    sampler: ID3D11SamplerState,
    params: ID3D11Buffer,
}

/// One screen: the capture with its mip chain, and what the overlay shows.
pub struct Detail {
    /// Where the capture copies to (mip 0).
    pub src: ID3D11Texture2D,
    srv: ID3D11ShaderResourceView,
    pub w: u32,
    pub h: u32,
    /// The overlay's texture, at `level`.
    pub out: ID3D11Texture2D,
    rtv: ID3D11RenderTargetView,
    pub level: u32,
}

fn compile(entry: PCSTR, target: PCSTR) -> Result<ID3DBlob> {
    let mut code = None;
    let mut errors = None;
    // SAFETY: the source buffer and names outlive the call; out-params are valid.
    let r = unsafe {
        D3DCompile(
            SHADER.as_ptr().cast(),
            SHADER.len(),
            s!("detail"),
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

fn level_size(n: u32, level: u32) -> u32 {
    (n >> level).max(1)
}

impl Sharpen {
    pub fn new(gpu: &Gpu) -> Result<Self> {
        let (vsb, psb) = (compile(s!("vs"), s!("vs_5_0"))?, compile(s!("ps"), s!("ps_5_0"))?);
        let (mut vs, mut ps, mut sampler, mut params) = (None, None, None, None);
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
        let buffer = D3D11_BUFFER_DESC {
            ByteWidth: 16,
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            ..Default::default()
        };
        // SAFETY: valid bytecode and descriptors; out-params are valid.
        unsafe {
            gpu.device.CreateVertexShader(bytes(&vsb), None, Some(&mut vs))?;
            gpu.device.CreatePixelShader(bytes(&psb), None, Some(&mut ps))?;
            gpu.device.CreateSamplerState(&sampler_desc, Some(&mut sampler))?;
            gpu.device.CreateBuffer(&buffer, None, Some(&mut params))?;
        }
        Ok(Self { vs: out_of(vs)?, ps: out_of(ps)?, sampler: out_of(sampler)?, params: out_of(params)? })
    }

    /// Rebuilds the mip chain after a new capture landed in `d.src`.
    pub fn new_frame(&self, gpu: &Gpu, d: &Detail) {
        // SAFETY: the view belongs to this device and covers every level.
        unsafe { gpu.context.GenerateMips(&d.srv) };
    }

    /// Draws `d.src` at `d.level`, lightly sharpened, into `d.out`.
    pub fn draw(&self, gpu: &Gpu, d: &Detail) {
        let (w, h) = (level_size(d.w, d.level), level_size(d.h, d.level));
        let params: [f32; 4] = [d.level as f32, SHARPEN, 1.0 / w as f32, 1.0 / h as f32];
        let c = &gpu.context;
        // SAFETY: every bound object belongs to this device; `params` matches the 16-byte buffer; the views are
        // unbound again before the textures are next copied to or handed to SteamVR.
        unsafe {
            c.UpdateSubresource(&self.params, 0, None, params.as_ptr().cast(), 0, 0);
            c.IASetInputLayout(None);
            c.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            c.VSSetShader(&self.vs, None);
            c.PSSetShader(&self.ps, None);
            c.PSSetShaderResources(0, Some(&[Some(d.srv.clone())]));
            c.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            c.PSSetConstantBuffers(0, Some(&[Some(self.params.clone())]));
            c.OMSetRenderTargets(Some(&[Some(d.rtv.clone())]), None);
            c.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: w as f32,
                Height: h as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            c.Draw(3, 0);
            c.PSSetShaderResources(0, Some(&[None]));
            c.OMSetRenderTargets(Some(&[None]), None);
        }
    }
}

impl Detail {
    /// A `w` x `h` screen shown at `level`.
    pub fn new(gpu: &Gpu, w: u32, h: u32, level: u32) -> Result<Self> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 0,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: D3D11_RESOURCE_MISC_GENERATE_MIPS.0 as u32,
        };
        let (mut src, mut srv) = (None, None);
        // SAFETY: valid descriptor; out-params are valid.
        unsafe { gpu.device.CreateTexture2D(&desc, None, Some(&mut src))? };
        let src = out_of(src)?;
        // SAFETY: a default view of a texture made on this device.
        unsafe { gpu.device.CreateShaderResourceView(&src, None, Some(&mut srv))? };
        let level = level.min(Self::levels(w, h) - 1);
        let (out, rtv) = Self::output(gpu, level_size(w, level), level_size(h, level))?;
        Ok(Self { src, srv: out_of(srv)?, w, h, out, rtv, level })
    }

    fn levels(w: u32, h: u32) -> u32 {
        32 - w.max(h).max(1).leading_zeros()
    }

    fn output(gpu: &Gpu, w: u32, h: u32) -> Result<(ID3D11Texture2D, ID3D11RenderTargetView)> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
            CPUAccessFlags: 0,
            // SteamVR opens overlay textures through their shared handle (which rules out mips here).
            MiscFlags: D3D11_RESOURCE_MISC_SHARED.0 as u32,
        };
        let (mut tex, mut rtv) = (None, None);
        // SAFETY: valid descriptor; out-params are valid.
        unsafe { gpu.device.CreateTexture2D(&desc, None, Some(&mut tex))? };
        let tex = out_of(tex)?;
        // SAFETY: a default view of a texture made on this device.
        unsafe { gpu.device.CreateRenderTargetView(&tex, None, Some(&mut rtv))? };
        Ok((tex, out_of(rtv)?))
    }

    /// Shows another level from now on: a new overlay texture, to draw before handing it to SteamVR.
    pub fn set_level(&mut self, gpu: &Gpu, level: u32) -> Result<()> {
        let level = level.min(Self::levels(self.w, self.h) - 1);
        let (out, rtv) = Self::output(gpu, level_size(self.w, level), level_size(self.h, level))?;
        (self.out, self.rtv, self.level) = (out, rtv, level);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_level_keeps_a_little_more_detail_than_the_headset_shows() {
        // 3840 wide across ~900 headset pixels: 1920 keeps a margin, 960 would not.
        assert_eq!(level_for(3840, 900.0), 1);
        assert_eq!(level_for(3840, 700.0), 2);
        // Close up, all of it.
        assert_eq!(level_for(3840, 3000.0), 0);
        assert_eq!(level_for(1920, 1600.0), 0);
        // Far away, never past the last level.
        assert!(level_for(3840, 1.0) <= 12);
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
        // SAFETY: test-only readback of a same-size texture; the map covers h rows of RowPitch bytes.
        unsafe {
            gpu.device.CreateTexture2D(&desc, None, Some(&mut staging)).expect("staging");
            let staging = staging.expect("staging");
            gpu.context.CopyResource(&staging, tex);
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

    /// GPU cost of one captured 4K frame through the pass, worst case (full-size level). Prints; run on demand.
    #[test]
    #[ignore = "measures this GPU; run on demand"]
    fn pass_cost_for_a_4k_frame() {
        let gpu = Gpu::new(None).expect("gpu");
        let sharpen = Sharpen::new(&gpu).expect("shaders");
        let d = Detail::new(&gpu, 3840, 2160, 0).expect("detail");
        let mut timer = crate::GpuTimer::new(&gpu).expect("timer");
        for _ in 0..300 {
            timer.begin(&gpu);
            sharpen.new_frame(&gpu, &d);
            sharpen.draw(&gpu, &d);
            timer.end(&gpu);
            gpu.flush();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        println!("detail pass, 4K frame: {:.3} ms GPU", timer.last_ms());
    }

    #[test]
    fn a_far_screen_gets_the_averaged_level_and_flat_colour_stays_flat() {
        let Ok(gpu) = Gpu::new(None) else { return };
        let sharpen = Sharpen::new(&gpu).expect("shaders compile");
        // A one-pixel black and white checker: its first level down is an even grey.
        let (w, h) = (64u32, 32u32);
        let checker: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                let v = if (i % w + i / w) % 2 == 0 { 255 } else { 0 };
                [v, v, v, 255]
            })
            .collect();
        let mut d = Detail::new(&gpu, w, h, 0).expect("detail");
        gpu.update(&d.src, &checker, w);
        d.set_level(&gpu, 1).expect("level");
        sharpen.new_frame(&gpu, &d);
        sharpen.draw(&gpu, &d);
        let px = read(&gpu, &d.out, w / 2, h / 2);
        for p in px.chunks(4) {
            assert!((p[0] as i32 - 128).abs() <= 2 && p[3] == 255, "grey, opaque: {p:?}");
        }
    }
}
