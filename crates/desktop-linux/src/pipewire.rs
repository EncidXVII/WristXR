//! PipeWire, loaded at run time (`libpipewire-0.3.so.0`), so building needs no PipeWire headers and the app still
//! runs on X11 machines without it. Only what screen capture needs: one connection on the portal's remote, and one
//! stream per shared monitor or window that keeps the newest frame.
//!
//! The layouts and numbers below are PipeWire's and SPA's stable C ABI (pipewire/stream.h, spa/buffer/buffer.h,
//! spa/pod/pod.h, spa/param/format.h, spa/param/video/raw.h).
use std::ffi::{CStr, c_char, c_int, c_void};
use std::os::fd::{IntoRawFd, OwnedFd};
use std::sync::{Arc, Mutex, OnceLock};

// --- SPA / PipeWire numbers ----------------------------------------------------------------------------------------
const SPA_TYPE_ID: u32 = 3;
const SPA_TYPE_INT: u32 = 4;
const SPA_TYPE_RECTANGLE: u32 = 10;
const SPA_TYPE_FRACTION: u32 = 11;
const SPA_TYPE_OBJECT: u32 = 15;
const SPA_TYPE_CHOICE: u32 = 19;
const SPA_TYPE_OBJECT_FORMAT: u32 = 0x40003;
const SPA_PARAM_ENUM_FORMAT: u32 = 3;
const SPA_PARAM_FORMAT: u32 = 4;
const SPA_CHOICE_NONE: u32 = 0;
const SPA_CHOICE_RANGE: u32 = 1;
const SPA_CHOICE_ENUM: u32 = 3;
const SPA_FORMAT_MEDIA_TYPE: u32 = 1;
const SPA_FORMAT_MEDIA_SUBTYPE: u32 = 2;
const SPA_FORMAT_VIDEO_FORMAT: u32 = 0x20001;
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
struct SpaBuffer {
    n_metas: u32,
    n_datas: u32,
    metas: *mut c_void,
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
            let thread_loop = (lib.thread_loop_new)(c"dvr-capture".as_ptr(), std::ptr::null());
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

/// The newest frame of a stream, tightly packed BGRA.
#[derive(Default)]
pub struct Latest {
    pub w: u32,
    pub h: u32,
    pub bgra: Vec<u8>,
    /// Counts frames, so a reader can tell a new one.
    pub seq: u64,
    format: u32,
}

struct Shared {
    lib: *const Lib,
    stream: *mut c_void,
    latest: Arc<Mutex<Latest>>,
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
    remove_buffer: None,
    process: Some(on_process),
    drained: None,
    command: None,
    trigger_done: None,
};

impl Stream {
    #[allow(unsafe_code)]
    pub fn new(pw: &'static PipeWire, node: u32) -> Result<Self, String> {
        let latest = Arc::new(Mutex::new(Latest::default()));
        let mut shared = Box::new(Shared { lib: &pw.lib, stream: std::ptr::null_mut(), latest: latest.clone() });
        let mut hook = Box::new(SpaHook { _fields: [std::ptr::null_mut(); 6] });
        let pod = enum_format();
        // SAFETY: under the loop lock; `shared` and `hook` are boxed, so their addresses hold for the stream's life,
        // and the stream is destroyed (in Drop) before they are freed.
        unsafe {
            (pw.lib.thread_loop_lock)(pw.thread_loop);
            let props =
                (pw.lib.properties_new_string)(c"media.type=Video media.category=Capture media.role=Screen".as_ptr());
            let stream = (pw.lib.stream_new)(pw.core, c"DesktopVR".as_ptr(), props);
            if stream.is_null() {
                (pw.lib.thread_loop_unlock)(pw.thread_loop);
                return Err("pw_stream_new failed".into());
            }
            shared.stream = stream;
            (pw.lib.stream_add_listener)(stream, &mut *hook, &EVENTS, (&mut *shared as *mut Shared).cast());
            let mut params = [pod.as_ptr().cast::<u8>()];
            let r = (pw.lib.stream_connect)(
                stream,
                PW_DIRECTION_INPUT,
                node,
                PW_STREAM_FLAG_AUTOCONNECT | PW_STREAM_FLAG_MAP_BUFFERS,
                params.as_mut_ptr(),
                1,
            );
            (pw.lib.thread_loop_unlock)(pw.thread_loop);
            if r < 0 {
                drop(Stream { shared, _hook: hook, latest });
                return Err(format!("pw_stream_connect failed ({r})"));
            }
        }
        Ok(Self { shared, _hook: hook, latest })
    }
}

impl Drop for Stream {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        let Some(pw) = PipeWire::get() else { return };
        // SAFETY: destroying under the loop lock guarantees no callback is running or will run on `shared`.
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
    if let Some((format, w, h)) = parse_format(pod)
        && let Ok(mut l) = shared.latest.lock()
    {
        l.format = format;
        l.w = w;
        l.h = h;
    }
}

#[allow(unsafe_code)]
unsafe extern "C" fn on_process(data: *mut c_void) {
    // SAFETY: `data` is our `Shared`; buffers come from this stream and go back to it; the mapped data covers the
    // chunk PipeWire describes.
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
        if buf.n_datas > 0 && !buf.datas.is_null() {
            let d = &*buf.datas;
            if !d.data.is_null() && !d.chunk.is_null() && (*d.chunk).size > 0 {
                let chunk = &*d.chunk;
                if let Ok(mut l) = shared.latest.lock() {
                    let (w, h) = (l.w as usize, l.h as usize);
                    let stride = if chunk.stride > 0 { chunk.stride as usize } else { w * 4 };
                    let base = d.data.cast::<u8>().add(chunk.offset as usize);
                    if w > 0 && h > 0 && stride >= w * 4 && (chunk.offset as usize + stride * h) <= d.maxsize as usize {
                        let swap = matches!(l.format, VIDEO_RGBX | VIDEO_RGBA);
                        let opaque = matches!(l.format, VIDEO_RGBX | VIDEO_BGRX);
                        l.bgra.resize(w * h * 4, 0);
                        for y in 0..h {
                            let src = std::slice::from_raw_parts(base.add(y * stride), w * 4);
                            let dst = &mut l.bgra[y * w * 4..(y + 1) * w * 4];
                            dst.copy_from_slice(src);
                            if swap || opaque {
                                for px in dst.as_chunks_mut::<4>().0 {
                                    if swap {
                                        px.swap(0, 2);
                                    }
                                    if opaque {
                                        px[3] = 0xFF;
                                    }
                                }
                            }
                        }
                        l.seq += 1;
                    }
                }
            }
        }
        (lib.stream_queue_buffer)(shared.stream, newest);
    }
}

