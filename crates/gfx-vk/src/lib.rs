// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! Linux: panel textures for SteamVR through Vulkan, the counterpart of `wxr-gfx-d3d11`.
//!
//! Every texture is a device-local image with its own persistently mapped staging buffer. `update` only copies
//! pixels into the staging memory; `flush` records the copies for every texture changed this frame into one
//! command buffer, submits it once and waits, so a frame costs one submit however many panels changed. GPU
//! timestamps around that submit give the cost meter its number. Textures free themselves when the last handle
//! goes, a few frames later, so SteamVR is never still reading an image that has been destroyed.
#![cfg(target_os = "linux")]
#![allow(unsafe_code)]

use ash::vk::{self, Handle};
use std::cell::{Cell, RefCell};
use std::ffi::{CString, c_void};
use std::rc::Rc;
use wxr_core::raster::Image;
use wxr_vr::{OpenVr, VulkanTexture};

/// Captures and UI pixels are BGRA bytes, shown as they are (`ColorSpace_Auto`), as on Windows.
const FORMAT: vk::Format = vk::Format::B8G8R8A8_UNORM;
/// A dropped texture is destroyed this many flushes later, once SteamVR has long moved on from it.
const GRACE_FRAMES: u64 = 4;

pub type Result<T> = std::result::Result<T, String>;

fn vk_err(call: &'static str) -> impl Fn(vk::Result) -> String {
    move |r| format!("{call}: {r:?}")
}

struct Device {
    _entry: ash::Entry,
    instance: ash::Instance,
    physical: vk::PhysicalDevice,
    device: ash::Device,
    queue: vk::Queue,
    family: u32,
    pool: vk::CommandPool,
    cmd: vk::CommandBuffer,
    fence: vk::Fence,
    queries: vk::QueryPool,
    /// Nanoseconds per timestamp tick; 0 when the queue has no timestamps.
    tick_ns: f32,
    memory: vk::PhysicalDeviceMemoryProperties,
    /// Textures with new pixels, copied at the next flush.
    pending: RefCell<Vec<Rc<TexInner>>>,
    /// Dropped textures and the flush after which they may go.
    graveyard: RefCell<Vec<(u64, Parts)>>,
    frame: Cell<u64>,
    last_ms: Cell<f32>,
}

/// The Vulkan objects of one texture.
#[derive(Clone, Copy)]
struct Parts {
    image: vk::Image,
    image_mem: vk::DeviceMemory,
    staging: vk::Buffer,
    staging_mem: vk::DeviceMemory,
}

struct TexInner {
    dev: Rc<Device>,
    parts: Parts,
    mapped: *mut u8,
    w: u32,
    h: u32,
    /// Its first copy moves it out of the undefined layout; later ones start from TRANSFER_SRC.
    initialised: Cell<bool>,
}

impl Drop for TexInner {
    fn drop(&mut self) {
        self.dev.graveyard.borrow_mut().push((self.dev.frame.get() + GRACE_FRAMES, self.parts));
    }
}

/// A panel texture. Cheap to clone; the image goes when the last clone does.
#[derive(Clone)]
pub struct Tex(Rc<TexInner>);

impl Tex {
    pub fn size(&self) -> (u32, u32) {
        (self.0.w, self.0.h)
    }
}

pub struct Gpu {
    dev: Rc<Device>,
}

