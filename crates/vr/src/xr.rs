// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! OpenXR overlay backend, for Linux runtimes without SteamVR (Monado, WiVRn): an overlay session
//! (`XR_EXTX_overlay`) on the app's own Vulkan device, panels and cursors as quad layers, controllers through the
//! ordinary action system (Monado keeps every overlay session focused). The OpenXR loader is opened at run time,
//! so the app still starts where none is installed.

use crate::{CURSOR_WIDTH_M, VrStats, cursor_key};
use openxr as xr;
use openxr::sys::Handle as _;
use std::any::Any;
use std::collections::HashMap;
use std::ffi::c_void;
use std::time::Duration;
use wxr_core::backend::VrBackend;
use wxr_core::engine::FrameInput;
use wxr_core::math::{Pose, Quat, Vec3};
use wxr_core::panel::{Anchor, Hand, Panel, Source};

/// The clear 1x1 layer shown when nothing else is: Monado misbehaves on frames without layers. Panels use their id,
/// cursors the two keys above this one.
pub const BLANK_KEY: u64 = u64::MAX - 2;
/// `sessionLayersPlacement`: above the scene app's layers (the value WlxOverlay-S uses).
const LAYERS_PLACEMENT: u32 = 5;
/// Swapchain formats in order of preference (`VK_FORMAT_B8G8R8A8_SRGB`, `VK_FORMAT_B8G8R8A8_UNORM`): both take the
/// BGRA bytes with a plain copy. sRGB first, since SteamVR shows the same bytes as sRGB (`ColorSpace_Auto` on an
/// 8-bit texture).
const FORMATS: [u32; 2] = [50, 44];

fn xr_err(call: &'static str) -> impl Fn(xr::sys::Result) -> String {
    move |r| format!("{call}: {r}")
}

/// The Vulkan objects the session runs on, as raw handles (from `wxr-gfx-vk`).
pub struct VulkanHandles {
    pub instance: u64,
    pub physical_device: u64,
    pub device: u64,
    pub queue_family: u32,
}

struct Actions {
    set: xr::ActionSet,
    pointer: xr::Action<xr::Posef>,
    click: xr::Action<bool>,
    grab: xr::Action<bool>,
    stick: xr::Action<xr::Vector2f>,
    menu: xr::Action<bool>,
    right_click: xr::Action<bool>,
    haptic: xr::Action<xr::Haptic>,
    hands: [xr::Path; 2],
}

#[derive(Clone, Copy)]
enum Act {
    Pointer,
    Click,
    Grab,
    Stick,
    Menu,
    Right,
    Haptic,
}

/// Default buttons per controller, as in the SteamVR bindings (`bindings/`): the trigger clicks, the grip grabs, the
/// stick scrolls, the left hand's upper face button opens the menu, A/X right-click. `*` is either hand. Wands have
/// no right click here: their trackpad's lower half needs a dpad binding.
const PROFILES: [(&str, &[(Act, &str)]); 6] = [
    ("khr/simple_controller", &[(Act::Click, "*/input/select/click"), (Act::Menu, "left/input/menu/click")]),
    (
        "valve/index_controller",
        &[
            (Act::Click, "*/input/trigger/click"),
            (Act::Grab, "*/input/squeeze/value"),
            (Act::Stick, "*/input/thumbstick"),
            (Act::Menu, "left/input/b/click"),
            (Act::Right, "*/input/a/click"),
        ],
    ),
    (
        "oculus/touch_controller",
        &[
            (Act::Click, "*/input/trigger/value"),
            (Act::Grab, "*/input/squeeze/value"),
            (Act::Stick, "*/input/thumbstick"),
            (Act::Menu, "left/input/y/click"),
            (Act::Right, "left/input/x/click"),
            (Act::Right, "right/input/a/click"),
        ],
    ),
    (
        "htc/vive_controller",
        &[
            (Act::Click, "*/input/trigger/click"),
            (Act::Grab, "*/input/squeeze/click"),
            (Act::Stick, "*/input/trackpad"),
            (Act::Menu, "left/input/menu/click"),
        ],
    ),
    (
        "microsoft/motion_controller",
        &[
            (Act::Click, "*/input/trigger/value"),
            (Act::Grab, "*/input/squeeze/click"),
            (Act::Stick, "*/input/thumbstick"),
            (Act::Menu, "left/input/menu/click"),
        ],
    ),
    (
        "bytedance/pico4_controller",
        &[
            (Act::Click, "*/input/trigger/value"),
            (Act::Grab, "*/input/squeeze/value"),
            (Act::Stick, "*/input/thumbstick"),
            (Act::Menu, "left/input/y/click"),
            (Act::Right, "left/input/x/click"),
            (Act::Right, "right/input/a/click"),
        ],
    ),
];

