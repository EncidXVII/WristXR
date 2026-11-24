// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! Linux: SteamVR through OpenVR when it is there, else an OpenXR runtime that can show overlays (Monado, WiVRn).
//! The calls the app makes go to whichever is in use; SteamVR-only ones (app registration, auto-launch, bindings
//! screen, input claims) do nothing under OpenXR.

use crate::{OpenVr, OpenXr, VrStats};
use std::path::Path;
use wxr_core::backend::VrBackend;
use wxr_core::engine::{Claims, FrameInput};
use wxr_core::math::Pose;
use wxr_core::panel::{Hand, Panel};

pub enum Vr {
    OpenVr(OpenVr),
    OpenXr(OpenXr),
}

impl Vr {
    /// SteamVR first, as everywhere; OpenXR when SteamVR cannot be reached, or straight away with
    /// `WRISTXR_OPENXR=1` (for a SteamVR install that is there but not in use).
    pub fn connect() -> Result<Self, String> {
        let steamvr = if std::env::var_os("WRISTXR_OPENXR").is_some_and(|v| v == "1") {
            Err("SteamVR skipped (WRISTXR_OPENXR=1)".to_owned())
        } else {
            OpenVr::connect()
        };
        match steamvr {
            Ok(vr) => Ok(Self::OpenVr(vr)),
            Err(e) => match OpenXr::connect() {
                Ok(xr) => {
                    tracing::info!(steamvr = %e, "SteamVR unavailable; using OpenXR");
                    Ok(Self::OpenXr(xr))
                }
                Err(x) => Err(format!("{e}\nOpenXR (Monado, WiVRn): {x}")),
            },
        }
    }

    pub fn register_app(&self, manifest: &Path, app_key: &str) -> Result<(), String> {
        match self {
            Self::OpenVr(vr) => vr.register_app(manifest, app_key),
            Self::OpenXr(_) => Ok(()),
        }
    }

    /// OpenXR's actions are made in `connect`.
    pub fn load_actions(&mut self, manifest: &Path) -> Result<(), String> {
        match self {
            Self::OpenVr(vr) => vr.load_actions(manifest),
            Self::OpenXr(_) => Ok(()),
        }
    }

    /// An OpenXR overlay cannot take controls away from the game.
    pub fn set_claims(&mut self, claims: [Claims; 2]) {
        if let Self::OpenVr(vr) = self {
            vr.set_claims(claims);
        }
    }

    pub fn auto_launch(&self, app_key: &str) -> bool {
        match self {
            Self::OpenVr(vr) => vr.auto_launch(app_key),
            Self::OpenXr(_) => false,
        }
    }

    pub fn set_auto_launch(&self, app_key: &str, on: bool) -> Result<(), String> {
        match self {
            Self::OpenVr(vr) => vr.set_auto_launch(app_key, on),
            Self::OpenXr(_) => Ok(()),
        }
    }

    pub fn open_bindings(&self, app_key: &str) -> Result<(), String> {
        match self {
            Self::OpenVr(vr) => vr.open_bindings(app_key),
            Self::OpenXr(_) => Err("the OpenXR runtime has no binding screen".into()),
        }
    }

    pub fn pixels_per_radian(&self) -> Option<f32> {
        match self {
            Self::OpenVr(vr) => vr.pixels_per_radian(),
            Self::OpenXr(_) => None,
        }
    }

    pub fn worn(&self) -> bool {
        match self {
            Self::OpenVr(vr) => vr.worn(),
            Self::OpenXr(xr) => xr.worn(),
        }
    }

    pub fn stats(&self) -> VrStats {
        match self {
            Self::OpenVr(vr) => vr.stats(),
            Self::OpenXr(xr) => xr.stats(),
        }
    }

    /// OpenXR quads always show their whole swapchain image.
    pub fn set_texture_bounds(&mut self, key: u64, bounds: [f32; 4]) -> Result<(), String> {
        match self {
            Self::OpenVr(vr) => vr.set_texture_bounds(key, bounds),
            Self::OpenXr(_) => Ok(()),
        }
    }

    pub fn wait_frame(&mut self, timeout_ms: u32) {
        match self {
            Self::OpenVr(vr) => vr.wait_frame(timeout_ms),
            Self::OpenXr(xr) => xr.wait_frame(timeout_ms),
        }
    }

    /// Vulkan extensions the runtime needs on the instance (`None`) or on a physical device (its raw handle).
    pub fn vulkan_extensions(&self, physical_device: Option<u64>) -> Vec<String> {
        match self {
            Self::OpenVr(vr) => vr.vulkan_extensions(physical_device),
            Self::OpenXr(xr) => xr.vulkan_extensions(physical_device.is_some()),
        }
    }

    /// The Vulkan physical device (raw handle) the headset is on, for `instance` (a raw `VkInstance` handle).
    pub fn vulkan_output_device(&self, instance: u64) -> u64 {
        match self {
            Self::OpenVr(vr) => vr.vulkan_output_device(instance),
            Self::OpenXr(xr) => xr.vulkan_output_device(instance),
        }
    }
}

impl VrBackend for Vr {
    fn poll(&mut self, now_ms: u64, input: &mut FrameInput) -> bool {
        match self {
            Self::OpenVr(vr) => vr.poll(now_ms, input),
            Self::OpenXr(xr) => xr.poll(now_ms, input),
        }
    }

    fn head_pose(&self) -> Option<Pose> {
        match self {
            Self::OpenVr(vr) => vr.head_pose(),
            Self::OpenXr(xr) => xr.head_pose(),
        }
    }

    fn haptic(&mut self, hand: Hand) {
        match self {
            Self::OpenVr(vr) => vr.haptic(hand),
            Self::OpenXr(xr) => xr.haptic(hand),
        }
    }

    fn place_panel(&mut self, panel: &Panel, world: Pose) {
        match self {
            Self::OpenVr(vr) => vr.place_panel(panel, world),
            Self::OpenXr(xr) => xr.place_panel(panel, world),
        }
    }

    fn remove_panel(&mut self, id: u32) {
        match self {
            Self::OpenVr(vr) => vr.remove_panel(id),
            Self::OpenXr(xr) => xr.remove_panel(id),
        }
    }

    fn place_cursor(&mut self, hand: Hand, at: Option<Pose>) {
        match self {
            Self::OpenVr(vr) => vr.place_cursor(hand, at),
            Self::OpenXr(xr) => xr.place_cursor(hand, at),
        }
    }
}
