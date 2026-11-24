// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! PipeWire, loaded at run time (`libpipewire-0.3.so.0`), so building needs no PipeWire headers and the app still
//! runs on X11 machines without it. Only what screen capture needs: one connection on the portal's remote, and one
//! stream per shared monitor or window that keeps the newest frame.
//!
//! Frames come in shared memory, copied once into the newest frame, cropped to what the compositor marks as the
//! picture and skipped when it marks them corrupted. With DMA-BUF modifiers set (`set_dmabuf_modifiers`), a stream
//! first offers GPU buffers: the frame is then the buffer's fd, which the GPU copies with no pass through the CPU,
//! and the buffer is held until the next one arrives. If the GPU cannot import it, `Stream::shared_memory_only`
//! asks for shared memory again.
//!
//! The layouts and numbers below are PipeWire's and SPA's stable C ABI (pipewire/stream.h, spa/buffer/buffer.h,
//! spa/buffer/meta.h, spa/param/buffers.h, spa/pod/pod.h, spa/param/format.h, spa/param/video/raw.h).
use std::ffi::{CStr, c_char, c_int, c_void};
use std::os::fd::{BorrowedFd, IntoRawFd, OwnedFd};
use std::sync::{Arc, Mutex, OnceLock};

// --- SPA / PipeWire numbers ----------------------------------------------------------------------------------------
const SPA_TYPE_ID: u32 = 3;
const SPA_TYPE_INT: u32 = 4;
const SPA_TYPE_LONG: u32 = 5;
const SPA_TYPE_RECTANGLE: u32 = 10;
const SPA_TYPE_FRACTION: u32 = 11;
const SPA_TYPE_OBJECT: u32 = 15;
const SPA_TYPE_CHOICE: u32 = 19;
const SPA_TYPE_OBJECT_FORMAT: u32 = 0x40003;
const SPA_TYPE_OBJECT_PARAM_BUFFERS: u32 = 0x40004;
const SPA_TYPE_OBJECT_PARAM_META: u32 = 0x40005;
const SPA_PARAM_ENUM_FORMAT: u32 = 3;
const SPA_PARAM_FORMAT: u32 = 4;
const SPA_PARAM_BUFFERS: u32 = 5;
const SPA_PARAM_META: u32 = 6;
const SPA_PARAM_BUFFERS_DATA_TYPE: u32 = 6;
const SPA_PARAM_META_TYPE: u32 = 1;
const SPA_PARAM_META_SIZE: u32 = 2;
const SPA_META_HEADER: u32 = 1;
const SPA_META_VIDEO_CROP: u32 = 2;
const SPA_META_HEADER_FLAG_CORRUPTED: u32 = 1 << 1;
const SPA_CHUNK_FLAG_CORRUPTED: i32 = 1 << 0;
const SPA_DATA_MEM_PTR: u32 = 1;
const SPA_DATA_MEM_FD: u32 = 2;
const SPA_DATA_DMA_BUF: u32 = 3;
const SPA_POD_PROP_FLAG_MANDATORY: u32 = 1 << 3;
const SPA_POD_PROP_FLAG_DONT_FIXATE: u32 = 1 << 4;
const SPA_CHOICE_NONE: u32 = 0;
const SPA_CHOICE_RANGE: u32 = 1;
const SPA_CHOICE_ENUM: u32 = 3;
const SPA_CHOICE_FLAGS: u32 = 4;
const SPA_FORMAT_MEDIA_TYPE: u32 = 1;
const SPA_FORMAT_MEDIA_SUBTYPE: u32 = 2;
const SPA_FORMAT_VIDEO_FORMAT: u32 = 0x20001;
const SPA_FORMAT_VIDEO_MODIFIER: u32 = 0x20002;
const SPA_FORMAT_VIDEO_SIZE: u32 = 0x20003;
const SPA_FORMAT_VIDEO_FRAMERATE: u32 = 0x20004;
const SPA_MEDIA_TYPE_VIDEO: u32 = 2;
const SPA_MEDIA_SUBTYPE_RAW: u32 = 1;
const VIDEO_RGBX: u32 = 7;
const VIDEO_BGRX: u32 = 8;
const VIDEO_RGBA: u32 = 11;
const VIDEO_BGRA: u32 = 12;
const PW_DIRECTION_INPUT: u32 = 0;
const PW_STREAM_FLAG_AUTOCONNECT: u32 = 1 << 0;
const PW_STREAM_FLAG_MAP_BUFFERS: u32 = 1 << 2;
const PW_VERSION_STREAM_EVENTS: u32 = 2;