impl Actions {
    fn new(instance: &xr::Instance) -> xr::Result<Self> {
        let hands = [instance.string_to_path(xr::USER_HAND_LEFT)?, instance.string_to_path(xr::USER_HAND_RIGHT)?];
        let set = instance.create_action_set("wristxr", "WristXR", 0)?;
        Ok(Self {
            pointer: set.create_action("pointer", "Pointer", &hands)?,
            click: set.create_action("click", "Click / type", &hands)?,
            grab: set.create_action("grab", "Grab panel", &hands)?,
            stick: set.create_action("stick", "Scroll / push-pull", &hands)?,
            menu: set.create_action("menu", "Menu (double-tap)", &hands)?,
            right_click: set.create_action("right_click", "Right click", &hands)?,
            haptic: set.create_action("haptic", "Haptic", &hands)?,
            set,
            hands,
        })
    }

    /// Suggests the bindings for every controller type the runtime knows; one it rejects is skipped.
    fn suggest(&self, instance: &xr::Instance, pico: bool) {
        let common = [(Act::Pointer, "*/input/aim/pose"), (Act::Haptic, "*/output/haptic")];
        for (profile, inputs) in PROFILES {
            // The Pico 4 profile comes with an extension.
            if profile.starts_with("bytedance/") && !pico {
                continue;
            }
            let mut bindings = Vec::new();
            for (act, path) in common.iter().chain(inputs.iter()) {
                let hands = match path.strip_prefix("*/") {
                    Some(rest) => vec![format!("left/{rest}"), format!("right/{rest}")],
                    None => vec![(*path).to_owned()],
                };
                for p in hands {
                    let Ok(p) = instance.string_to_path(&format!("/user/hand/{p}")) else { continue };
                    bindings.push(match act {
                        Act::Pointer => xr::Binding::new(&self.pointer, p),
                        Act::Click => xr::Binding::new(&self.click, p),
                        Act::Grab => xr::Binding::new(&self.grab, p),
                        Act::Stick => xr::Binding::new(&self.stick, p),
                        Act::Menu => xr::Binding::new(&self.menu, p),
                        Act::Right => xr::Binding::new(&self.right_click, p),
                        Act::Haptic => xr::Binding::new(&self.haptic, p),
                    });
                }
            }
            let result = instance
                .string_to_path(&format!("/interaction_profiles/{profile}"))
                .and_then(|p| instance.suggest_interaction_profile_bindings(p, &bindings));
            if let Err(e) = result {
                tracing::warn!(%e, profile, "OpenXR bindings not suggested");
            }
        }
    }
}

/// One quad: where it goes (from `place_panel` or `place_cursor`) and its swapchain (from `acquire`).
#[derive(Default)]
struct Layer {
    chain: Option<Chain>,
    /// In the stage, or relative to the head (`head`).
    pose: Pose,
    head: bool,
    /// Width and height in metres.
    size: (f32, f32),
    alpha: f32,
    shown: bool,
    /// Drawn in this order: panels, then the keyboard and bars, then cursors (SteamVR's sort order).
    order: u8,
}

struct Chain {
    swapchain: xr::Swapchain<xr::Vulkan>,
    images: Vec<u64>,
    size: (u32, u32),
    /// An image has been released, so there is a picture to show.
    ready: bool,
}

