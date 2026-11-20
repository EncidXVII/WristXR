//! OpenVR overlay backend. Talks to the runtime's `vrclient` directly through the C function
//! tables, so no `openvr_api` library has to ship with the app.
#![allow(unsafe_code)]

mod ffi;

pub use ffi::VulkanTexture;

use dvr_core::backend::VrBackend;
use dvr_core::engine::{Claims, FrameInput};
use dvr_core::math::Pose;
use dvr_core::panel::{Anchor, Hand, Panel, Source};
use ffi::{
    ActiveActionSet, AnalogData, ClientCore, DigitalData, FrameTiming, Mat34, PoseData, Table, TrackedPose, VrEvent,
    applications, compositor, input, overlay, system,
};
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::mem::size_of;
use std::path::{Path, PathBuf};

const CURSOR_WIDTH_M: f32 = 0.015;
/// `k_nActionSetOverlayGlobalPriorityMin`: input bound at this priority is not delivered to the game.
const OVERLAY_PRIORITY: i32 = 0x0100_0000;
const CURSOR_SORT: u32 = 10;
const KEYBOARD_SORT: u32 = 5;

/// Calls a function-table entry with an explicit signature.
macro_rules! call {
    ($table:expr, $idx:expr, fn($($t:ty),*) -> $r:ty $(, $arg:expr)*) => {{
        // SAFETY: the signature is copied from openvr_capi.h for this index (checked by ffi tests),
        // and the table comes from a live, initialized runtime.
        let f: unsafe extern "system" fn($($t),*) -> $r = unsafe { $table.get($idx) };
        unsafe { f($($arg),*) }
    }};
}

/// Overlay identity: panels use their id, cursors use the top two values.
pub fn cursor_key(hand: Hand) -> u64 {
    u64::MAX - 1 + hand.index() as u64
}

/// Numbers for the wrist: what the game and the user's devices are doing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VrStats {
    /// Frame rate of the scene app (game); `None` when no game is presenting.
    pub game_fps: Option<f32>,
    /// Each hand's controller (`Hand::index()`).
    pub controllers: [Battery; 2],
    /// Lowest battery among trackers.
    pub lowest_tracker: Option<f32>,
    /// Headset refresh rate (Hz), when SteamVR reports it.
    pub display_hz: Option<f32>,
}

/// A controller's battery as SteamVR reports it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Battery {
    /// The controller is connected.
    pub on: bool,
    /// 0..1; `None` when the controller does not report it.
    pub level: Option<f32>,
    pub charging: bool,
}

struct Actions {
    /// Always shared with the game: the pointer pose, haptics and the menu button.
    set: u64,
    /// Taken from the game per hand, each on its own (see `Claims`): trigger and grip, the stick, A/X.
    point: u64,
    scroll: u64,
    right: u64,
    pointer: u64,
    click: u64,
    grab: u64,
    stick: u64,
    menu: u64,
    right_click: u64,
    haptic: u64,
    hands: [u64; 2],
}

struct Placed {
    handle: u64,
    last: Option<(Panel, Pose)>,
    shown: bool,
}

pub struct OpenVr {
    core: *mut ClientCore,
    system: Table,
    overlay: Table,
    input: Table,
    compositor: Table,
    applications: Table,
    actions: Actions,
    claims: [Claims; 2],
    /// The action manifest last loaded, and since when a tracked hand's buttons have not reached us (unbound),
    /// for one reload.
    manifest: Option<PathBuf>,
    unbound_since: Option<u64>,
    reloaded: bool,
    input_error: i32,
    overlays: HashMap<u64, Placed>,
}

fn vrpath_file() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(unix)]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    base.map(|b| b.join("openvr").join("openvrpaths.vrpath"))
}

/// SteamVR install directory as registered in `openvrpaths.vrpath`.
pub fn runtime_dir() -> Result<PathBuf, String> {
    let file = vrpath_file().ok_or("no user config directory")?;
    let text = std::fs::read_to_string(&file).map_err(|_| "SteamVR is not installed (no openvrpaths.vrpath)")?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
    json["runtime"][0].as_str().map(PathBuf::from).ok_or_else(|| "no SteamVR runtime registered".into())
}

/// SteamVR's config folder (`appconfig.json`, `vrappconfig`), as registered in `openvrpaths.vrpath`.
fn config_dir() -> Result<PathBuf, String> {
    let file = vrpath_file().ok_or("no user config directory")?;
    let text = std::fs::read_to_string(&file).map_err(|_| "SteamVR is not installed (no openvrpaths.vrpath)")?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
    json["config"][0].as_str().map(PathBuf::from).ok_or_else(|| "no SteamVR config folder registered".into())
}