#[repr(C)]
struct SpaHook {
    _fields: [*mut c_void; 6],
}

#[repr(C)]
struct SpaChunk {
    offset: u32,
    size: u32,
    stride: i32,
    flags: i32,
}

#[repr(C)]
struct SpaData {
    kind: u32,
    flags: u32,
    fd: i64,
    mapoffset: u32,
    maxsize: u32,
    data: *mut c_void,
    chunk: *mut SpaChunk,
}

#[repr(C)]
struct SpaMeta {
    kind: u32,
    size: u32,
    data: *mut c_void,
}

#[repr(C)]
struct SpaMetaHeader {
    flags: u32,
    offset: u32,
    pts: i64,
    dts_offset: i64,
    seq: u64,
}

/// `spa_meta_region`: a position and a size.
#[repr(C)]
struct SpaRegion {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

#[repr(C)]
struct SpaBuffer {
    n_metas: u32,
    n_datas: u32,
    metas: *mut SpaMeta,
    datas: *mut SpaData,
}

#[repr(C)]
struct PwBuffer {
    buffer: *mut SpaBuffer,
}

type Cb = Option<unsafe extern "C" fn(*mut c_void)>;

#[repr(C)]
struct StreamEvents {
    version: u32,
    destroy: Cb,
    state_changed: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int, *const c_char)>,
    control_info: Option<unsafe extern "C" fn(*mut c_void, u32, *const c_void)>,
    io_changed: Option<unsafe extern "C" fn(*mut c_void, u32, *mut c_void, u32)>,
    param_changed: Option<unsafe extern "C" fn(*mut c_void, u32, *const u8)>,
    add_buffer: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
    remove_buffer: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
    process: Cb,
    drained: Cb,
    command: Option<unsafe extern "C" fn(*mut c_void, *const c_void)>,
    trigger_done: Cb,
}

/// The library's entry points we use.
struct Lib {
    init: unsafe extern "C" fn(*mut c_int, *mut *mut *mut c_char),
    thread_loop_new: unsafe extern "C" fn(*const c_char, *const c_void) -> *mut c_void,
    thread_loop_start: unsafe extern "C" fn(*mut c_void) -> c_int,
    thread_loop_lock: unsafe extern "C" fn(*mut c_void),
    thread_loop_unlock: unsafe extern "C" fn(*mut c_void),
    thread_loop_get_loop: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    context_new: unsafe extern "C" fn(*mut c_void, *mut c_void, usize) -> *mut c_void,
    context_connect_fd: unsafe extern "C" fn(*mut c_void, c_int, *mut c_void, usize) -> *mut c_void,
    properties_new_string: unsafe extern "C" fn(*const c_char) -> *mut c_void,
    stream_new: unsafe extern "C" fn(*mut c_void, *const c_char, *mut c_void) -> *mut c_void,
    stream_add_listener: unsafe extern "C" fn(*mut c_void, *mut SpaHook, *const StreamEvents, *mut c_void),
    stream_connect: unsafe extern "C" fn(*mut c_void, u32, u32, u32, *mut *const u8, u32) -> c_int,
    stream_dequeue_buffer: unsafe extern "C" fn(*mut c_void) -> *mut PwBuffer,
    stream_queue_buffer: unsafe extern "C" fn(*mut c_void, *mut PwBuffer) -> c_int,
    stream_destroy: unsafe extern "C" fn(*mut c_void),
    stream_update_params: unsafe extern "C" fn(*mut c_void, *mut *const u8, u32) -> c_int,
}

#[allow(unsafe_code)]
// Each symbol becomes the type of the `Lib` field it fills: the C prototype from the PipeWire headers.
#[allow(clippy::missing_transmute_annotations)]
fn load() -> Option<Lib> {
    // SAFETY: dlopen/dlsym on a system library; each symbol is cast to its C prototype from the PipeWire headers.
    unsafe {
        let lib = libc::dlopen(c"libpipewire-0.3.so.0".as_ptr(), libc::RTLD_NOW);
        if lib.is_null() {
            return None;
        }
        macro_rules! sym {
            ($name:literal) => {{
                let p = libc::dlsym(lib, CStr::from_bytes_with_nul($name.as_bytes()).ok()?.as_ptr());
                if p.is_null() {
                    return None;
                }
                std::mem::transmute::<*mut c_void, _>(p)
            }};
        }
        Some(Lib {
            init: sym!("pw_init\0"),
            thread_loop_new: sym!("pw_thread_loop_new\0"),
            thread_loop_start: sym!("pw_thread_loop_start\0"),
            thread_loop_lock: sym!("pw_thread_loop_lock\0"),
            thread_loop_unlock: sym!("pw_thread_loop_unlock\0"),
            thread_loop_get_loop: sym!("pw_thread_loop_get_loop\0"),
            context_new: sym!("pw_context_new\0"),
            context_connect_fd: sym!("pw_context_connect_fd\0"),
            properties_new_string: sym!("pw_properties_new_string\0"),
            stream_new: sym!("pw_stream_new\0"),
            stream_add_listener: sym!("pw_stream_add_listener\0"),
            stream_connect: sym!("pw_stream_connect\0"),
            stream_dequeue_buffer: sym!("pw_stream_dequeue_buffer\0"),
            stream_queue_buffer: sym!("pw_stream_queue_buffer\0"),
            stream_destroy: sym!("pw_stream_destroy\0"),
            stream_update_params: sym!("pw_stream_update_params\0"),
        })
    }
}