/// Everything that holds the session, so it can all go before the Vulkan device it runs on.
struct Live {
    session: xr::Session<xr::Vulkan>,
    waiter: xr::FrameWaiter,
    stream: xr::FrameStream<xr::Vulkan>,
    stage: xr::Space,
    view: xr::Space,
    pointers: [xr::Space; 2],
    format: u32,
    layers: HashMap<u64, Layer>,
}

pub struct OpenXr {
    // Fields drop in this order: the session first, the Vulkan device (`gpu`) last.
    live: Option<Live>,
    actions: Actions,
    instance: xr::Instance,
    system: xr::SystemId,
    blend: xr::EnvironmentBlendMode,
    /// The runtime can fade layers (`XR_KHR_composition_layer_color_scale_bias`).
    color_scale: bool,
    running: bool,
    /// A frame is begun (`wait_frame`) and not yet ended.
    begun: bool,
    /// Display time of the last frame begun: poses are predicted for it.
    time: Option<xr::Time>,
    head: Option<Pose>,
    gpu: Option<Box<dyn Any>>,
}

fn to_pose(p: xr::Posef) -> Pose {
    let (o, v) = (p.orientation, p.position);
    Pose::new(Vec3::new(v.x, v.y, v.z), Quat { x: o.x, y: o.y, z: o.z, w: o.w })
}

fn to_xr(p: Pose) -> xr::Posef {
    let (q, v) = (p.rot, p.pos);
    xr::Posef {
        orientation: xr::Quaternionf { x: q.x, y: q.y, z: q.z, w: q.w },
        position: xr::Vector3f { x: v.x, y: v.y, z: v.z },
    }
}

fn located(l: &xr::SpaceLocation) -> bool {
    l.location_flags.contains(xr::SpaceLocationFlags::ORIENTATION_VALID | xr::SpaceLocationFlags::POSITION_VALID)
}

impl OpenXr {
    /// Opens the runtime (no session yet: that needs the Vulkan device, see `start`) and sets up the actions.
    pub fn connect() -> Result<Self, String> {
        // SAFETY: the system's OpenXR loader (`libopenxr_loader.so.1`), which follows the OpenXR specification.
        let entry = unsafe { xr::Entry::load(&()) }.map_err(|e| format!("no OpenXR loader ({e})"))?;
        let available = entry.enumerate_extensions().map_err(xr_err("xrEnumerateInstanceExtensionProperties"))?;
        if !available.extx_overlay {
            return Err("the OpenXR runtime cannot show overlays (no XR_EXTX_overlay)".into());
        }
        if !available.khr_vulkan_enable {
            return Err("the OpenXR runtime has no Vulkan support (no XR_KHR_vulkan_enable)".into());
        }
        let mut wanted = xr::ExtensionSet::default();
        wanted.extx_overlay = true;
        wanted.khr_vulkan_enable = true;
        wanted.khr_composition_layer_color_scale_bias = available.khr_composition_layer_color_scale_bias;
        wanted.bd_controller_interaction = available.bd_controller_interaction;
        let app = xr::ApplicationInfo { application_name: "WristXR", engine_name: "WristXR", ..Default::default() };
        let instance = entry.create_instance(&app, &wanted, &[], &()).map_err(xr_err("xrCreateInstance"))?;
        let system = instance.system(xr::FormFactor::HEAD_MOUNTED_DISPLAY).map_err(xr_err("xrGetSystem"))?;
        // The spec asks for the graphics requirements before the graphics device and the session.
        let needs = instance
            .graphics_requirements::<xr::Vulkan>(system)
            .map_err(xr_err("xrGetVulkanGraphicsRequirementsKHR"))?;
        let min = needs.min_api_version_supported;
        if (min.major(), min.minor()) > (1, 1) {
            return Err(format!("the OpenXR runtime needs Vulkan {}.{}", min.major(), min.minor()));
        }
        let blend = instance
            .enumerate_environment_blend_modes(system, xr::ViewConfigurationType::PRIMARY_STEREO)
            .ok()
            .and_then(|m| m.first().copied())
            .unwrap_or(xr::EnvironmentBlendMode::OPAQUE);
        let actions = Actions::new(&instance).map_err(xr_err("OpenXR actions"))?;
        actions.suggest(&instance, available.bd_controller_interaction);
        let name = instance.properties().map(|p| p.runtime_name).unwrap_or_default();
        tracing::info!(runtime = %name, "OpenXR overlay");
        Ok(Self {
            live: None,
            actions,
            instance,
            system,
            blend,
            color_scale: available.khr_composition_layer_color_scale_bias,
            running: false,
            begun: false,
            time: None,
            head: None,
            gpu: None,
        })
    }

