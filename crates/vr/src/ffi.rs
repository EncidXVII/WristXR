//! Minimal OpenVR C ABI, from third_party/openvr/openvr_capi.h (BSD-3).
//! Function tables are indexed; `table_indices_match_header` checks every index against the header.
use std::ffi::{CStr, c_char, c_void};

pub const APP_OVERLAY: i32 = 2;
/// Never starts SteamVR: init fails when it is not running.
pub const APP_BACKGROUND: i32 = 3;
pub const ORIGIN_STANDING: i32 = 1;
pub const TEXTURE_DIRECTX: i32 = 0;
pub const COLOR_SPACE_AUTO: i32 = 0;
pub const EVENT_QUIT: u32 = 700;
pub const HMD_INDEX: usize = 0;
pub const MAX_DEVICES: u32 = 64;
pub const ROLE_LEFT_HAND: i32 = 1;
pub const ROLE_RIGHT_HAND: i32 = 2;
pub const CLASS_CONTROLLER: i32 = 2;
pub const CLASS_GENERIC_TRACKER: i32 = 3;
pub const PROP_IS_CHARGING: i32 = 1011;
pub const PROP_BATTERY_PERCENT: i32 = 1012;
/// Prop_DisplayFrequency_Float (HMD refresh rate, Hz).
pub const PROP_DISPLAY_FREQUENCY: i32 = 2002;

pub const SYSTEM_VERSION: &CStr = c"FnTable:IVRSystem_026";
pub const OVERLAY_VERSION: &CStr = c"FnTable:IVROverlay_028";
pub const INPUT_VERSION: &CStr = c"FnTable:IVRInput_011";
pub const COMPOSITOR_VERSION: &CStr = c"FnTable:IVRCompositor_029";
pub const APPLICATIONS_VERSION: &CStr = c"FnTable:IVRApplications_008";
pub const CLIENT_CORE_VERSION: &CStr = c"IVRClientCore_003";