/// Takes the app off SteamVR's list: its manifest (so no more start with SteamVR) and its per-app settings.
/// Through SteamVR when it is running (it keeps its list in memory and would write it back); otherwise straight in
/// SteamVR's files. Never starts SteamVR.
pub fn remove_app(manifest: &Path, app_key: &str) -> Result<(), String> {
    // SteamVR gone (uninstalled first), or never there: no list to be on.
    let Ok(config) = config_dir() else { return Ok(()) };
    if remove_app_live(manifest, app_key).is_err() {
        // With SteamVR off: out of `appconfig.json`'s list.
        drop_manifest(&config.join("appconfig.json"), manifest)?;
    }
    let settings = config.join("vrappconfig").join(format!("{app_key}.vrappconfig"));
    match std::fs::remove_file(&settings) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("{}: {e}", settings.display())),
        _ => Ok(()),
    }
}

fn remove_app_live(manifest: &Path, app_key: &str) -> Result<(), String> {
    let factory = ffi::load_factory(&client_library(&runtime_dir()?))?;
    let mut err = 0;
    // SAFETY: factory from the runtime library; NUL-terminated version string.
    let core = unsafe { factory(ffi::CLIENT_CORE_VERSION.as_ptr(), &mut err) }.cast::<ClientCore>();
    if core.is_null() {
        return Err(format!("SteamVR client core unavailable (error {err})"));
    }
    // SAFETY: a non-null core object always carries its vtable.
    let vtbl = unsafe { &*(*core).vtbl };
    // SAFETY: valid core pointer; null startup info is allowed. A background app fails here when SteamVR is off.
    if unsafe { (vtbl.init)(core, ffi::APP_BACKGROUND, std::ptr::null()) } != 0 {
        return Err("SteamVR is not running".into());
    }
    let mut e = 0;
    // SAFETY: initialized core; NUL-terminated interface name.
    let p = unsafe { (vtbl.get_generic_interface)(core, ffi::APPLICATIONS_VERSION.as_ptr(), &mut e) };
    let result = if p.is_null() || e != 0 {
        Err(format!("IVRApplications unavailable (VRInitError {e})"))
    } else {
        let apps = Table(p.cast());
        let (path, key) = (cstring(&manifest.to_string_lossy())?, cstring(app_key)?);
        call!(apps, applications::SET_APPLICATION_AUTO_LAUNCH, fn(*const c_char, u8) -> i32, key.as_ptr(), 0);
        match call!(apps, applications::REMOVE_APPLICATION_MANIFEST, fn(*const c_char) -> i32, path.as_ptr()) {
            0 => Ok(()),
            e => Err(format!("RemoveApplicationManifest: EVRApplicationError {e}")),
        }
    };
    // SAFETY: balances the successful init above.
    unsafe { (vtbl.cleanup)(core) };
    result
}

fn drop_manifest(file: &Path, manifest: &Path) -> Result<(), String> {
    let Ok(text) = std::fs::read_to_string(file) else { return Ok(()) };
    let mut json: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
    let same = |p: &str| same_path(Path::new(p), manifest);
    if let Some(list) = json["manifest_paths"].as_array_mut() {
        let before = list.len();
        list.retain(|p| !p.as_str().is_some_and(same));
        if list.len() != before {
            let out = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
            std::fs::write(file, out).map_err(|e| format!("{}: {e}", file.display()))?;
        }
    }
    Ok(())
}

/// Windows paths compare without case and either slash.
fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().replace('/', "\\").to_lowercase();
    norm(a) == norm(b)
}

fn client_library(runtime: &Path) -> PathBuf {
    if cfg!(windows) {
        runtime.join("bin").join("vrclient_x64.dll")
    } else if cfg!(target_arch = "aarch64") {
        runtime.join("bin").join("linuxarm64").join("vrclient.so")
    } else {
        runtime.join("bin").join("linux64").join("vrclient.so")
    }
}

fn cstring(s: &str) -> Result<CString, String> {
    CString::new(s).map_err(|e| e.to_string())
}

impl OpenVr {
    /// Connects as an overlay app (starts SteamVR if needed) and loads the action manifest.
    pub fn init(manifest: &Path) -> Result<Self, String> {
        let mut vr = Self::connect()?;
        vr.load_actions(manifest)?;
        Ok(vr)
    }