    /// Vulkan extensions the runtime needs on the instance, or (`device`) on the device.
    pub fn vulkan_extensions(&self, device: bool) -> Vec<String> {
        let list = if device {
            self.instance.vulkan_legacy_device_extensions(self.system)
        } else {
            self.instance.vulkan_legacy_instance_extensions(self.system)
        };
        match list {
            Ok(s) => s.split_whitespace().map(str::to_owned).collect(),
            Err(e) => {
                tracing::warn!(%e, "OpenXR Vulkan extensions unknown");
                Vec::new()
            }
        }
    }

    /// The Vulkan physical device (raw handle) the headset is on, for `instance` (a raw `VkInstance` handle).
    pub fn vulkan_output_device(&self, instance: u64) -> u64 {
        // SAFETY: `instance` is a live VkInstance made with the extensions `vulkan_extensions` asked for.
        let device = unsafe { self.instance.vulkan_graphics_device(self.system, instance as *const c_void) };
        device.map_or(0, |d| d as u64)
    }

    /// Starts the overlay session on the app's Vulkan device. `keep` holds that device and is dropped only after the
    /// session, which the runtime may still be using it for.
    pub fn start(&mut self, vk: VulkanHandles, keep: Box<dyn Any>) -> Result<(), String> {
        let overlay = xr::sys::SessionCreateInfoOverlayEXTX {
            ty: xr::sys::SessionCreateInfoOverlayEXTX::TYPE,
            next: std::ptr::null(),
            create_flags: Default::default(),
            session_layers_placement: LAYERS_PLACEMENT,
        };
        let binding = xr::sys::GraphicsBindingVulkanKHR {
            ty: xr::sys::GraphicsBindingVulkanKHR::TYPE,
            next: (&raw const overlay).cast(),
            instance: vk.instance as *const c_void,
            physical_device: vk.physical_device as *const c_void,
            device: vk.device as *const c_void,
            queue_family_index: vk.queue_family,
            queue_index: 0,
        };
        let info = xr::sys::SessionCreateInfo {
            ty: xr::sys::SessionCreateInfo::TYPE,
            next: (&raw const binding).cast(),
            create_flags: Default::default(),
            system_id: self.system,
        };
        let mut handle = xr::sys::Session::NULL;
        // SAFETY: a well-formed create-info chain (all of it alive for the call) with live Vulkan handles from the
        // device the runtime picked; the instance's own function table.
        let r = unsafe { (self.instance.fp().create_session)(self.instance.as_raw(), &info, &mut handle) };
        if r.into_raw() < 0 {
            return Err(xr_err("xrCreateSession (overlay)")(r));
        }
        // SAFETY: a new session of this instance, made for Vulkan, not inside a frame.
        let (session, waiter, stream) =
            unsafe { xr::Session::<xr::Vulkan>::from_raw(self.instance.clone(), handle, Box::new(())) };
        let formats = session.enumerate_swapchain_formats().map_err(xr_err("xrEnumerateSwapchainFormats"))?;
        let format =
            FORMATS.into_iter().find(|f| formats.contains(f)).ok_or("the OpenXR runtime offers no BGRA swapchains")?;
        let space = |ty| session.create_reference_space(ty, xr::Posef::IDENTITY);
        let stage = space(xr::ReferenceSpaceType::STAGE).map_err(xr_err("xrCreateReferenceSpace (stage)"))?;
        let view = space(xr::ReferenceSpaceType::VIEW).map_err(xr_err("xrCreateReferenceSpace (view)"))?;
        let a = &self.actions;
        session.attach_action_sets(&[&a.set]).map_err(xr_err("xrAttachSessionActionSets"))?;
        let pointer = |i: usize| a.pointer.create_space(&session, a.hands[i], xr::Posef::IDENTITY);
        let pointers = [pointer(0), pointer(1)];
        let [Ok(left), Ok(right)] = pointers else { return Err("xrCreateActionSpace failed".into()) };
        let mut layers = HashMap::new();
        // Clear and tiny, a metre in front of the eyes.
        let blank = Layer {
            pose: Pose::new(Vec3::FORWARD, Quat::IDENTITY),
            head: true,
            size: (0.01, 0.01),
            alpha: 1.0,
            ..Layer::default()
        };
        layers.insert(BLANK_KEY, blank);
        self.live = Some(Live { session, waiter, stream, stage, view, pointers: [left, right], format, layers });
        self.gpu = Some(keep);
        Ok(())
    }