impl Gpu {
    /// On the GPU the headset is on, with the extensions SteamVR asks for.
    pub fn new(vr: &OpenVr) -> Result<Self> {
        // SAFETY: loads the system Vulkan loader; the calls below follow the Vulkan valid-usage rules for these
        // simple objects, and every handle made here is destroyed in `Device::drop` (or right here on failure).
        unsafe {
            let entry = ash::Entry::load().map_err(|e| format!("Vulkan not available: {e}"))?;
            let names: Vec<CString> =
                vr.vulkan_extensions(None).into_iter().filter_map(|e| CString::new(e).ok()).collect();
            let ext: Vec<_> = names.iter().map(|e| e.as_ptr()).collect();
            let app = vk::ApplicationInfo::default().api_version(vk::make_api_version(0, 1, 1, 0));
            let instance = entry
                .create_instance(
                    &vk::InstanceCreateInfo::default().application_info(&app).enabled_extension_names(&ext),
                    None,
                )
                .map_err(vk_err("vkCreateInstance"))?;
            let wanted = vr.vulkan_output_device(instance.handle().as_raw());
            let devices = match instance.enumerate_physical_devices() {
                Ok(d) => d,
                Err(e) => {
                    instance.destroy_instance(None);
                    return Err(vk_err("vkEnumeratePhysicalDevices")(e));
                }
            };
            let Some(physical) = devices.iter().copied().find(|d| d.as_raw() == wanted).or(devices.first().copied())
            else {
                instance.destroy_instance(None);
                return Err("no Vulkan GPU".into());
            };
            let families = instance.get_physical_device_queue_family_properties(physical);
            let Some(family) = families.iter().position(|q| q.queue_flags.contains(vk::QueueFlags::GRAPHICS)) else {
                instance.destroy_instance(None);
                return Err("no graphics queue".into());
            };
            let timestamps = families[family].timestamp_valid_bits > 0;
            let family = family as u32;
            let names: Vec<CString> = vr
                .vulkan_extensions(Some(physical.as_raw()))
                .into_iter()
                .filter_map(|e| CString::new(e).ok())
                .collect();
            let dev_ext: Vec<_> = names.iter().map(|e| e.as_ptr()).collect();
            let queue_info = [vk::DeviceQueueCreateInfo::default().queue_family_index(family).queue_priorities(&[1.0])];
            let device = match instance.create_device(
                physical,
                &vk::DeviceCreateInfo::default().queue_create_infos(&queue_info).enabled_extension_names(&dev_ext),
                None,
            ) {
                Ok(d) => d,
                Err(e) => {
                    instance.destroy_instance(None);
                    return Err(vk_err("vkCreateDevice")(e));
                }
            };
            let props = instance.get_physical_device_properties(physical);
            let memory = instance.get_physical_device_memory_properties(physical);
            let mut dev = Device {
                _entry: entry,
                queue: device.get_device_queue(family, 0),
                instance,
                physical,
                device,
                family,
                pool: vk::CommandPool::null(),
                cmd: vk::CommandBuffer::null(),
                fence: vk::Fence::null(),
                queries: vk::QueryPool::null(),
                tick_ns: if timestamps { props.limits.timestamp_period } else { 0.0 },
                memory,
                pending: RefCell::new(Vec::new()),
                graveyard: RefCell::new(Vec::new()),
                frame: Cell::new(0),
                last_ms: Cell::new(0.0),
            };
            // From here `Device::drop` frees whatever exists.
            let d = &dev.device;
            dev.pool = d
                .create_command_pool(
                    &vk::CommandPoolCreateInfo::default()
                        .queue_family_index(family)
                        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                    None,
                )
                .map_err(vk_err("vkCreateCommandPool"))?;
            dev.cmd = d
                .allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::default()
                        .command_pool(dev.pool)
                        .level(vk::CommandBufferLevel::PRIMARY)
                        .command_buffer_count(1),
                )
                .map_err(vk_err("vkAllocateCommandBuffers"))?[0];
            dev.fence = d.create_fence(&vk::FenceCreateInfo::default(), None).map_err(vk_err("vkCreateFence"))?;
            if dev.tick_ns > 0.0 {
                dev.queries = d
                    .create_query_pool(
                        &vk::QueryPoolCreateInfo::default().query_type(vk::QueryType::TIMESTAMP).query_count(2),
                        None,
                    )
                    .map_err(vk_err("vkCreateQueryPool"))?;
            }
            Ok(Self { dev: Rc::new(dev) })
        }
    }

    fn memory_type(&self, bits: u32, flags: vk::MemoryPropertyFlags) -> Result<u32> {
        let m = &self.dev.memory;
        (0..m.memory_type_count)
            .find(|&i| bits & (1 << i) != 0 && m.memory_types[i as usize].property_flags.contains(flags))
            .ok_or_else(|| "no suitable GPU memory".to_owned())
    }

    /// A texture of this size, black (or filled from `bgra`, `w * h * 4` bytes) once the next flush runs.
    pub fn texture(&self, w: u32, h: u32, bgra: Option<&[u8]>) -> Result<Tex> {
        let (w, h) = (w.max(1), h.max(1));
        let d = &self.dev.device;
        let mut parts = Parts {
            image: vk::Image::null(),
            image_mem: vk::DeviceMemory::null(),
            staging: vk::Buffer::null(),
            staging_mem: vk::DeviceMemory::null(),
        };
        let size = u64::from(w) * u64::from(h) * 4;
        // SAFETY: image, buffer and memory set up on our own device; on failure the partial parts go to the
        // graveyard, where null handles are valid no-ops to destroy.
        let built = unsafe {
            (|| -> Result<*mut u8> {
                parts.image = d
                    .create_image(
                        &vk::ImageCreateInfo::default()
                            .image_type(vk::ImageType::TYPE_2D)
                            .format(FORMAT)
                            .extent(vk::Extent3D { width: w, height: h, depth: 1 })
                            .mip_levels(1)
                            .array_layers(1)
                            .samples(vk::SampleCountFlags::TYPE_1)
                            .tiling(vk::ImageTiling::OPTIMAL)
                            .usage(
                                vk::ImageUsageFlags::TRANSFER_DST
                                    | vk::ImageUsageFlags::TRANSFER_SRC
                                    | vk::ImageUsageFlags::SAMPLED,
                            ),
                        None,
                    )
                    .map_err(vk_err("vkCreateImage"))?;
                let req = d.get_image_memory_requirements(parts.image);
                let ty = self.memory_type(req.memory_type_bits, vk::MemoryPropertyFlags::DEVICE_LOCAL)?;
                parts.image_mem = d
                    .allocate_memory(
                        &vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(ty),
                        None,
                    )
                    .map_err(vk_err("vkAllocateMemory"))?;
                d.bind_image_memory(parts.image, parts.image_mem, 0).map_err(vk_err("vkBindImageMemory"))?;
                parts.staging = d
                    .create_buffer(
                        &vk::BufferCreateInfo::default().size(size).usage(vk::BufferUsageFlags::TRANSFER_SRC),
                        None,
                    )
                    .map_err(vk_err("vkCreateBuffer"))?;
                let req = d.get_buffer_memory_requirements(parts.staging);
                let host = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
                let ty = self.memory_type(req.memory_type_bits, host)?;
                parts.staging_mem = d
                    .allocate_memory(
                        &vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(ty),
                        None,
                    )
                    .map_err(vk_err("vkAllocateMemory"))?;
                d.bind_buffer_memory(parts.staging, parts.staging_mem, 0).map_err(vk_err("vkBindBufferMemory"))?;
                Ok(d.map_memory(parts.staging_mem, 0, size, vk::MemoryMapFlags::empty())
                    .map_err(vk_err("vkMapMemory"))?
                    .cast())
            })()
        };
        let mapped = match built {
            Ok(p) => p,
            Err(e) => {
                self.dev.graveyard.borrow_mut().push((self.dev.frame.get(), parts));
                return Err(e);
            }
        };
        let tex = Tex(Rc::new(TexInner { dev: self.dev.clone(), parts, mapped, w, h, initialised: Cell::new(false) }));
        match bgra {
            Some(px) => self.update(&tex, px, w),
            None => {
                // SAFETY: `mapped` covers exactly `size` bytes of host-coherent memory we own.
                unsafe { std::ptr::write_bytes(mapped, 0, size as usize) };
                self.dev.pending.borrow_mut().push(tex.0.clone());
            }
        }
        Ok(tex)
    }

    pub fn image(&self, img: &Image) -> Result<Tex> {
        self.texture(img.w, img.h, Some(&img.bgra))
    }

    /// New pixels for `tex`: `bgra` holds rows of `width` pixels (at least the texture's own width).
    pub fn update(&self, tex: &Tex, bgra: &[u8], width: u32) {
        let t = &tex.0;
        let (src_row, dst_row) = (width as usize * 4, t.w as usize * 4);
        let rows = (bgra.len() / src_row.max(1)).min(t.h as usize);
        if src_row < dst_row {
            return;
        }
        for y in 0..rows {
            // SAFETY: row `y < h` of the mapped staging memory (w * h * 4 bytes); the source row is in bounds.
            unsafe {
                std::ptr::copy_nonoverlapping(bgra.as_ptr().add(y * src_row), t.mapped.add(y * dst_row), dst_row)
            };
        }
        let mut pending = self.dev.pending.borrow_mut();
        if !pending.iter().any(|p| Rc::ptr_eq(p, t)) {
            pending.push(t.clone());
        }
    }

    /// Copies this frame's new pixels to the GPU in one submit and waits for it; frees textures past their grace.
    pub fn flush(&self) {
        let dev = &self.dev;
        let frame = dev.frame.get() + 1;
        dev.frame.set(frame);
        let pending: Vec<_> = dev.pending.borrow_mut().drain(..).collect();
        if !pending.is_empty()
            && let Err(e) = self.copy(&pending)
        {
            tracing::warn!(%e, "texture upload failed");
        }
        let mut grave = dev.graveyard.borrow_mut();
        if grave.iter().any(|(at, _)| *at <= frame) {
            let d = &dev.device;
            grave.retain(|(at, p)| {
                if *at > frame {
                    return true;
                }
                // SAFETY: the last upload touching these finished (we wait on every submit) and SteamVR has had
                // GRACE_FRAMES frames with a newer texture; null handles are no-ops.
                unsafe {
                    d.destroy_buffer(p.staging, None);
                    d.free_memory(p.staging_mem, None);
                    d.destroy_image(p.image, None);
                    d.free_memory(p.image_mem, None);
                }
                false
            });
        }
    }

    fn copy(&self, textures: &[Rc<TexInner>]) -> Result<()> {
        let dev = &self.dev;
        let d = &dev.device;
        let range =
            vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
        // SAFETY: one command buffer, re-recorded each flush after the previous submit was waited for; every image
        // and buffer is alive (the `Rc`s in `textures`) until this function returns.
        unsafe {
            d.reset_command_buffer(dev.cmd, vk::CommandBufferResetFlags::empty())
                .map_err(vk_err("vkResetCommandBuffer"))?;
            d.begin_command_buffer(
                dev.cmd,
                &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )
            .map_err(vk_err("vkBeginCommandBuffer"))?;
            if dev.tick_ns > 0.0 {
                d.cmd_reset_query_pool(dev.cmd, dev.queries, 0, 2);
                d.cmd_write_timestamp(dev.cmd, vk::PipelineStageFlags::TOP_OF_PIPE, dev.queries, 0);
            }
            let barrier = |image, from, to, src, dst| {
                vk::ImageMemoryBarrier::default()
                    .old_layout(from)
                    .new_layout(to)
                    .src_access_mask(src)
                    .dst_access_mask(dst)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(image)
                    .subresource_range(range)
            };
            let to_dst: Vec<_> = textures
                .iter()
                .map(|t| {
                    let from = if t.initialised.get() {
                        vk::ImageLayout::TRANSFER_SRC_OPTIMAL
                    } else {
                        vk::ImageLayout::UNDEFINED
                    };
                    barrier(
                        t.parts.image,
                        from,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        vk::AccessFlags::empty(),
                        vk::AccessFlags::TRANSFER_WRITE,
                    )
                })
                .collect();
            d.cmd_pipeline_barrier(
                dev.cmd,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &to_dst,
            );
            for t in textures {
                let region = vk::BufferImageCopy::default()
                    .image_subresource(
                        vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).layer_count(1),
                    )
                    .image_extent(vk::Extent3D { width: t.w, height: t.h, depth: 1 });
                d.cmd_copy_buffer_to_image(
                    dev.cmd,
                    t.parts.staging,
                    t.parts.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[region],
                );
            }
            // SteamVR reads overlay textures in TRANSFER_SRC_OPTIMAL.
            let to_src: Vec<_> = textures
                .iter()
                .map(|t| {
                    barrier(
                        t.parts.image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                        vk::AccessFlags::TRANSFER_WRITE,
                        vk::AccessFlags::TRANSFER_READ,
                    )
                })
                .collect();
            d.cmd_pipeline_barrier(
                dev.cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &to_src,
            );
            if dev.tick_ns > 0.0 {
                d.cmd_write_timestamp(dev.cmd, vk::PipelineStageFlags::BOTTOM_OF_PIPE, dev.queries, 1);
            }
            d.end_command_buffer(dev.cmd).map_err(vk_err("vkEndCommandBuffer"))?;
            let cmds = [dev.cmd];
            d.queue_submit(dev.queue, &[vk::SubmitInfo::default().command_buffers(&cmds)], dev.fence)
                .map_err(vk_err("vkQueueSubmit"))?;
            d.wait_for_fences(&[dev.fence], true, u64::MAX).map_err(vk_err("vkWaitForFences"))?;
            d.reset_fences(&[dev.fence]).map_err(vk_err("vkResetFences"))?;
            if dev.tick_ns > 0.0 {
                let mut stamps = [0u64; 2];
                if d.get_query_pool_results(dev.queries, 0, &mut stamps, vk::QueryResultFlags::TYPE_64).is_ok() {
                    dev.last_ms.set(stamps[1].saturating_sub(stamps[0]) as f32 * dev.tick_ns / 1e6);
                }
            }
        }
        for t in textures {
            t.initialised.set(true);
        }
        Ok(())
    }

    /// Hands `tex` to the overlay `key`. Call after `flush`.
    pub fn submit(&self, vr: &mut OpenVr, key: u64, tex: &Tex) -> std::result::Result<(), String> {
        let dev = &self.dev;
        let mut data = VulkanTexture {
            image: tex.0.parts.image.as_raw(),
            device: dev.device.handle().as_raw() as *mut c_void,
            physical_device: dev.physical.as_raw() as *mut c_void,
            instance: dev.instance.handle().as_raw() as *mut c_void,
            queue: dev.queue.as_raw() as *mut c_void,
            queue_family_index: dev.family,
            width: tex.0.w,
            height: tex.0.h,
            format: FORMAT.as_raw() as u32,
            sample_count: 1,
        };
        vr.set_texture_vulkan(key, &mut data)
    }

    /// The GPU's name, for the self-test report.
    pub fn name(&self) -> String {
        // SAFETY: `physical` belongs to `instance`.
        let props = unsafe { self.dev.instance.get_physical_device_properties(self.dev.physical) };
        props.device_name_as_c_str().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: waits for the GPU, then destroys children before parents; null handles are no-ops. Textures hold
        // an `Rc<Device>`, so none is alive here; their parts are all in the graveyard.
        unsafe {
            let d = &self.device;
            let _ = d.device_wait_idle();
            for (_, p) in self.graveyard.get_mut().drain(..) {
                d.destroy_buffer(p.staging, None);
                d.free_memory(p.staging_mem, None);
                d.destroy_image(p.image, None);
                d.free_memory(p.image_mem, None);
            }
            d.destroy_query_pool(self.queries, None);
            d.destroy_fence(self.fence, None);
            d.destroy_command_pool(self.pool, None);
            d.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

/// GPU time of the last flush, for the cost meter (same shape as the D3D11 timer).
pub struct GpuTimer {
    dev: Rc<Device>,
}

impl GpuTimer {
    pub fn new(gpu: &Gpu) -> Result<Self> {
        Ok(Self { dev: gpu.dev.clone() })
    }
    pub fn begin(&self, _gpu: &Gpu) {}
    pub fn end(&mut self, _gpu: &Gpu) {}
    pub fn last_ms(&self) -> f32 {
        self.dev.last_ms.get()
    }
}