/// The connection to the portal's PipeWire remote, shared by every stream.
pub struct PipeWire {
    lib: Lib,
    thread_loop: *mut c_void,
    core: *mut c_void,
}

// SAFETY: every use of `thread_loop` and `core` is under the thread loop's lock, as PipeWire requires.
#[allow(unsafe_code)]
unsafe impl Send for PipeWire {}
#[allow(unsafe_code)]
unsafe impl Sync for PipeWire {}

static PW: OnceLock<Option<PipeWire>> = OnceLock::new();

impl PipeWire {
    /// Connects once, on the remote the screen-cast portal opened for us.
    pub fn connect(fd: OwnedFd) -> Option<&'static PipeWire> {
        PW.get_or_init(|| Self::open(fd).inspect_err(|e| tracing::warn!(%e, "PipeWire unavailable")).ok()).as_ref()
    }

    pub fn get() -> Option<&'static PipeWire> {
        PW.get().and_then(Option::as_ref)
    }

    #[allow(unsafe_code)]
    fn open(fd: OwnedFd) -> Result<Self, String> {
        let lib = load().ok_or("libpipewire-0.3.so.0 not found")?;
        // SAFETY: PipeWire's documented start-up sequence; the remote fd is handed over (PipeWire closes it).
        unsafe {
            (lib.init)(std::ptr::null_mut(), std::ptr::null_mut());
            let thread_loop = (lib.thread_loop_new)(c"wxr-capture".as_ptr(), std::ptr::null());
            if thread_loop.is_null() {
                return Err("pw_thread_loop_new failed".into());
            }
            let context = (lib.context_new)((lib.thread_loop_get_loop)(thread_loop), std::ptr::null_mut(), 0);
            if context.is_null() {
                return Err("pw_context_new failed".into());
            }
            if (lib.thread_loop_start)(thread_loop) < 0 {
                return Err("pw_thread_loop_start failed".into());
            }
            (lib.thread_loop_lock)(thread_loop);
            let core = (lib.context_connect_fd)(context, fd.into_raw_fd(), std::ptr::null_mut(), 0);
            (lib.thread_loop_unlock)(thread_loop);
            if core.is_null() {
                return Err("pw_context_connect_fd failed".into());
            }
            Ok(Self { lib, thread_loop, core })
        }
    }
}

/// The part of a frame that is the picture (the compositor's crop), in the frame's pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Crop {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// A frame still in the GPU: one plane of a DMA-BUF.
#[derive(Debug)]
pub struct DmaFrame {
    pub fd: OwnedFd,
    pub offset: u32,
    pub stride: u32,
    pub modifier: u64,
    /// The whole buffer's size; `crop` is the picture inside it.
    pub w: u32,
    pub h: u32,
    pub crop: Crop,
}

#[derive(Debug, Default)]
pub enum Pixels {
    #[default]
    None,
    /// Tightly packed BGRA, already cropped.
    Bgra(Vec<u8>),
    Dma(DmaFrame),
}

/// The newest frame of a stream.
#[derive(Default)]
pub struct Latest {
    /// The picture's size (after the crop).
    pub w: u32,
    pub h: u32,
    pub pixels: Pixels,
    /// Counts frames, so a reader can tell a new one.
    pub seq: u64,
    format: u32,
    /// The negotiated buffer size.
    size: (u32, u32),
    /// Set when the stream settled on GPU buffers.
    modifier: Option<u64>,
}

struct Shared {
    lib: *const Lib,
    stream: *mut c_void,
    latest: Arc<Mutex<Latest>>,
    /// The DMA-BUF of the newest frame, kept from the compositor until the next one replaces it.
    held: Mutex<*mut PwBuffer>,
}