    /// The swapchain image (raw `VkImage`) to copy the overlay `key`'s picture into, `w` x `h` pixels, in
    /// COLOR_ATTACHMENT_OPTIMAL. Must be followed by `release`, whatever happens in between.
    pub fn acquire(&mut self, key: u64, w: u32, h: u32) -> Result<u64, String> {
        let live = self.live.as_mut().ok_or("no OpenXR session")?;
        let layer = live.layers.entry(key).or_default();
        if layer.chain.as_ref().is_none_or(|c| c.size != (w, h)) {
            layer.chain = None;
            let swapchain = live
                .session
                .create_swapchain(&xr::SwapchainCreateInfo {
                    create_flags: xr::SwapchainCreateFlags::EMPTY,
                    usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT | xr::SwapchainUsageFlags::TRANSFER_DST,
                    format: live.format,
                    sample_count: 1,
                    width: w,
                    height: h,
                    face_count: 1,
                    array_size: 1,
                    mip_count: 1,
                })
                .map_err(xr_err("xrCreateSwapchain"))?;
            let images = swapchain.enumerate_images().map_err(xr_err("xrEnumerateSwapchainImages"))?;
            layer.chain = Some(Chain { swapchain, images, size: (w, h), ready: false });
        }
        let Some(chain) = layer.chain.as_mut() else { return Err("no swapchain".into()) };
        let i = chain.swapchain.acquire_image().map_err(xr_err("xrAcquireSwapchainImage"))?;
        if let Err(e) = chain.swapchain.wait_image(xr::Duration::INFINITE) {
            // Acquired but never waited for: the swapchain cannot go on, so a new one comes next time.
            layer.chain = None;
            return Err(xr_err("xrWaitSwapchainImage")(e));
        }
        chain.images.get(i as usize).copied().ok_or_else(|| "swapchain image out of range".into())
    }

    /// Hands the image from `acquire` back to the runtime, to be shown from the next frame on.
    pub fn release(&mut self, key: u64) -> Result<(), String> {
        let layer = self.live.as_mut().and_then(|l| l.layers.get_mut(&key));
        let Some(chain) = layer.and_then(|l| l.chain.as_mut()) else { return Ok(()) };
        chain.swapchain.release_image().map_err(xr_err("xrReleaseSwapchainImage"))?;
        chain.ready = true;
        Ok(())
    }

    /// Ends the frame begun in `wait_frame`, then waits for and begins the next one. Without a running session,
    /// sleeps `timeout_ms` instead.
    pub fn wait_frame(&mut self, timeout_ms: u32) {
        self.end_frame();
        let pause = Duration::from_millis(u64::from(timeout_ms));
        let Some(live) = self.live.as_mut().filter(|_| self.running) else {
            std::thread::sleep(pause);
            return;
        };
        match live.waiter.wait() {
            Ok(state) => match live.stream.begin() {
                Ok(()) => {
                    self.time = Some(state.predicted_display_time);
                    self.begun = true;
                }
                Err(e) => tracing::warn!(%e, "xrBeginFrame failed"),
            },
            Err(e) => {
                tracing::warn!(%e, "xrWaitFrame failed");
                std::thread::sleep(pause);
            }
        }
    }