    /// Connects as an overlay app (starts SteamVR if needed), without input yet: an app that registers itself
    /// (`register_app`) does that first, then `load_actions`, so SteamVR files the bindings under the app's own key.
    /// The other way round, a first launch gets bindings filed under a stand-in key and no buttons until a restart.
    pub fn connect() -> Result<Self, String> {
        let factory = ffi::load_factory(&client_library(&runtime_dir()?))?;
        let mut err = 0;
        // SAFETY: factory from the runtime library; NUL-terminated version string.
        let core = unsafe { factory(ffi::CLIENT_CORE_VERSION.as_ptr(), &mut err) }.cast::<ClientCore>();
        if core.is_null() {
            return Err(format!("SteamVR client core unavailable (error {err})"));
        }
        // SAFETY: a non-null core object always carries its vtable.
        let vtbl = unsafe { &*(*core).vtbl };
        // SAFETY: valid core pointer; null startup info is allowed.
        let e = unsafe { (vtbl.init)(core, ffi::APP_OVERLAY, std::ptr::null()) };
        if e != 0 {
            return Err(format!("SteamVR init failed (VRInitError {e})"));
        }
        let interface = |name: &CStr| -> Result<Table, String> {
            let mut e = 0;
            // SAFETY: initialized core; NUL-terminated interface name.
            let p = unsafe { (vtbl.get_generic_interface)(core, name.as_ptr(), &mut e) };
            if p.is_null() || e != 0 {
                Err(format!("{} unavailable (VRInitError {e})", name.to_string_lossy()))
            } else {
                Ok(Table(p.cast()))
            }
        };
        let tables = (|| {
            Ok::<_, String>((
                interface(ffi::SYSTEM_VERSION)?,
                interface(ffi::OVERLAY_VERSION)?,
                interface(ffi::INPUT_VERSION)?,
                interface(ffi::COMPOSITOR_VERSION)?,
                interface(ffi::APPLICATIONS_VERSION)?,
            ))
        })();
        let (system, overlay, input, compositor, applications) = match tables {
            Ok(t) => t,
            Err(e) => {
                // SAFETY: balances the successful init above.
                unsafe { (vtbl.cleanup)(core) };
                return Err(e);
            }
        };
        let mut vr = Self {
            core,
            system,
            overlay,
            input,
            compositor,
            applications,
            claims: [Claims::default(); 2],
            manifest: None,
            unbound_since: None,
            reloaded: false,
            input_error: 0,
            actions: Actions {
                set: 0,
                point: 0,
                scroll: 0,
                right: 0,
                pointer: 0,
                click: 0,
                grab: 0,
                stick: 0,
                menu: 0,
                right_click: 0,
                haptic: 0,
                hands: [0; 2],
            },
            overlays: HashMap::new(),
        };
        for hand in Hand::BOTH {
            let key = cursor_key(hand);
            let name = format!("dvr.cursor.{}", hand.index());
            let h = vr.overlay_handle(key, &name)?;
            call!(vr.overlay, overlay::SET_OVERLAY_WIDTH_IN_METERS, fn(u64, f32) -> i32, h, CURSOR_WIDTH_M);
            call!(vr.overlay, overlay::SET_OVERLAY_SORT_ORDER, fn(u64, u32) -> i32, h, CURSOR_SORT);
        }
        Ok(vr)
    }

    /// Loads the action manifest (see `connect`).
    pub fn load_actions(&mut self, manifest: &Path) -> Result<(), String> {
        self.manifest = Some(manifest.to_path_buf());
        let path = cstring(&manifest.to_string_lossy())?;
        let e = call!(self.input, input::SET_ACTION_MANIFEST_PATH, fn(*const c_char) -> i32, path.as_ptr());
        if e != 0 {
            return Err(format!("action manifest rejected (EVRInputError {e})"));
        }
        let handle = |idx: usize, name: &CStr| -> Result<u64, String> {
            let mut h = 0u64;
            let e = call!(self.input, idx, fn(*const c_char, *mut u64) -> i32, name.as_ptr(), &mut h);
            if e != 0 { Err(format!("{}: EVRInputError {e}", name.to_string_lossy())) } else { Ok(h) }
        };
        self.actions = Actions {
            set: handle(input::GET_ACTION_SET_HANDLE, c"/actions/main")?,
            point: handle(input::GET_ACTION_SET_HANDLE, c"/actions/point")?,
            scroll: handle(input::GET_ACTION_SET_HANDLE, c"/actions/scroll")?,
            right: handle(input::GET_ACTION_SET_HANDLE, c"/actions/right")?,
            pointer: handle(input::GET_ACTION_HANDLE, c"/actions/main/in/pointer")?,
            click: handle(input::GET_ACTION_HANDLE, c"/actions/point/in/click")?,
            grab: handle(input::GET_ACTION_HANDLE, c"/actions/point/in/grab")?,
            stick: handle(input::GET_ACTION_HANDLE, c"/actions/scroll/in/stick")?,
            menu: handle(input::GET_ACTION_HANDLE, c"/actions/main/in/menu")?,
            right_click: handle(input::GET_ACTION_HANDLE, c"/actions/right/in/rightclick")?,
            haptic: handle(input::GET_ACTION_HANDLE, c"/actions/main/out/haptic")?,
            hands: [
                handle(input::GET_INPUT_SOURCE_HANDLE, c"/user/hand/left")?,
                handle(input::GET_INPUT_SOURCE_HANDLE, c"/user/hand/right")?,
            ],
        };
        Ok(())
    }