/// One PipeWire stream on a portal node; destroyed (under the loop lock) when dropped.
pub struct Stream {
    shared: Box<Shared>,
    _hook: Box<SpaHook>,
    pub latest: Arc<Mutex<Latest>>,
}

static EVENTS: StreamEvents = StreamEvents {
    version: PW_VERSION_STREAM_EVENTS,
    destroy: None,
    state_changed: None,
    control_info: None,
    io_changed: None,
    param_changed: Some(on_param_changed),
    add_buffer: None,
    remove_buffer: Some(on_remove_buffer),
    process: Some(on_process),
    drained: None,
    command: None,
    trigger_done: None,
};

/// DMA-BUF modifiers the GPU can import BGRA frames with; empty offers shared memory only.
static MODIFIERS: Mutex<Vec<u64>> = Mutex::new(Vec::new());

/// Sets the modifiers streams created from now on offer (the GPU's, from `wxr-gfx-vk`); empty turns DMA-BUF off.
pub fn set_dmabuf_modifiers(modifiers: Vec<u64>) {
    if let Ok(mut m) = MODIFIERS.lock() {
        *m = modifiers;
    }
}

fn offered_formats() -> Vec<Vec<u64>> {
    let modifiers = MODIFIERS.lock().map(|m| m.clone()).unwrap_or_default();
    let mut formats = Vec::new();
    if !modifiers.is_empty() {
        formats.push(dma_format(&modifiers));
    }
    formats.push(shm_format());
    formats
}

impl Stream {
    #[allow(unsafe_code)]
    pub fn new(pw: &'static PipeWire, node: u32) -> Result<Self, String> {
        let latest = Arc::new(Mutex::new(Latest::default()));
        let mut shared = Box::new(Shared {
            lib: &pw.lib,
            stream: std::ptr::null_mut(),
            latest: latest.clone(),
            held: Mutex::new(std::ptr::null_mut()),
        });
        let mut hook = Box::new(SpaHook { _fields: [std::ptr::null_mut(); 6] });
        let formats = offered_formats();
        // SAFETY: under the loop lock; `shared` and `hook` are boxed, so their addresses hold for the stream's life,
        // and the stream is destroyed (in Drop) before they are freed.
        unsafe {
            (pw.lib.thread_loop_lock)(pw.thread_loop);
            let props =
                (pw.lib.properties_new_string)(c"media.type=Video media.category=Capture media.role=Screen".as_ptr());
            let stream = (pw.lib.stream_new)(pw.core, c"WristXR".as_ptr(), props);
            if stream.is_null() {
                (pw.lib.thread_loop_unlock)(pw.thread_loop);
                return Err("pw_stream_new failed".into());
            }
            shared.stream = stream;
            (pw.lib.stream_add_listener)(stream, &mut *hook, &EVENTS, (&mut *shared as *mut Shared).cast());
            let mut params: Vec<*const u8> = formats.iter().map(|f| f.as_ptr().cast::<u8>()).collect();
            let r = (pw.lib.stream_connect)(
                stream,
                PW_DIRECTION_INPUT,
                node,
                PW_STREAM_FLAG_AUTOCONNECT | PW_STREAM_FLAG_MAP_BUFFERS,
                params.as_mut_ptr(),
                params.len() as u32,
            );
            (pw.lib.thread_loop_unlock)(pw.thread_loop);
            if r < 0 {
                drop(Stream { shared, _hook: hook, latest });
                return Err(format!("pw_stream_connect failed ({r})"));
            }
        }
        Ok(Self { shared, _hook: hook, latest })
    }

    /// The GPU could not import this stream's buffers: negotiate shared memory instead.
    #[allow(unsafe_code)]
    pub fn shared_memory_only(&self) {
        let Some(pw) = PipeWire::get() else { return };
        let shm = shm_format();
        let mut params = [shm.as_ptr().cast::<u8>()];
        // SAFETY: under the loop lock, on our own stream; the pod outlives the call (PipeWire copies params).
        unsafe {
            (pw.lib.thread_loop_lock)(pw.thread_loop);
            (pw.lib.stream_update_params)(self.shared.stream, params.as_mut_ptr(), 1);
            (pw.lib.thread_loop_unlock)(pw.thread_loop);
        }
    }
}

impl Drop for Stream {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        let Some(pw) = PipeWire::get() else { return };
        // SAFETY: destroying under the loop lock guarantees no callback is running or will run on `shared`; the
        // stream takes back the buffer we held.
        unsafe {
            (pw.lib.thread_loop_lock)(pw.thread_loop);
            (pw.lib.stream_destroy)(self.shared.stream);
            (pw.lib.thread_loop_unlock)(pw.thread_loop);
        }
    }
}