    /// Submits the layers of the frame begun, if one is: every shown quad with a picture, back to front within
    /// each group, since layers do not test depth against each other.
    fn end_frame(&mut self) {
        if !std::mem::take(&mut self.begun) {
            return;
        }
        let (Some(live), Some(time)) = (self.live.as_mut(), self.time) else { return };
        let eye = self.head.map_or(Vec3::ZERO, |h| h.pos);
        let distance = |l: &Layer| if l.head { l.pose.pos.length() } else { (l.pose.pos - eye).length() };
        let ready = |l: &Layer| l.chain.as_ref().is_some_and(|c| c.ready);
        let mut list: Vec<&Layer> = live.layers.values().filter(|l| l.shown && l.alpha > 0.001 && ready(l)).collect();
        list.sort_by(|a, b| a.order.cmp(&b.order).then(distance(b).total_cmp(&distance(a))));
        if list.is_empty() {
            list.extend(live.layers.get(&BLANK_KEY).filter(|l| ready(l)));
        }
        let fades: Vec<xr::sys::CompositionLayerColorScaleBiasKHR> = list
            .iter()
            .map(|l| xr::sys::CompositionLayerColorScaleBiasKHR {
                ty: xr::sys::CompositionLayerColorScaleBiasKHR::TYPE,
                next: std::ptr::null(),
                color_scale: xr::Color4f { r: 1.0, g: 1.0, b: 1.0, a: l.alpha.min(1.0) },
                color_bias: xr::Color4f { r: 0.0, g: 0.0, b: 0.0, a: 0.0 },
            })
            .collect();
        let mut quads = Vec::with_capacity(list.len());
        for (l, fade) in list.iter().zip(&fades) {
            let Some(c) = &l.chain else { continue };
            let rect = xr::Rect2Di {
                offset: xr::Offset2Di { x: 0, y: 0 },
                extent: xr::Extent2Di { width: c.size.0 as i32, height: c.size.1 as i32 },
            };
            let quad = xr::CompositionLayerQuad::new()
                .layer_flags(
                    xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA
                        | xr::CompositionLayerFlags::UNPREMULTIPLIED_ALPHA,
                )
                .space(if l.head { &live.view } else { &live.stage })
                .eye_visibility(xr::EyeVisibility::BOTH)
                .sub_image(xr::SwapchainSubImage::new().swapchain(&c.swapchain).image_rect(rect))
                .pose(to_xr(l.pose))
                .size(xr::Extent2Df { width: l.size.0, height: l.size.1 });
            if !self.color_scale || l.alpha >= 1.0 {
                quads.push(quad);
                continue;
            }
            // Faded panels (hide-all, the watch's look-to-show) dim through the layer's alpha.
            let mut raw = quad.into_raw();
            raw.next = (fade as *const xr::sys::CompositionLayerColorScaleBiasKHR).cast();
            // SAFETY: the same quad with a well-formed extension struct chained on; `fades` outlives the call below.
            quads.push(unsafe { xr::CompositionLayerQuad::from_raw(raw) });
        }
        let layers: Vec<&xr::CompositionLayerBase<'_, xr::Vulkan>> = quads.iter().map(|q| &**q).collect();
        if let Err(e) = live.stream.end(time, self.blend, &layers) {
            tracing::warn!(%e, "xrEndFrame failed");
        }
    }

    /// Follows the session through its states; false once the runtime wants us gone.
    fn session_state(&mut self, state: xr::SessionState) -> bool {
        tracing::info!(?state, "OpenXR session");
        match state {
            xr::SessionState::READY => {
                let Some(live) = &self.live else { return true };
                match live.session.begin(xr::ViewConfigurationType::PRIMARY_STEREO) {
                    Ok(_) => self.running = true,
                    Err(e) => tracing::warn!(%e, "xrBeginSession failed"),
                }
            }
            xr::SessionState::STOPPING => {
                self.end_frame();
                self.running = false;
                self.time = None;
                if let Some(live) = &self.live
                    && let Err(e) = live.session.end()
                {
                    tracing::warn!(%e, "xrEndSession failed");
                }
            }
            xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => return false,
            _ => {}
        }
        true
    }