    fn overlay_handle(&mut self, key: u64, name: &str) -> Result<u64, String> {
        if let Some(p) = self.overlays.get(&key) {
            return Ok(p.handle);
        }
        let k = cstring(name)?;
        let mut handle = 0u64;
        let e = call!(
            self.overlay,
            overlay::CREATE_OVERLAY,
            fn(*const c_char, *const c_char, *mut u64) -> i32,
            k.as_ptr(),
            k.as_ptr(),
            &mut handle
        );
        if e != 0 {
            return Err(format!("CreateOverlay {name}: EVROverlayError {e}"));
        }
        self.overlays.insert(key, Placed { handle, last: None, shown: false });
        Ok(handle)
    }

    /// Registers our `.vrmanifest` (permanent) and tells SteamVR this process is that app.
    pub fn register_app(&self, manifest: &Path, app_key: &str) -> Result<(), String> {
        let path = cstring(&manifest.to_string_lossy())?;
        let key = cstring(app_key)?;
        let e = call!(
            self.applications,
            applications::ADD_APPLICATION_MANIFEST,
            fn(*const c_char, u8) -> i32,
            path.as_ptr(),
            0
        );
        if e != 0 {
            return Err(format!("AddApplicationManifest: EVRApplicationError {e}"));
        }
        let e = call!(
            self.applications,
            applications::IDENTIFY_APPLICATION,
            fn(u32, *const c_char) -> i32,
            std::process::id(),
            key.as_ptr()
        );
        if e != 0 { Err(format!("IdentifyApplication: EVRApplicationError {e}")) } else { Ok(()) }
    }

    /// Per hand: the controls to take away from the game from the next poll on (see `Claims`).
    pub fn set_claims(&mut self, claims: [Claims; 2]) {
        self.claims = claims;
    }

    /// EVRInputError of the last action-state update (0 = fine).
    pub fn input_error(&self) -> i32 {
        self.input_error
    }

    /// Whether SteamVR starts us together with itself.
    pub fn auto_launch(&self, app_key: &str) -> bool {
        cstring(app_key).is_ok_and(|k| {
            call!(self.applications, applications::GET_APPLICATION_AUTO_LAUNCH, fn(*const c_char) -> u8, k.as_ptr())
                != 0
        })
    }

    /// Opens SteamVR's controller binding screen for this app (in the headset), where every action can be put on
    /// another button, per controller type.
    pub fn open_bindings(&self, app_key: &str) -> Result<(), String> {
        let k = cstring(app_key)?;
        let e = call!(
            self.input,
            input::OPEN_BINDING_UI,
            fn(*const c_char, u64, u64, bool) -> i32,
            k.as_ptr(),
            self.actions.set,
            0,
            false
        );
        if e != 0 { Err(format!("OpenBindingUI: EVRInputError {e}")) } else { Ok(()) }
    }

    pub fn set_auto_launch(&self, app_key: &str, on: bool) -> Result<(), String> {
        let k = cstring(app_key)?;
        let e = call!(
            self.applications,
            applications::SET_APPLICATION_AUTO_LAUNCH,
            fn(*const c_char, u8) -> i32,
            k.as_ptr(),
            u8::from(on)
        );
        if e != 0 { Err(format!("SetApplicationAutoLaunch: EVRApplicationError {e}")) } else { Ok(()) }
    }

    /// How many of the headset's rendered pixels one radian spans in the middle of the view (left eye, at SteamVR's
    /// render resolution): how much detail a panel of a given size and distance can show.
    pub fn pixels_per_radian(&self) -> Option<f32> {
        let (mut w, mut h) = (0u32, 0u32);
        call!(self.system, system::GET_RECOMMENDED_RENDER_TARGET_SIZE, fn(*mut u32, *mut u32) -> (), &mut w, &mut h);
        let (mut l, mut r, mut t, mut b) = (0f32, 0f32, 0f32, 0f32);
        call!(
            self.system,
            system::GET_PROJECTION_RAW,
            fn(i32, *mut f32, *mut f32, *mut f32, *mut f32) -> (),
            0,
            &mut l,
            &mut r,
            &mut t,
            &mut b
        );
        // Tangents: one tangent unit is one radian at the centre of the view.
        (w > 0 && r - l > 0.1).then(|| w as f32 / (r - l))
    }