// --- Callbacks (on PipeWire's thread) ------------------------------------------------------------------------------

#[allow(unsafe_code)]
unsafe extern "C" fn on_param_changed(data: *mut c_void, id: u32, param: *const u8) {
    if id != SPA_PARAM_FORMAT || param.is_null() {
        return;
    }
    // SAFETY: `data` is our `Shared`; `param` is a valid SPA pod for the length its header gives.
    let (shared, pod) = unsafe {
        let size = u32::from_ne_bytes(*param.cast::<[u8; 4]>()) as usize;
        (&*data.cast::<Shared>(), std::slice::from_raw_parts(param, 8 + size))
    };
    let Some(f) = parse_format(pod) else { return };
    if let Ok(mut l) = shared.latest.lock() {
        l.format = f.format;
        l.size = (f.w, f.h);
        l.modifier = f.modifier;
    }
    // Ask for the compositor's crop and frame flags, and for buffers of the kind the format implies.
    let header = meta_param(SPA_META_HEADER, std::mem::size_of::<SpaMetaHeader>() as u32);
    let crop = meta_param(SPA_META_VIDEO_CROP, std::mem::size_of::<SpaRegion>() as u32);
    let types =
        if f.modifier.is_some() { 1 << SPA_DATA_DMA_BUF } else { (1 << SPA_DATA_MEM_PTR) | (1 << SPA_DATA_MEM_FD) };
    let buffers = buffers_param(types);
    let mut params = [header.as_ptr().cast::<u8>(), crop.as_ptr().cast::<u8>(), buffers.as_ptr().cast::<u8>()];
    // SAFETY: we are on the loop thread, inside the stream's own callback; PipeWire copies the pods.
    unsafe {
        ((*shared.lib).stream_update_params)(shared.stream, params.as_mut_ptr(), params.len() as u32);
    }
}

/// The buffer's meta of `kind`, when present and at least `size` bytes.
#[allow(unsafe_code)]
unsafe fn meta(buf: &SpaBuffer, kind: u32, size: usize) -> Option<*const c_void> {
    if buf.metas.is_null() {
        return None;
    }
    // SAFETY: `metas` points to `n_metas` entries, as the caller's buffer describes.
    let metas = unsafe { std::slice::from_raw_parts(buf.metas, buf.n_metas as usize) };
    metas.iter().find(|m| m.kind == kind && m.size as usize >= size && !m.data.is_null()).map(|m| m.data.cast_const())
}

/// The crop the compositor set, clamped to the frame; the whole frame when there is none.
fn crop_of(region: Option<&SpaRegion>, w: u32, h: u32) -> Crop {
    let whole = Crop { x: 0, y: 0, w, h };
    let Some(r) = region else { return whole };
    let (Ok(x), Ok(y)) = (u32::try_from(r.x), u32::try_from(r.y)) else { return whole };
    if r.w == 0 || r.h == 0 || x >= w || y >= h {
        return whole;
    }
    Crop { x, y, w: r.w.min(w - x), h: r.h.min(h - y) }
}

#[allow(unsafe_code)]
/// A buffer leaves the stream (the format or the buffers were renegotiated): if it is the one held for the GPU, it
/// is no longer ours to give back.
unsafe extern "C" fn on_remove_buffer(data: *mut c_void, buffer: *mut c_void) {
    // SAFETY: `data` is our `Shared`, as for every callback of this stream.
    let shared = unsafe { &*data.cast::<Shared>() };
    let mut held = shared.held.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if held.cast::<c_void>() == buffer {
        *held = std::ptr::null_mut();
    }
}