pub mod system {
    pub const GET_RECOMMENDED_RENDER_TARGET_SIZE: usize = 0;
    pub const GET_PROJECTION_RAW: usize = 2;
    pub const GET_DXGI_OUTPUT_INFO: usize = 8;
    pub const GET_DEVICE_TO_ABSOLUTE_TRACKING_POSE: usize = 12;
    pub const GET_TRACKED_DEVICE_ACTIVITY_LEVEL: usize = 16;
    pub const GET_TRACKED_DEVICE_INDEX_FOR_CONTROLLER_ROLE: usize = 18;
    pub const GET_CONTROLLER_ROLE_FOR_TRACKED_DEVICE_INDEX: usize = 19;
    pub const GET_TRACKED_DEVICE_CLASS: usize = 20;
    pub const IS_TRACKED_DEVICE_CONNECTED: usize = 21;
    pub const GET_BOOL_TRACKED_DEVICE_PROPERTY: usize = 22;
    pub const GET_FLOAT_TRACKED_DEVICE_PROPERTY: usize = 23;
    pub const POLL_NEXT_EVENT: usize = 30;
    pub const ACKNOWLEDGE_QUIT_EXITING: usize = 47;
}
pub mod overlay {
    pub const CREATE_OVERLAY: usize = 1;
    pub const DESTROY_OVERLAY: usize = 3;
    pub const SET_OVERLAY_ALPHA: usize = 16;
    pub const SET_OVERLAY_SORT_ORDER: usize = 20;
    pub const SET_OVERLAY_WIDTH_IN_METERS: usize = 22;
    pub const SET_OVERLAY_CURVATURE: usize = 24;
    pub const SET_OVERLAY_TRANSFORM_ABSOLUTE: usize = 33;
    pub const SET_OVERLAY_TRANSFORM_TRACKED_DEVICE_RELATIVE: usize = 35;
    pub const SHOW_OVERLAY: usize = 43;
    pub const HIDE_OVERLAY: usize = 44;
    pub const WAIT_FRAME_SYNC: usize = 47;
    pub const SET_OVERLAY_TEXTURE: usize = 60;
    pub const IS_DASHBOARD_VISIBLE: usize = 68;
}
pub mod applications {
    pub const ADD_APPLICATION_MANIFEST: usize = 0;
    pub const REMOVE_APPLICATION_MANIFEST: usize = 1;
    pub const IDENTIFY_APPLICATION: usize = 11;
    pub const SET_APPLICATION_AUTO_LAUNCH: usize = 17;
    pub const GET_APPLICATION_AUTO_LAUNCH: usize = 18;
}
pub mod compositor {
    pub const GET_FRAME_TIMING: usize = 10;
}
pub mod input {
    pub const SET_ACTION_MANIFEST_PATH: usize = 0;
    pub const GET_ACTION_SET_HANDLE: usize = 1;
    pub const GET_ACTION_HANDLE: usize = 2;
    pub const GET_INPUT_SOURCE_HANDLE: usize = 3;
    pub const UPDATE_ACTION_STATE: usize = 4;
    pub const GET_DIGITAL_ACTION_DATA: usize = 5;
    pub const GET_ANALOG_ACTION_DATA: usize = 6;
    pub const GET_POSE_ACTION_DATA_FOR_NEXT_FRAME: usize = 8;
    pub const TRIGGER_HAPTIC_VIBRATION_ACTION: usize = 23;
    pub const OPEN_BINDING_UI: usize = 32;
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Mat34(pub [[f32; 4]; 3]);

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TrackedPose {
    pub to_absolute: Mat34,
    pub velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub result: i32,
    pub valid: u8,
    pub connected: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct DigitalData {
    pub active: u8,
    pub origin: u64,
    pub state: u8,
    pub changed: u8,
    pub update_time: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AnalogData {
    pub active: u8,
    pub origin: u64,
    pub xyz: [f32; 3],
    pub delta: [f32; 3],
    pub update_time: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PoseData {
    pub active: u8,
    pub origin: u64,
    pub pose: TrackedPose,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ActiveActionSet {
    pub set: u64,
    pub restricted_to_device: u64,
    pub secondary: u64,
    pub padding: u32,
    pub priority: i32,
}

/// `Compositor_FrameTiming`; only the fields we read are named.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FrameTiming {
    pub size: u32,
    pub frame_index: u32,
    pub presents: u32,
    pub mispresented: u32,
    pub dropped: u32,
    pub reprojection_flags: u32,
    pub system_time_s: f64,
    /// Pre-submit GPU .. compositor idle CPU (6 fields).
    pub render_ms: [f32; 6],
    pub client_frame_interval_ms: f32,
    /// Present-call CPU .. compositor render start (9 fields).
    pub timeline_ms: [f32; 9],
    pub hmd_pose: TrackedPose,
    pub vsyncs_ready: u32,
    pub vsyncs_to_first_view: u32,
    pub transfer_latency_ms: f32,
}

#[repr(C)]
pub struct Texture {
    pub handle: *mut c_void,
    pub kind: i32,
    pub color_space: i32,
}

/// `VREvent_t`: 4-byte packed on Linux/macOS (header keeps an old mis-packing).
#[cfg_attr(unix, repr(C, packed(4)))]
#[cfg_attr(not(unix), repr(C))]
#[derive(Clone, Copy, Default)]
pub struct VrEvent {
    pub kind: u32,
    pub device: u32,
    pub age: f32,
    pub data: [u64; 6],
}

#[derive(Clone, Copy)]
pub struct Table(pub *const *const c_void);

impl Table {
    /// # Safety
    /// `F` must be the `extern "system"` signature declared at `index` in this table's header struct.
    pub unsafe fn get<F: Copy>(self, index: usize) -> F {
        // SAFETY: the table holds function pointers; caller guarantees the type at `index`.
        unsafe { std::mem::transmute_copy(&*self.0.add(index)) }
    }
}

/// `IVRClientCore_003` C++ vtable (Itanium and MSVC x64 share this layout for single inheritance).
#[repr(C)]
pub struct ClientCoreVtbl {
    pub init: unsafe extern "system" fn(*mut ClientCore, i32, *const c_char) -> i32,
    pub cleanup: unsafe extern "system" fn(*mut ClientCore),
    pub is_interface_version_valid: unsafe extern "system" fn(*mut ClientCore, *const c_char) -> i32,
    pub get_generic_interface: unsafe extern "system" fn(*mut ClientCore, *const c_char, *mut i32) -> *mut c_void,
}

#[repr(C)]
pub struct ClientCore {
    pub vtbl: *const ClientCoreVtbl,
}

pub type ClientCoreFactory = unsafe extern "C" fn(*const c_char, *mut i32) -> *mut c_void;

#[cfg(windows)]
mod os {
    use std::ffi::{c_char, c_void};
    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn LoadLibraryW(name: *const u16) -> *mut c_void;
        pub fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    }
}

#[cfg(unix)]
mod os {
    use std::ffi::{c_char, c_int, c_void};
    pub const RTLD_NOW: c_int = 2;
    #[link(name = "dl")]
    unsafe extern "C" {
        pub fn dlopen(file: *const c_char, mode: c_int) -> *mut c_void;
        pub fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    }
}

/// Loads the runtime's client library and returns its `VRClientCoreFactory`. Never unloaded.
pub fn load_factory(path: &std::path::Path) -> Result<ClientCoreFactory, String> {
    #[cfg(windows)]
    let symbol = {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: NUL-terminated wide path; the returned module stays loaded for the process lifetime.
        let module = unsafe { os::LoadLibraryW(wide.as_ptr()) };
        if module.is_null() {
            return Err(format!("cannot load {}", path.display()));
        }
        // SAFETY: valid module handle and NUL-terminated symbol name.
        unsafe { os::GetProcAddress(module, c"VRClientCoreFactory".as_ptr()) }
    };
    #[cfg(unix)]
    let symbol = {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
        // SAFETY: NUL-terminated path; handle intentionally never closed.
        let handle = unsafe { os::dlopen(c.as_ptr(), os::RTLD_NOW) };
        if handle.is_null() {
            return Err(format!("cannot load {}", path.display()));
        }
        // SAFETY: valid handle and NUL-terminated symbol name.
        unsafe { os::dlsym(handle, c"VRClientCoreFactory".as_ptr()) }
    };
    if symbol.is_null() {
        return Err("VRClientCoreFactory not exported".into());
    }
    // SAFETY: the exported symbol has the documented factory signature.
    Ok(unsafe { std::mem::transmute::<*mut c_void, ClientCoreFactory>(symbol) })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = include_str!("../../../third_party/openvr/openvr_capi.h");

    fn table(name: &str) -> Vec<String> {
        let start = HEADER.find(&format!("struct VR_{name}_FnTable\n")).expect("table in header");
        let body = &HEADER[start..];
        let body = &body[..body.find("};").expect("table end")];
        body.split("(OPENVR_FNTABLE_CALLTYPE *")
            .skip(1)
            .map(|s| s[..s.find(')').expect("name end")].to_owned())
            .collect()
    }

    #[test]
    fn table_indices_match_header() {
        let checks: &[(&str, &str, usize)] = &[
            ("IVRSystem", "GetDXGIOutputInfo", system::GET_DXGI_OUTPUT_INFO),
            ("IVRSystem", "GetDeviceToAbsoluteTrackingPose", system::GET_DEVICE_TO_ABSOLUTE_TRACKING_POSE),
            ("IVRSystem", "PollNextEvent", system::POLL_NEXT_EVENT),
            ("IVRSystem", "GetTrackedDeviceActivityLevel", system::GET_TRACKED_DEVICE_ACTIVITY_LEVEL),
            (
                "IVRSystem",
                "GetTrackedDeviceIndexForControllerRole",
                system::GET_TRACKED_DEVICE_INDEX_FOR_CONTROLLER_ROLE,
            ),
            ("IVRSystem", "GetTrackedDeviceClass", system::GET_TRACKED_DEVICE_CLASS),
            (
                "IVRSystem",
                "GetControllerRoleForTrackedDeviceIndex",
                system::GET_CONTROLLER_ROLE_FOR_TRACKED_DEVICE_INDEX,
            ),
            ("IVRSystem", "IsTrackedDeviceConnected", system::IS_TRACKED_DEVICE_CONNECTED),
            ("IVRSystem", "GetBoolTrackedDeviceProperty", system::GET_BOOL_TRACKED_DEVICE_PROPERTY),
            ("IVRSystem", "GetFloatTrackedDeviceProperty", system::GET_FLOAT_TRACKED_DEVICE_PROPERTY),
            ("IVRCompositor", "GetFrameTiming", compositor::GET_FRAME_TIMING),
            ("IVRApplications", "AddApplicationManifest", applications::ADD_APPLICATION_MANIFEST),
            ("IVRApplications", "RemoveApplicationManifest", applications::REMOVE_APPLICATION_MANIFEST),
            ("IVRApplications", "IdentifyApplication", applications::IDENTIFY_APPLICATION),
            ("IVRApplications", "SetApplicationAutoLaunch", applications::SET_APPLICATION_AUTO_LAUNCH),
            ("IVRApplications", "GetApplicationAutoLaunch", applications::GET_APPLICATION_AUTO_LAUNCH),
            ("IVRSystem", "AcknowledgeQuit_Exiting", system::ACKNOWLEDGE_QUIT_EXITING),
            ("IVROverlay", "CreateOverlay", overlay::CREATE_OVERLAY),
            ("IVRSystem", "GetRecommendedRenderTargetSize", system::GET_RECOMMENDED_RENDER_TARGET_SIZE),
            ("IVRSystem", "GetProjectionRaw", system::GET_PROJECTION_RAW),
            ("IVROverlay", "IsDashboardVisible", overlay::IS_DASHBOARD_VISIBLE),
            ("IVROverlay", "DestroyOverlay", overlay::DESTROY_OVERLAY),
            ("IVROverlay", "SetOverlayAlpha", overlay::SET_OVERLAY_ALPHA),
            ("IVROverlay", "SetOverlaySortOrder", overlay::SET_OVERLAY_SORT_ORDER),
            ("IVROverlay", "SetOverlayWidthInMeters", overlay::SET_OVERLAY_WIDTH_IN_METERS),
            ("IVROverlay", "SetOverlayCurvature", overlay::SET_OVERLAY_CURVATURE),
            ("IVROverlay", "SetOverlayTransformAbsolute", overlay::SET_OVERLAY_TRANSFORM_ABSOLUTE),
            (
                "IVROverlay",
                "SetOverlayTransformTrackedDeviceRelative",
                overlay::SET_OVERLAY_TRANSFORM_TRACKED_DEVICE_RELATIVE,
            ),
            ("IVROverlay", "ShowOverlay", overlay::SHOW_OVERLAY),
            ("IVROverlay", "HideOverlay", overlay::HIDE_OVERLAY),
            ("IVROverlay", "WaitFrameSync", overlay::WAIT_FRAME_SYNC),
            ("IVROverlay", "SetOverlayTexture", overlay::SET_OVERLAY_TEXTURE),
            ("IVRInput", "SetActionManifestPath", input::SET_ACTION_MANIFEST_PATH),
            ("IVRInput", "GetActionSetHandle", input::GET_ACTION_SET_HANDLE),
            ("IVRInput", "GetActionHandle", input::GET_ACTION_HANDLE),
            ("IVRInput", "GetInputSourceHandle", input::GET_INPUT_SOURCE_HANDLE),
            ("IVRInput", "UpdateActionState", input::UPDATE_ACTION_STATE),
            ("IVRInput", "GetDigitalActionData", input::GET_DIGITAL_ACTION_DATA),
            ("IVRInput", "GetAnalogActionData", input::GET_ANALOG_ACTION_DATA),
            ("IVRInput", "GetPoseActionDataForNextFrame", input::GET_POSE_ACTION_DATA_FOR_NEXT_FRAME),
            ("IVRInput", "TriggerHapticVibrationAction", input::TRIGGER_HAPTIC_VIBRATION_ACTION),
            ("IVRInput", "OpenBindingUI", input::OPEN_BINDING_UI),
        ];
        for (t, f, i) in checks {
            assert_eq!(table(t).get(*i).map(String::as_str), Some(*f), "{t}::{f}");
        }
        for (v, t) in [
            (SYSTEM_VERSION, "IVRSystem"),
            (OVERLAY_VERSION, "IVROverlay"),
            (INPUT_VERSION, "IVRInput"),
            (COMPOSITOR_VERSION, "IVRCompositor"),
            (APPLICATIONS_VERSION, "IVRApplications"),
        ] {
            let name = v.to_str().expect("ascii").trim_start_matches("FnTable:");
            assert!(HEADER.contains(&format!("{t}_Version = \"{name}\"")), "{name}");
        }
    }

    #[test]
    fn struct_sizes_match_abi() {
        use std::mem::size_of;
        assert_eq!(size_of::<TrackedPose>(), 80);
        assert_eq!(size_of::<DigitalData>(), 24);
        assert_eq!(size_of::<AnalogData>(), 48);
        assert_eq!(size_of::<PoseData>(), 96);
        assert_eq!(size_of::<ActiveActionSet>(), 32);
        assert_eq!(size_of::<FrameTiming>(), 192);
        assert_eq!(size_of::<VrEvent>(), if cfg!(unix) { 60 } else { 64 });
    }
}