    /// Vulkan extensions SteamVR needs on the instance (`None`) or on a physical device (its raw handle).
    pub fn vulkan_extensions(&self, physical_device: Option<u64>) -> Vec<String> {
        let mut buf = vec![0u8; 4096];
        let n = match physical_device {
            None => call!(
                self.compositor,
                compositor::GET_VULKAN_INSTANCE_EXTENSIONS_REQUIRED,
                fn(*mut u8, u32) -> u32,
                buf.as_mut_ptr(),
                buf.len() as u32
            ),
            Some(d) => call!(
                self.compositor,
                compositor::GET_VULKAN_DEVICE_EXTENSIONS_REQUIRED,
                fn(*mut c_void, *mut u8, u32) -> u32,
                d as *mut c_void,
                buf.as_mut_ptr(),
                buf.len() as u32
            ),
        };
        let text = &buf[..(n as usize).min(buf.len())];
        let text = text.split(|b| *b == 0).next().unwrap_or_default();
        String::from_utf8_lossy(text).split_whitespace().map(str::to_owned).collect()
    }

    /// The Vulkan physical device (raw handle) the headset is on, for `instance` (a raw `VkInstance` handle).
    pub fn vulkan_output_device(&self, instance: u64) -> u64 {
        let mut device = 0u64;
        call!(
            self.system,
            system::GET_OUTPUT_DEVICE,
            fn(*mut u64, i32, *mut c_void) -> (),
            &mut device,
            ffi::TEXTURE_VULKAN,
            instance as *mut c_void
        );
        device
    }

    /// Hands a Vulkan image to an existing overlay. The image must be in TRANSFER_SRC_OPTIMAL and its upload done.
    pub fn set_texture_vulkan(&mut self, key: u64, texture: &mut ffi::VulkanTexture) -> Result<(), String> {
        let p = self.overlays.get(&key).ok_or("no such overlay")?;
        let mut t = ffi::Texture {
            handle: (texture as *mut ffi::VulkanTexture).cast(),
            kind: ffi::TEXTURE_VULKAN,
            color_space: ffi::COLOR_SPACE_AUTO,
        };
        let e = call!(self.overlay, overlay::SET_OVERLAY_TEXTURE, fn(u64, *mut ffi::Texture) -> i32, p.handle, &mut t);
        if e != 0 { Err(format!("SetOverlayTexture: EVROverlayError {e}")) } else { Ok(()) }
    }

    /// DXGI adapter SteamVR renders on; textures must come from it.
    pub fn adapter_index(&self) -> Option<u32> {
        let mut i = -1i32;
        call!(self.system, system::GET_DXGI_OUTPUT_INFO, fn(*mut i32) -> (), &mut i);
        u32::try_from(i).ok()
    }

    /// Hands a D3D11 texture (`ID3D11Texture2D*`) to an existing overlay. Call after the GPU copy is flushed.
    pub fn set_texture_d3d11(&mut self, key: u64, texture: *mut c_void) -> Result<(), String> {
        let p = self.overlays.get(&key).ok_or("no such overlay")?;
        let mut t = ffi::Texture { handle: texture, kind: ffi::TEXTURE_DIRECTX, color_space: ffi::COLOR_SPACE_AUTO };
        let e = call!(self.overlay, overlay::SET_OVERLAY_TEXTURE, fn(u64, *mut ffi::Texture) -> i32, p.handle, &mut t);
        if e != 0 { Err(format!("SetOverlayTexture: EVROverlayError {e}")) } else { Ok(()) }
    }

    /// Paces the loop to the compositor (returns at frame start or after `timeout_ms`).
    pub fn wait_frame(&self, timeout_ms: u32) {
        let e = call!(self.overlay, overlay::WAIT_FRAME_SYNC, fn(u32) -> i32, timeout_ms);
        if e != 0 {
            std::thread::sleep(std::time::Duration::from_millis(u64::from(timeout_ms)));
        }
    }

    fn battery(&self, device: u32) -> Option<f32> {
        if device >= ffi::MAX_DEVICES {
            return None;
        }
        let mut e = 0i32;
        let v = call!(
            self.system,
            system::GET_FLOAT_TRACKED_DEVICE_PROPERTY,
            fn(u32, i32, *mut i32) -> f32,
            device,
            ffi::PROP_BATTERY_PERCENT,
            &mut e
        );
        // Some drivers report percent (0..100) instead of 0..1.
        (e == 0 && v.is_finite() && v >= 0.0).then(|| if v > 1.0 { (v / 100.0).min(1.0) } else { v })
    }