unsafe extern "C" fn on_process(data: *mut c_void) {
    // SAFETY: `data` is our `Shared`; buffers come from this stream and go back to it; the mapped data covers the
    // chunk PipeWire describes; metas are read within the sizes PipeWire gives.
    unsafe {
        let shared = &*data.cast::<Shared>();
        let lib = &*shared.lib;
        // Only the newest buffer matters: return older ones at once.
        let mut newest: *mut PwBuffer = std::ptr::null_mut();
        loop {
            let b = (lib.stream_dequeue_buffer)(shared.stream);
            if b.is_null() {
                break;
            }
            if !newest.is_null() {
                (lib.stream_queue_buffer)(shared.stream, newest);
            }
            newest = b;
        }
        if newest.is_null() {
            return;
        }
        let buf = &*(*newest).buffer;
        let corrupted = meta(buf, SPA_META_HEADER, std::mem::size_of::<SpaMetaHeader>())
            .is_some_and(|h| (*h.cast::<SpaMetaHeader>()).flags & SPA_META_HEADER_FLAG_CORRUPTED != 0);
        let region = meta(buf, SPA_META_VIDEO_CROP, std::mem::size_of::<SpaRegion>()).map(|r| &*r.cast::<SpaRegion>());
        let mut keep = false;
        if !corrupted && buf.n_datas > 0 && !buf.datas.is_null() && !(*buf.datas).chunk.is_null() {
            let d = &*buf.datas;
            let chunk = &*d.chunk;
            if let Ok(mut l) = shared.latest.lock()
                && chunk.flags & SPA_CHUNK_FLAG_CORRUPTED == 0
            {
                let (w, h) = l.size;
                let crop = crop_of(region, w, h);
                if d.kind == SPA_DATA_DMA_BUF {
                    // One plane only (BGRA has one; compressed layouts with more are not offered).
                    if let (1, Some(modifier), Ok(fd)) =
                        (buf.n_datas, l.modifier, BorrowedFd::borrow_raw(d.fd as i32).try_clone_to_owned())
                    {
                        let stride = if chunk.stride > 0 { chunk.stride as u32 } else { w * 4 };
                        l.pixels = Pixels::Dma(DmaFrame { fd, offset: chunk.offset, stride, modifier, w, h, crop });
                        (l.w, l.h) = (crop.w, crop.h);
                        l.seq += 1;
                        keep = true;
                    }
                } else if !d.data.is_null() && chunk.size > 0 {
                    let stride = if chunk.stride > 0 { chunk.stride as usize } else { w as usize * 4 };
                    let end = chunk.offset as usize + stride * h as usize;
                    if w > 0 && h > 0 && stride >= w as usize * 4 && end <= d.maxsize as usize {
                        let base = d.data.cast::<u8>().add(chunk.offset as usize);
                        let swap = matches!(l.format, VIDEO_RGBX | VIDEO_RGBA);
                        let opaque = matches!(l.format, VIDEO_RGBX | VIDEO_BGRX);
                        let (cw, ch) = (crop.w as usize, crop.h as usize);
                        let mut px = match std::mem::take(&mut l.pixels) {
                            Pixels::Bgra(v) => v,
                            _ => Vec::new(),
                        };
                        px.resize(cw * ch * 4, 0);
                        for y in 0..ch {
                            let src = std::slice::from_raw_parts(
                                base.add((crop.y as usize + y) * stride + crop.x as usize * 4),
                                cw * 4,
                            );
                            let dst = &mut px[y * cw * 4..(y + 1) * cw * 4];
                            dst.copy_from_slice(src);
                            if swap || opaque {
                                for p in dst.as_chunks_mut::<4>().0 {
                                    if swap {
                                        p.swap(0, 2);
                                    }
                                    if opaque {
                                        p[3] = 0xFF;
                                    }
                                }
                            }
                        }
                        l.pixels = Pixels::Bgra(px);
                        (l.w, l.h) = (crop.w, crop.h);
                        l.seq += 1;
                    }
                }
            }
        }
        // A DMA-BUF frame is read later by the GPU: hold its buffer until the next frame, return the one held before.
        let mut held = shared.held.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if !held.is_null() {
            (lib.stream_queue_buffer)(shared.stream, *held);
            *held = std::ptr::null_mut();
        }
        if keep {
            *held = newest;
        } else {
            (lib.stream_queue_buffer)(shared.stream, newest);
        }
    }
}

// --- SPA pods --------------------------------------------------------------------------------------------------------

/// Words of a pod under construction; pods are 8-byte aligned, so they are built in u64s.
struct Pod(Vec<u32>);

impl Pod {
    /// An object pod of `object_type` for param `id`; `finish` fills in its size.
    fn object(object_type: u32, id: u32) -> Self {
        Self(vec![0, SPA_TYPE_OBJECT, object_type, id])
    }
    fn push(&mut self, words: &[u32]) {
        self.0.extend_from_slice(words);
    }
    fn pad(&mut self) {
        if self.0.len() % 2 == 1 {
            self.0.push(0);
        }
    }
    /// `key: value` where value is a pod of `kind` with body `body` (32-bit words).
    fn prop(&mut self, key: u32, kind: u32, body: &[u32]) {
        self.push(&[key, 0, (body.len() * 4) as u32, kind]);
        self.push(body);
        self.pad();
    }
    /// `key: choice` of `values` (each `child_words` long) of `child_type`, with property `flags`.
    fn choice_flags(&mut self, key: u32, flags: u32, choice: u32, child_type: u32, child_words: usize, values: &[u32]) {
        let body = 16 + values.len() * 4;
        self.push(&[key, flags, body as u32, SPA_TYPE_CHOICE, choice, 0, (child_words * 4) as u32, child_type]);
        self.push(values);
        self.pad();
    }
    fn choice(&mut self, key: u32, choice: u32, child_type: u32, child_words: usize, values: &[u32]) {
        self.choice_flags(key, 0, choice, child_type, child_words, values);
    }
    fn finish(self) -> Vec<u64> {
        let mut words = self.0;
        words[0] = ((words.len() - 2) * 4) as u32;
        words.chunks(2).map(|c| u64::from(c[0]) | (u64::from(c.get(1).copied().unwrap_or(0)) << 32)).collect()
    }
}