    /// Someone is wearing the headset, as far as OpenXR tells: it is being tracked.
    pub fn worn(&self) -> bool {
        self.head.is_some()
    }

    pub fn stats(&self) -> VrStats {
        VrStats::default()
    }
}

impl VrBackend for OpenXr {
    fn poll(&mut self, now_ms: u64, input: &mut FrameInput) -> bool {
        input.now_ms = now_ms;
        let mut buf = xr::EventDataBuffer::new();
        loop {
            let event = match self.instance.poll_event(&mut buf) {
                Ok(Some(e)) => e,
                Ok(None) => break,
                Err(e) => {
                    tracing::warn!(%e, "OpenXR runtime gone");
                    return false;
                }
            };
            match event {
                xr::Event::SessionStateChanged(s) => {
                    if !self.session_state(s.state()) {
                        return false;
                    }
                }
                xr::Event::InstanceLossPending(_) => return false,
                _ => {}
            }
        }
        let (Some(live), Some(time), true) = (&self.live, self.time, self.running) else { return true };
        let a = &self.actions;
        if let Err(e) = live.session.sync_actions(&[xr::ActiveActionSet::new(&a.set)]) {
            tracing::debug!(%e, "xrSyncActions failed");
        }
        for hand in Hand::BOTH {
            let i = hand.index();
            let path = a.hands[i];
            let on = |action: &xr::Action<bool>| {
                action.state(&live.session, path).is_ok_and(|s| s.is_active && s.current_state)
            };
            let h = &mut input.hands[i];
            let at = live.pointers[i].locate(&live.stage, time).ok().filter(located);
            h.tracked = at.is_some();
            if let Some(l) = at {
                h.pose = to_pose(l.pose);
            }
            h.trigger = on(&a.click);
            h.grip = on(&a.grab);
            h.menu = on(&a.menu);
            h.right_click = on(&a.right_click);
            h.stick = match a.stick.state(&live.session, path) {
                Ok(s) if s.is_active => [s.current_state.x, s.current_state.y],
                _ => [0.0; 2],
            };
        }
        self.head = live.view.locate(&live.stage, time).ok().filter(located).map(|l| to_pose(l.pose));
        if let Some(head) = self.head {
            input.head = head;
        }
        true
    }

    fn head_pose(&self) -> Option<Pose> {
        self.head
    }

    fn haptic(&mut self, hand: Hand) {
        let Some(live) = &self.live else { return };
        let pulse =
            xr::HapticVibration::new().amplitude(0.35).frequency(160.0).duration(xr::Duration::from_nanos(12_000_000));
        if let Err(e) = self.actions.haptic.apply_feedback(&live.session, self.actions.hands[hand.index()], &pulse) {
            tracing::debug!(%e, "xrApplyHapticFeedback failed");
        }
    }

    fn place_panel(&mut self, panel: &Panel, world: Pose) {
        let Some(live) = self.live.as_mut() else { return };
        let l = live.layers.entry(u64::from(panel.id)).or_default();
        // Head-locked panels ride on the headset in the compositor (no lag); everything else is placed in the room.
        l.head = panel.anchor == Anchor::Head;
        l.pose = if l.head { panel.pose } else { world };
        l.size = (panel.width_m, panel.height_m());
        l.alpha = panel.opacity * (1.0 - panel.dim);
        l.shown = panel.shown();
        l.order = u8::from(matches!(panel.source, Source::Keyboard | Source::Toolbar | Source::Banner));
    }

    fn remove_panel(&mut self, id: u32) {
        if let Some(live) = self.live.as_mut() {
            live.layers.remove(&u64::from(id));
        }
    }

    fn place_cursor(&mut self, hand: Hand, at: Option<Pose>) {
        let Some(live) = self.live.as_mut() else { return };
        let l = live.layers.entry(cursor_key(hand)).or_default();
        l.shown = at.is_some();
        if let Some(p) = at {
            l.pose = p;
        }
        l.size = (CURSOR_WIDTH_M, CURSOR_WIDTH_M);
        l.alpha = 1.0;
        l.order = 2;
    }
}