    /// The left and right controllers: by role, else (a driver that assigns no roles) the connected controllers in
    /// device order, left first.
    fn controller_devices(&self) -> [Option<u32>; 2] {
        let connected = |d: u32| call!(self.system, system::IS_TRACKED_DEVICE_CONNECTED, fn(u32) -> u8, d) != 0;
        let role = |r: i32| {
            let d = call!(self.system, system::GET_TRACKED_DEVICE_INDEX_FOR_CONTROLLER_ROLE, fn(i32) -> u32, r);
            (d < ffi::MAX_DEVICES && connected(d)).then_some(d)
        };
        let mut found = [role(ffi::ROLE_LEFT_HAND), role(ffi::ROLE_RIGHT_HAND)];
        let taken = found;
        let mut spare = (0..ffi::MAX_DEVICES).filter(|&d| {
            call!(self.system, system::GET_TRACKED_DEVICE_CLASS, fn(u32) -> i32, d) == ffi::CLASS_CONTROLLER
                && connected(d)
                && !taken.contains(&Some(d))
                && !matches!(
                    call!(self.system, system::GET_CONTROLLER_ROLE_FOR_TRACKED_DEVICE_INDEX, fn(u32) -> i32, d),
                    ffi::ROLE_LEFT_HAND | ffi::ROLE_RIGHT_HAND
                )
        });
        for slot in &mut found {
            if slot.is_none() {
                *slot = spare.next();
            }
        }
        found
    }

    fn controller_battery(&self, device: Option<u32>) -> Battery {
        let Some(d) = device else { return Battery::default() };
        let mut e = 0i32;
        let charging = call!(
            self.system,
            system::GET_BOOL_TRACKED_DEVICE_PROPERTY,
            fn(u32, i32, *mut i32) -> u8,
            d,
            ffi::PROP_IS_CHARGING,
            &mut e
        ) != 0
            && e == 0;
        Battery { on: true, level: self.battery(d), charging }
    }

    /// Someone is wearing the headset: its proximity sensor (or, without one, its movement) reports use. A
    /// headset lying on the desk while SteamVR starts does not count.
    pub fn worn(&self) -> bool {
        const USER_INTERACTION: i32 = 1;
        call!(self.system, system::GET_TRACKED_DEVICE_ACTIVITY_LEVEL, fn(u32) -> i32, ffi::HMD_INDEX as u32)
            == USER_INTERACTION
    }

    pub fn stats(&self) -> VrStats {
        let mut timing = FrameTiming { size: size_of::<FrameTiming>() as u32, ..FrameTiming::default() };
        let ok = call!(self.compositor, compositor::GET_FRAME_TIMING, fn(*mut FrameTiming, u32) -> u8, &mut timing, 0);
        let interval = timing.client_frame_interval_ms;
        let lowest_tracker = (0..ffi::MAX_DEVICES)
            .filter(|&d| {
                call!(self.system, system::GET_TRACKED_DEVICE_CLASS, fn(u32) -> i32, d) == ffi::CLASS_GENERIC_TRACKER
            })
            .filter_map(|d| self.battery(d))
            .reduce(f32::min);
        VrStats {
            game_fps: (ok != 0 && timing.presents > 0 && interval > 0.0).then(|| 1000.0 / interval),
            controllers: self.controller_devices().map(|d| self.controller_battery(d)),
            lowest_tracker,
            display_hz: self.display_hz(),
        }
    }

    fn display_hz(&self) -> Option<f32> {
        let mut e = 0i32;
        let v = call!(
            self.system,
            system::GET_FLOAT_TRACKED_DEVICE_PROPERTY,
            fn(u32, i32, *mut i32) -> f32,
            0,
            ffi::PROP_DISPLAY_FREQUENCY,
            &mut e
        );
        (e == 0 && v.is_finite() && v > 1.0).then_some(v)
    }

    fn digital(&self, action: u64, device: u64) -> bool {
        let d = self.digital_data(action, device);
        d.active != 0 && d.state != 0
    }

    fn digital_data(&self, action: u64, device: u64) -> DigitalData {
        let mut d = DigitalData::default();
        call!(
            self.input,
            input::GET_DIGITAL_ACTION_DATA,
            fn(u64, *mut DigitalData, u32, u64) -> i32,
            action,
            &mut d,
            size_of::<DigitalData>() as u32,
            device
        );
        d
    }

    /// Safety net for bindings SteamVR filed under the wrong app: a tracked controller whose trigger has not
    /// reached us for a few seconds (outside SteamVR's dashboard, which takes the buttons while open) gets the
    /// action manifest loaded again, once.
    fn check_bindings(&mut self, now_ms: u64, input: &FrameInput) {
        const UNBOUND_MS: u64 = 3000;
        let unbound = Hand::BOTH.iter().any(|h| {
            input.hands[h.index()].tracked
                && self.digital_data(self.actions.click, self.actions.hands[h.index()]).active == 0
        }) && !(call!(self.overlay, overlay::IS_DASHBOARD_VISIBLE, fn() -> u8) != 0);
        if !unbound {
            self.unbound_since = None;
            return;
        }
        let since = *self.unbound_since.get_or_insert(now_ms);
        if !self.reloaded
            && now_ms.saturating_sub(since) >= UNBOUND_MS
            && let Some(m) = self.manifest.clone()
        {
            self.reloaded = true;
            tracing::warn!("controller buttons not bound; loading the action manifest again");
            if let Err(e) = self.load_actions(&m) {
                tracing::warn!(%e, "action manifest reload failed");
            }
        }
    }