fn video_basics(p: &mut Pod) {
    p.prop(SPA_FORMAT_MEDIA_TYPE, SPA_TYPE_ID, &[SPA_MEDIA_TYPE_VIDEO]);
    p.prop(SPA_FORMAT_MEDIA_SUBTYPE, SPA_TYPE_ID, &[SPA_MEDIA_SUBTYPE_RAW]);
}

fn size_and_rate(p: &mut Pod) {
    p.choice(SPA_FORMAT_VIDEO_SIZE, SPA_CHOICE_RANGE, SPA_TYPE_RECTANGLE, 2, &[1920, 1080, 1, 1, 16384, 16384]);
    p.choice(SPA_FORMAT_VIDEO_FRAMERATE, SPA_CHOICE_RANGE, SPA_TYPE_FRACTION, 2, &[60, 1, 0, 1, 360, 1]);
}

/// What we take in shared memory: 32-bit RGB in any order, any size, up to 360 Hz (no modifier, so the compositor
/// sends plain memory).
fn shm_format() -> Vec<u64> {
    let mut p = Pod::object(SPA_TYPE_OBJECT_FORMAT, SPA_PARAM_ENUM_FORMAT);
    video_basics(&mut p);
    p.choice(
        SPA_FORMAT_VIDEO_FORMAT,
        SPA_CHOICE_ENUM,
        SPA_TYPE_ID,
        1,
        &[VIDEO_BGRX, VIDEO_BGRX, VIDEO_BGRA, VIDEO_RGBX, VIDEO_RGBA],
    );
    size_and_rate(&mut p);
    p.finish()
}

/// What we take as DMA-BUF: BGRA only, so the GPU copies it byte for byte into a BGRA texture (alpha included),
/// with one of the GPU's modifiers.
fn dma_format(modifiers: &[u64]) -> Vec<u64> {
    let mut p = Pod::object(SPA_TYPE_OBJECT_FORMAT, SPA_PARAM_ENUM_FORMAT);
    video_basics(&mut p);
    p.prop(SPA_FORMAT_VIDEO_FORMAT, SPA_TYPE_ID, &[VIDEO_BGRA]);
    // An enum choice: the default first, then every option.
    let longs: Vec<u32> = std::iter::once(modifiers[0])
        .chain(modifiers.iter().copied())
        .flat_map(|m| [m as u32, (m >> 32) as u32])
        .collect();
    p.choice_flags(
        SPA_FORMAT_VIDEO_MODIFIER,
        SPA_POD_PROP_FLAG_MANDATORY | SPA_POD_PROP_FLAG_DONT_FIXATE,
        SPA_CHOICE_ENUM,
        SPA_TYPE_LONG,
        2,
        &longs,
    );
    size_and_rate(&mut p);
    p.finish()
}

fn meta_param(kind: u32, size: u32) -> Vec<u64> {
    let mut p = Pod::object(SPA_TYPE_OBJECT_PARAM_META, SPA_PARAM_META);
    p.prop(SPA_PARAM_META_TYPE, SPA_TYPE_ID, &[kind]);
    p.prop(SPA_PARAM_META_SIZE, SPA_TYPE_INT, &[size]);
    p.finish()
}

/// Buffers whose memory is of the `types` mask (bits of `SPA_DATA_*`).
fn buffers_param(types: u32) -> Vec<u64> {
    let mut p = Pod::object(SPA_TYPE_OBJECT_PARAM_BUFFERS, SPA_PARAM_BUFFERS);
    p.choice(SPA_PARAM_BUFFERS_DATA_TYPE, SPA_CHOICE_FLAGS, SPA_TYPE_INT, 1, &[types]);
    p.finish()
}

fn word(b: &[u8], i: usize) -> Option<u32> {
    b.get(i * 4..i * 4 + 4).map(|w| u32::from_ne_bytes([w[0], w[1], w[2], w[3]]))
}