// --- SPA pods --------------------------------------------------------------------------------------------------------

/// Words of a pod under construction; pods are 8-byte aligned, so they are built in u64s.
struct Pod(Vec<u32>);

impl Pod {
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
    /// `key: choice` of `values` (each `child_words` long) of `child_type`.
    fn choice(&mut self, key: u32, choice: u32, child_type: u32, child_words: usize, values: &[u32]) {
        let body = 16 + values.len() * 4;
        self.push(&[key, 0, body as u32, SPA_TYPE_CHOICE, choice, 0, (child_words * 4) as u32, child_type]);
        self.push(values);
        self.pad();
    }
}

/// What we can take: 32-bit RGB in any order, any size, up to 360 Hz, in shared memory (no modifiers offered, so
/// compositors send plain memory rather than GPU buffers).
fn enum_format() -> Vec<u64> {
    let mut p = Pod(vec![0, SPA_TYPE_OBJECT, SPA_TYPE_OBJECT_FORMAT, SPA_PARAM_ENUM_FORMAT]);
    p.prop(SPA_FORMAT_MEDIA_TYPE, SPA_TYPE_ID, &[SPA_MEDIA_TYPE_VIDEO]);
    p.prop(SPA_FORMAT_MEDIA_SUBTYPE, SPA_TYPE_ID, &[SPA_MEDIA_SUBTYPE_RAW]);
    p.choice(
        SPA_FORMAT_VIDEO_FORMAT,
        SPA_CHOICE_ENUM,
        SPA_TYPE_ID,
        1,
        &[VIDEO_BGRX, VIDEO_BGRX, VIDEO_BGRA, VIDEO_RGBX, VIDEO_RGBA],
    );
    p.choice(SPA_FORMAT_VIDEO_SIZE, SPA_CHOICE_RANGE, SPA_TYPE_RECTANGLE, 2, &[1920, 1080, 1, 1, 16384, 16384]);
    p.choice(SPA_FORMAT_VIDEO_FRAMERATE, SPA_CHOICE_RANGE, SPA_TYPE_FRACTION, 2, &[60, 1, 0, 1, 360, 1]);
    let mut words = p.0;
    words[0] = ((words.len() - 2) * 4) as u32;
    words.chunks(2).map(|c| u64::from(c[0]) | (u64::from(c.get(1).copied().unwrap_or(0)) << 32)).collect()
}

fn word(b: &[u8], i: usize) -> Option<u32> {
    b.get(i * 4..i * 4 + 4).map(|w| u32::from_ne_bytes([w[0], w[1], w[2], w[3]]))
}

/// The fixed format the stream settled on: (video format, width, height).
fn parse_format(pod: &[u8]) -> Option<(u32, u32, u32)> {
    if word(pod, 1)? != SPA_TYPE_OBJECT {
        return None;
    }
    let (mut format, mut size) = (None, None);
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
            _ => {}
        }
        at = body + vsize.div_ceil(8) * 8;
    }
    let (w, h) = size?;
    Some((format?, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(words: &[u64]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_ne_bytes()).collect()
    }

    #[test]
    fn our_enum_format_reads_back() {
        let pod = bytes(&enum_format());
        let size = u32::from_ne_bytes([pod[0], pod[1], pod[2], pod[3]]) as usize;
        assert_eq!(size + 8, pod.len(), "pod length matches its header");
        assert_eq!(parse_format(&pod), Some((VIDEO_BGRX, 1920, 1080)));
    }
}