    fn set_visible(&mut self, key: u64, visible: bool) {
        let Some(p) = self.overlays.get_mut(&key) else { return };
        if p.shown != visible {
            let idx = if visible { overlay::SHOW_OVERLAY } else { overlay::HIDE_OVERLAY };
            call!(self.overlay, idx, fn(u64) -> i32, p.handle);
            p.shown = visible;
        }
    }

    fn set_transform(&self, handle: u64, world: Pose) {
        let m = Mat34(world.to_mat34());
        call!(
            self.overlay,
            overlay::SET_OVERLAY_TRANSFORM_ABSOLUTE,
            fn(u64, i32, *const Mat34) -> i32,
            handle,
            ffi::ORIGIN_STANDING,
            &m
        );
    }
}

impl VrBackend for OpenVr {
    fn poll(&mut self, now_ms: u64, input: &mut FrameInput) -> bool {
        input.now_ms = now_ms;
        let mut ev = VrEvent::default();
        while call!(
            self.system,
            system::POLL_NEXT_EVENT,
            fn(*mut VrEvent, u32) -> u8,
            &mut ev,
            size_of::<VrEvent>() as u32
        ) != 0
        {
            if { ev.kind } == ffi::EVENT_QUIT {
                call!(self.system, system::ACKNOWLEDGE_QUIT_EXITING, fn() -> ());
                return false;
            }
        }
        // Each set once per hand; a group of controls the overlay has taken gets overlay priority, so the game
        // stops seeing just those.
        let a = &self.actions;
        let mut sets = Vec::with_capacity(8);
        for h in Hand::BOTH {
            let c = self.claims[h.index()];
            for (set, taken) in [(a.set, false), (a.point, c.point), (a.scroll, c.stick), (a.right, c.right)] {
                sets.push(ActiveActionSet {
                    set,
                    restricted_to_device: a.hands[h.index()],
                    priority: if taken { OVERLAY_PRIORITY } else { 0 },
                    ..ActiveActionSet::default()
                });
            }
        }
        self.input_error = call!(
            self.input,
            input::UPDATE_ACTION_STATE,
            fn(*mut ActiveActionSet, u32, u32) -> i32,
            sets.as_mut_ptr(),
            size_of::<ActiveActionSet>() as u32,
            sets.len() as u32
        );
        for hand in Hand::BOTH {
            let dev = self.actions.hands[hand.index()];
            let mut pd = PoseData::default();
            call!(
                self.input,
                input::GET_POSE_ACTION_DATA_FOR_NEXT_FRAME,
                fn(u64, i32, *mut PoseData, u32, u64) -> i32,
                self.actions.pointer,
                ffi::ORIGIN_STANDING,
                &mut pd,
                size_of::<PoseData>() as u32,
                dev
            );
            let mut stick = AnalogData::default();
            call!(
                self.input,
                input::GET_ANALOG_ACTION_DATA,
                fn(u64, *mut AnalogData, u32, u64) -> i32,
                self.actions.stick,
                &mut stick,
                size_of::<AnalogData>() as u32,
                dev
            );
            let h = &mut input.hands[hand.index()];
            h.tracked = pd.active != 0 && pd.pose.valid != 0;
            if h.tracked {
                h.pose = Pose::from_mat34(pd.pose.to_absolute.0);
            }
            h.trigger = self.digital(self.actions.click, dev);
            h.grip = self.digital(self.actions.grab, dev);
            h.menu = self.digital(self.actions.menu, dev);
            h.right_click = self.digital(self.actions.right_click, dev);
            h.stick = if stick.active != 0 { [stick.xyz[0], stick.xyz[1]] } else { [0.0; 2] };
        }
        if let Some(head) = self.head_pose() {
            input.head = head;
        }
        self.check_bindings(now_ms, input);
        true
    }

    fn head_pose(&self) -> Option<Pose> {
        let mut poses = [TrackedPose::default(); 1];
        call!(
            self.system,
            system::GET_DEVICE_TO_ABSOLUTE_TRACKING_POSE,
            fn(i32, f32, *mut TrackedPose, u32) -> (),
            ffi::ORIGIN_STANDING,
            0.0,
            poses.as_mut_ptr(),
            1
        );
        let p = poses[ffi::HMD_INDEX];
        (p.valid != 0).then(|| Pose::from_mat34(p.to_absolute.0))
    }