/// The fixed format the stream settled on.
#[derive(Debug, PartialEq, Eq)]
struct Format {
    format: u32,
    w: u32,
    h: u32,
    /// Present when the buffers are DMA-BUFs.
    modifier: Option<u64>,
}

fn parse_format(pod: &[u8]) -> Option<Format> {
    if word(pod, 1)? != SPA_TYPE_OBJECT {
        return None;
    }
    let (mut format, mut size, mut modifier) = (None, None, None);
    let mut at = 16; // pod header + object body header
    while at + 16 <= pod.len() {
        let key = word(pod, at / 4)?;
        let vsize = word(pod, at / 4 + 2)? as usize;
        let vtype = word(pod, at / 4 + 3)?;
        let body = at + 16;
        // A choice wraps its value; its first child is the chosen (default) one.
        let (vtype, first) = if vtype == SPA_TYPE_CHOICE {
            let kind = word(pod, body / 4)?;
            if kind != SPA_CHOICE_NONE && kind != SPA_CHOICE_ENUM && kind != SPA_CHOICE_RANGE {
                (0, body)
            } else {
                (word(pod, body / 4 + 3)?, body + 16)
            }
        } else {
            (vtype, body)
        };
        match (key, vtype) {
            (SPA_FORMAT_VIDEO_FORMAT, SPA_TYPE_ID | SPA_TYPE_INT) => format = word(pod, first / 4),
            (SPA_FORMAT_VIDEO_SIZE, SPA_TYPE_RECTANGLE) => {
                size = Some((word(pod, first / 4)?, word(pod, first / 4 + 1)?))
            }
            (SPA_FORMAT_VIDEO_MODIFIER, SPA_TYPE_LONG) => {
                modifier = Some(u64::from(word(pod, first / 4)?) | (u64::from(word(pod, first / 4 + 1)?) << 32))
            }
            _ => {}
        }
        at = body + vsize.div_ceil(8) * 8;
    }
    let (w, h) = size?;
    Some(Format { format: format?, w, h, modifier })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(words: &[u64]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_ne_bytes()).collect()
    }

    fn length_matches(pod: &[u8]) {
        let size = u32::from_ne_bytes([pod[0], pod[1], pod[2], pod[3]]) as usize;
        assert_eq!(size + 8, pod.len(), "pod length matches its header");
    }

    #[test]
    fn our_shared_memory_format_reads_back() {
        let pod = bytes(&shm_format());
        length_matches(&pod);
        assert_eq!(parse_format(&pod), Some(Format { format: VIDEO_BGRX, w: 1920, h: 1080, modifier: None }));
    }

    #[test]
    fn our_dma_buf_format_reads_back_with_its_first_modifier() {
        let m = 0x0100_0000_0000_0001_u64;
        let pod = bytes(&dma_format(&[m, 0]));
        length_matches(&pod);
        assert_eq!(parse_format(&pod), Some(Format { format: VIDEO_BGRA, w: 1920, h: 1080, modifier: Some(m) }));
    }

    #[test]
    fn params_are_whole_pods() {
        for pod in [meta_param(SPA_META_HEADER, 32), meta_param(SPA_META_VIDEO_CROP, 16), buffers_param(6)] {
            let pod = bytes(&pod);
            length_matches(&pod);
            assert_eq!(word(&pod, 1), Some(SPA_TYPE_OBJECT));
        }
    }

    #[test]
    fn meta_layouts_match_spa() {
        assert_eq!(std::mem::size_of::<SpaMetaHeader>(), 32);
        assert_eq!(std::mem::size_of::<SpaRegion>(), 16);
        assert_eq!(std::mem::size_of::<SpaMeta>(), 16);
    }

    #[test]
    fn crops_stay_inside_the_frame() {
        let r = |x, y, w, h| SpaRegion { x, y, w, h };
        assert_eq!(crop_of(None, 800, 600), Crop { x: 0, y: 0, w: 800, h: 600 });
        assert_eq!(crop_of(Some(&r(10, 20, 300, 200)), 800, 600), Crop { x: 10, y: 20, w: 300, h: 200 });
        assert_eq!(crop_of(Some(&r(700, 500, 300, 200)), 800, 600), Crop { x: 700, y: 500, w: 100, h: 100 });
        assert_eq!(crop_of(Some(&r(-5, 0, 10, 10)), 800, 600), Crop { x: 0, y: 0, w: 800, h: 600 });
        assert_eq!(crop_of(Some(&r(0, 0, 0, 0)), 800, 600), Crop { x: 0, y: 0, w: 800, h: 600 });
    }
}