    fn haptic(&mut self, hand: Hand) {
        call!(
            self.input,
            input::TRIGGER_HAPTIC_VIBRATION_ACTION,
            fn(u64, f32, f32, f32, f32, u64) -> i32,
            self.actions.haptic,
            0.0,
            0.012,
            160.0,
            0.35,
            self.actions.hands[hand.index()]
        );
    }

    fn place_panel(&mut self, panel: &Panel, world: Pose) {
        let key = u64::from(panel.id);
        let handle = match self.overlays.get(&key) {
            Some(p) => p.handle,
            None => match self.overlay_handle(key, &format!("dvr.panel.{}", panel.id)) {
                Ok(h) => {
                    if matches!(panel.source, Source::Keyboard | Source::Toolbar | Source::Banner) {
                        call!(self.overlay, overlay::SET_OVERLAY_SORT_ORDER, fn(u64, u32) -> i32, h, KEYBOARD_SORT);
                    }
                    h
                }
                Err(e) => {
                    tracing::warn!(%e, "overlay unavailable");
                    return;
                }
            },
        };
        let last = self.overlays.get(&key).and_then(|p| p.last);
        let changed = |f: fn(&Panel) -> f32| last.is_none_or(|(l, _)| f(&l) != f(panel));
        if changed(|p| p.width_m) {
            call!(self.overlay, overlay::SET_OVERLAY_WIDTH_IN_METERS, fn(u64, f32) -> i32, handle, panel.width_m);
        }
        if changed(|p| p.curvature) {
            call!(self.overlay, overlay::SET_OVERLAY_CURVATURE, fn(u64, f32) -> i32, handle, panel.curvature);
        }
        // Faded panels (hide-all, the watch's look-to-show) dim through the overlay's alpha.
        let alpha = |p: &Panel| p.opacity * (1.0 - p.dim);
        if last.is_none_or(|(l, _)| alpha(&l) != alpha(panel)) {
            call!(self.overlay, overlay::SET_OVERLAY_ALPHA, fn(u64, f32) -> i32, handle, alpha(panel));
        }
        // Head-locked panels ride on the headset in the compositor (no lag); everything else is placed in the
        // room each frame.
        if panel.anchor == Anchor::Head {
            if last.is_none_or(|(l, _)| l.anchor != Anchor::Head || l.pose != panel.pose) {
                let m = Mat34(panel.pose.to_mat34());
                call!(
                    self.overlay,
                    overlay::SET_OVERLAY_TRANSFORM_TRACKED_DEVICE_RELATIVE,
                    fn(u64, u32, *const Mat34) -> i32,
                    handle,
                    ffi::HMD_INDEX as u32,
                    &m
                );
            }
        } else if last.is_none_or(|(l, w)| w != world || l.anchor == Anchor::Head) {
            self.set_transform(handle, world);
        }
        if let Some(p) = self.overlays.get_mut(&key) {
            p.last = Some((*panel, world));
        }
        self.set_visible(key, panel.shown());
    }

    fn remove_panel(&mut self, id: u32) {
        if let Some(p) = self.overlays.remove(&u64::from(id)) {
            call!(self.overlay, overlay::DESTROY_OVERLAY, fn(u64) -> i32, p.handle);
        }
    }

    fn place_cursor(&mut self, hand: Hand, at: Option<Pose>) {
        let key = cursor_key(hand);
        if let (Some(world), Some(p)) = (at, self.overlays.get(&key)) {
            self.set_transform(p.handle, world);
        }
        self.set_visible(key, at.is_some());
    }
}

impl Drop for OpenVr {
    fn drop(&mut self) {
        for (_, p) in self.overlays.drain() {
            call!(self.overlay, overlay::DESTROY_OVERLAY, fn(u64) -> i32, p.handle);
        }
        // SAFETY: core was initialized in `init` and is cleaned up exactly once.
        unsafe { ((*(*self.core).vtbl).cleanup)(self.core) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uninstalling_drops_only_our_manifest_from_steamvrs_list() {
        let dir = std::env::temp_dir().join(format!("dvr-appconfig-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("appconfig.json");
        let ours = r"C:\Users\Someone\AppData\Roaming\DesktopVR\config\desktopvr.vrmanifest";
        let theirs = r"C:\Program Files\Other\other.vrmanifest";
        let list = serde_json::json!({ "manifest_paths": [ours.to_uppercase().replace('\\', "/"), theirs] });
        std::fs::write(&file, list.to_string()).expect("write");
        drop_manifest(&file, Path::new(ours)).expect("drop");
        let back: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&file).expect("read")).expect("json");
        assert_eq!(back["manifest_paths"], serde_json::json!([theirs]));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
