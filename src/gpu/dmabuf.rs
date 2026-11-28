//! Zero-copy Vulkan DMA-BUF + explicit `SYNC_FD` export for `QQuickRhiItem` (`WP-15`).
//!
//! Creates double-buffered `Rgba16Float` (`VK_FORMAT_R16G16B16A16_SFLOAT`) Vulkan images
//! backed by dedicated `VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT` memory, wraps them
//! into `wgpu::Texture` via `wgpu::Device::create_texture_from_hal`, and exports binary
//! `VK_EXTERNAL_SEMAPHORE_HANDLE_TYPE_SYNC_FD_BIT` semaphores signaled via
//! `wgpu::hal::vulkan::Queue::add_signal_semaphore`.

use std::mem::ManuallyDrop;
use std::os::fd::{FromRawFd, OwnedFd};

use ash::vk;

use crate::preview_shm::{
    ColorSpaceTag, DRM_FORMAT_ABGR16161616F, DRM_FORMAT_MOD_INVALID, DmaBufFrameDesc,
    VK_FORMAT_R16G16B16A16_SFLOAT,
};

use super::Gpu;

/// Vulkan capabilities probed once during `gpu::init()` to gate zero-copy DMA-BUF export.
#[derive(Debug, Clone, Copy)]
pub struct DmaBufCaps {
    pub device_uuid: [u8; 16],
    pub external_memory_dma_buf: bool,
    pub external_semaphore_fd: bool,
}

impl DmaBufCaps {
    pub fn is_supported(self) -> bool {
        self.external_memory_dma_buf && self.external_semaphore_fd
    }
}

/// Queries the physical device's 16-byte Vulkan `deviceUUID` (for matching producer/consumer GPUs).
pub(super) fn query_device_uuid(
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
) -> [u8; 16] {
    let mut id_props = vk::PhysicalDeviceIDProperties::default();
    let mut props2 = vk::PhysicalDeviceProperties2::default().push_next(&mut id_props);
    // SAFETY: `instance` and `physical_device` are valid Vulkan handles; `props2` and `id_props`
    // live for the duration of `get_physical_device_properties2`.
    unsafe {
        instance.get_physical_device_properties2(physical_device, &mut props2);
    }
    id_props.device_uuid
}

/// Exported GPU preview frame ready for `SCM_RIGHTS` delivery to `omastudio-app`.
pub struct ExportedDmaBufFrame {
    pub desc: DmaBufFrameDesc,
    pub dmabuf_fd: OwnedFd,
    pub sync_fd: Option<OwnedFd>,
    pub histogram: crate::pipeline::histogram::HistogramData,
}

const PACK_RGBA16F_WGSL: &str = r#"
struct BlitParams {
    w: u32,
    h: u32,
    _pad0: u32,
    _pad1: u32,
}

@group(0) @binding(0) var<uniform> bp: BlitParams;
@group(0) @binding(1) var<storage, read> px8: array<u32>;
@group(0) @binding(2) var out_tex: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(16, 16)
fn pack_rgba16f(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= bp.w || id.y >= bp.h {
        return;
    }
    let p = px8[id.y * bp.w + id.x];
    let r = f32(p & 255u) / 255.0;
    let g = f32((p >> 8u) & 255u) / 255.0;
    let b = f32((p >> 16u) & 255u) / 255.0;
    textureStore(out_tex, vec2<i32>(i32(id.x), i32(id.y)), vec4<f32>(r, g, b, 1.0));
}
"#;

/// One exportable `Rgba16Float` Vulkan image + dedicated DMA-BUF memory + `SYNC_FD` semaphore.
struct ExportableSlot {
    /// Wrapped `wgpu::Texture` (`TextureMemory::External` — wgpu does not destroy `raw_image`).
    texture: ManuallyDrop<wgpu::Texture>,
    view: ManuallyDrop<wgpu::TextureView>,
    raw_device: ash::Device,
    raw_image: vk::Image,
    raw_memory: vk::DeviceMemory,
    raw_semaphore: vk::Semaphore,
    width: u32,
    height: u32,
    stride: u32,
    offset: u32,
    drm_modifier: u64,
    alloc_size: u64,
}

impl Drop for ExportableSlot {
    fn drop(&mut self) {
        // SAFETY: Drop wgpu view & texture wrapper first (External memory: does not destroy raw_image),
        // then wait for idle and destroy the underlying Vulkan semaphore, image, and memory.
        unsafe {
            ManuallyDrop::drop(&mut self.view);
            ManuallyDrop::drop(&mut self.texture);
            let _ = self.raw_device.device_wait_idle();
            self.raw_device.destroy_semaphore(self.raw_semaphore, None);
            self.raw_device.destroy_image(self.raw_image, None);
            self.raw_device.free_memory(self.raw_memory, None);
        }
    }
}

impl ExportableSlot {
    fn create(gpu: &Gpu, width: u32, height: u32) -> Result<Self, String> {
        // SAFETY: `gpu.device` is a live Vulkan device; `as_hal` yields the active Vulkan HAL device guard.
        let hal_dev = unsafe { gpu.device.as_hal::<wgpu::hal::api::Vulkan>() }
            .ok_or_else(|| "GPU backend is not Vulkan".to_string())?;
        Self::create_with_hal(gpu, &hal_dev, width, height)
    }

    fn create_with_hal(
        gpu: &Gpu,
        hal_dev: &wgpu::hal::vulkan::Device,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let raw_device = hal_dev.raw_device().clone();
        let instance = hal_dev.shared_instance().raw_instance();
        let phys = hal_dev.raw_physical_device();

        let format = vk::Format::R16G16B16A16_SFLOAT;
        let needed_features =
            vk::FormatFeatureFlags::STORAGE_IMAGE | vk::FormatFeatureFlags::SAMPLED_IMAGE;
        let modifiers = query_usable_drm_modifiers(instance, phys, format, needed_features);
        if modifiers.is_empty() {
            return Err("no DRM format modifiers support Rgba16Float storage+sampled".to_string());
        }

        let mut ext_image_info = vk::ExternalMemoryImageCreateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        let mut mod_list_info = vk::ImageDrmFormatModifierListCreateInfoEXT::default()
            .drm_format_modifiers(&modifiers);

        let image_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(vk::Extent3D { width, height, depth: 1 })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
            .usage(
                vk::ImageUsageFlags::STORAGE
                    | vk::ImageUsageFlags::SAMPLED
                    | vk::ImageUsageFlags::TRANSFER_SRC
                    | vk::ImageUsageFlags::TRANSFER_DST,
            )
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .push_next(&mut ext_image_info)
            .push_next(&mut mod_list_info);

        // SAFETY: `image_info` and its chained pNext structs are valid for the duration of `create_image`.
        let raw_image = unsafe { raw_device.create_image(&image_info, None) }
            .map_err(|e| format!("vkCreateImage (DMA-BUF): {e:?}"))?;

        // SAFETY: `raw_image` was just created on `raw_device`.
        let mem_reqs = unsafe { raw_device.get_image_memory_requirements(raw_image) };
        let Some(memory_type_index) = find_memory_type(
            instance,
            phys,
            mem_reqs.memory_type_bits,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )
        .or_else(|| {
            find_memory_type(instance, phys, mem_reqs.memory_type_bits, vk::MemoryPropertyFlags::empty())
        }) else {
            // SAFETY: cleanup `raw_image` before returning error.
            unsafe { raw_device.destroy_image(raw_image, None) };
            return Err("no compatible Vulkan memory type for DMA-BUF image".to_string());
        };

        let mut export_alloc = vk::ExportMemoryAllocateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(raw_image);
        let alloc_info = vk::MemoryAllocateInfo::default()
            .allocation_size(mem_reqs.size)
            .memory_type_index(memory_type_index)
            .push_next(&mut export_alloc)
            .push_next(&mut dedicated);

        // SAFETY: `alloc_info` and its pNext chain are valid; on error we destroy `raw_image`.
        let raw_memory = match unsafe { raw_device.allocate_memory(&alloc_info, None) } {
            Ok(m) => m,
            Err(e) => {
                // SAFETY: `raw_image` has no bound memory yet and is owned by this function.
                unsafe { raw_device.destroy_image(raw_image, None) };
                return Err(format!("vkAllocateMemory (DMA-BUF): {e:?}"));
            }
        };

        // SAFETY: `raw_image` and `raw_memory` match `mem_reqs` with offset 0.
        if let Err(e) = unsafe { raw_device.bind_image_memory(raw_image, raw_memory, 0) } {
            // SAFETY: cleanup both Vulkan handles on bind failure.
            unsafe {
                raw_device.destroy_image(raw_image, None);
                raw_device.free_memory(raw_memory, None);
            }
            return Err(format!("vkBindImageMemory (DMA-BUF): {e:?}"));
        }

        // Query chosen DRM format modifier and plane-0 layout.
        let drm_mod_fn = ash::ext::image_drm_format_modifier::Device::new(instance, &raw_device);
        let mut mod_props = vk::ImageDrmFormatModifierPropertiesEXT::default();
        // SAFETY: `raw_image` was created with `DRM_FORMAT_MODIFIER_EXT` and bound to memory.
        let drm_modifier = match unsafe {
            drm_mod_fn.get_image_drm_format_modifier_properties(raw_image, &mut mod_props)
        } {
            Ok(()) => mod_props.drm_format_modifier,
            Err(_) => DRM_FORMAT_MOD_INVALID,
        };

        let subresource = vk::ImageSubresource {
            aspect_mask: vk::ImageAspectFlags::MEMORY_PLANE_0_EXT,
            mip_level: 0,
            array_layer: 0,
        };
        // SAFETY: `raw_image` is bound and uses `DRM_FORMAT_MODIFIER_EXT` (plane 0 is valid).
        let layout = unsafe { raw_device.get_image_subresource_layout(raw_image, subresource) };
        let stride = u32::try_from(layout.row_pitch).unwrap_or(width.saturating_mul(8));
        let offset = u32::try_from(layout.offset).unwrap_or(0);

        // Probe that the driver actually supports exporting a DMA-BUF FD from this memory now
        // (avoids allocating a broken pool on software ICDs that advertise extensions without export).
        let ext_mem_fd = ash::khr::external_memory_fd::Device::new(instance, &raw_device);
        let fd_info = vk::MemoryGetFdInfoKHR::default()
            .memory(raw_memory)
            .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        // SAFETY: `raw_memory` was allocated with `DMA_BUF_EXT`; immediately close the probe FD.
        match unsafe { ext_mem_fd.get_memory_fd(&fd_info) } {
            Ok(probe_fd) if probe_fd >= 0 => {
                // SAFETY: `probe_fd` is a fresh FD returned by `vkGetMemoryFdKHR`.
                drop(unsafe { OwnedFd::from_raw_fd(probe_fd) });
            }
            Ok(_) | Err(_) => {
                // SAFETY: cleanup Vulkan handles when DMA-BUF FD export is unsupported.
                unsafe {
                    raw_device.destroy_image(raw_image, None);
                    raw_device.free_memory(raw_memory, None);
                }
                return Err("vkGetMemoryFdKHR (DMA_BUF_EXT) unsupported on this adapter".to_string());
            }
        }

        // Create exportable binary semaphore with `VkExportSemaphoreCreateInfo` chained onto
        // `VkSemaphoreCreateInfo::pNext` (never onto `VkSubmitInfo::pNext`).
        let mut export_sem_info = vk::ExportSemaphoreCreateInfo::default()
            .handle_types(vk::ExternalSemaphoreHandleTypeFlags::SYNC_FD);
        let sem_info = vk::SemaphoreCreateInfo::default().push_next(&mut export_sem_info);
        // SAFETY: `sem_info` and `export_sem_info` are valid for `create_semaphore`.
        let raw_semaphore = match unsafe { raw_device.create_semaphore(&sem_info, None) } {
            Ok(s) => s,
            Err(e) => {
                // SAFETY: cleanup image and memory on semaphore creation failure.
                unsafe {
                    raw_device.destroy_image(raw_image, None);
                    raw_device.free_memory(raw_memory, None);
                }
                return Err(format!("vkCreateSemaphore (SYNC_FD): {e:?}"));
            }
        };

        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let hal_desc = wgpu::hal::TextureDescriptor {
            label: Some("omastudio dmabuf preview"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::wgt::TextureUses::STORAGE_READ_WRITE
                | wgpu::wgt::TextureUses::RESOURCE
                | wgpu::wgt::TextureUses::COPY_SRC
                | wgpu::wgt::TextureUses::COPY_DST,
            memory_flags: wgpu::hal::MemoryFlags::empty(),
            view_formats: Vec::new(),
        };
        let wgpu_desc = wgpu::TextureDescriptor {
            label: Some("omastudio dmabuf preview"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        };

        // SAFETY: `raw_image` matches `hal_desc` and `TextureMemory::External` leaves destruction
        // to `ExportableSlot::drop` after `ManuallyDrop::drop(&mut self.texture)`.
        let hal_tex = unsafe {
            hal_dev.texture_from_raw(raw_image, &hal_desc, None, wgpu::hal::vulkan::TextureMemory::External)
        };
        // SAFETY: `hal_tex` was created on this exact Vulkan device and starts in UNINITIALIZED state;
        // `pack_rgba16f` overwrites every pixel before export.
        let texture = unsafe {
            gpu.device.create_texture_from_hal::<wgpu::hal::api::Vulkan>(
                hal_tex,
                &wgpu_desc,
                wgpu::wgt::TextureUses::UNINITIALIZED,
            )
        };
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        Ok(Self {
            texture: ManuallyDrop::new(texture),
            view: ManuallyDrop::new(view),
            raw_device,
            raw_image,
            raw_memory,
            raw_semaphore,
            width,
            height,
            stride,
            offset,
            drm_modifier,
            alloc_size: mem_reqs.size,
        })
    }

    fn export_dmabuf_fd(&self, instance: &ash::Instance) -> Result<OwnedFd, String> {
        let ext_mem_fd = ash::khr::external_memory_fd::Device::new(instance, &self.raw_device);
        let info = vk::MemoryGetFdInfoKHR::default()
            .memory(self.raw_memory)
            .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        // SAFETY: `self.raw_memory` was allocated with `DMA_BUF_EXT` and remains valid for `self`.
        let fd = unsafe { ext_mem_fd.get_memory_fd(&info) }
            .map_err(|e| format!("vkGetMemoryFdKHR: {e:?}"))?;
        if fd < 0 {
            return Err("vkGetMemoryFdKHR returned negative fd".to_string());
        }
        // SAFETY: `fd >= 0` is a newly duplicated DMA-BUF descriptor owned by the caller.
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }

    fn export_sync_fd(&self, instance: &ash::Instance) -> Result<OwnedFd, String> {
        let ext_sem_fd = ash::khr::external_semaphore_fd::Device::new(instance, &self.raw_device);
        let info = vk::SemaphoreGetFdInfoKHR::default()
            .semaphore(self.raw_semaphore)
            .handle_type(vk::ExternalSemaphoreHandleTypeFlags::SYNC_FD);
        // SAFETY: `self.raw_semaphore` was created with `SYNC_FD` export enabled and just signaled.
        let fd = unsafe { ext_sem_fd.get_semaphore_fd(&info) }
            .map_err(|e| format!("vkGetSemaphoreFdKHR: {e:?}"))?;
        if fd < 0 {
            return Err("vkGetSemaphoreFdKHR returned negative fd".to_string());
        }
        // SAFETY: `fd >= 0` is a newly exported Linux sync_file descriptor owned by the caller.
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }
}

fn query_usable_drm_modifiers(
    instance: &ash::Instance,
    phys: vk::PhysicalDevice,
    format: vk::Format,
    needed_features: vk::FormatFeatureFlags,
) -> Vec<u64> {
    let mut mod_list = vk::DrmFormatModifierPropertiesListEXT::default();
    let mut fmt_props = vk::FormatProperties2::default().push_next(&mut mod_list);
    // SAFETY: first query counts available DRM format modifiers for `(phys, format)`.
    unsafe {
        instance.get_physical_device_format_properties2(phys, format, &mut fmt_props);
    }
    let count = mod_list.drm_format_modifier_count as usize;
    if count == 0 {
        return Vec::new();
    }
    let mut props = vec![vk::DrmFormatModifierPropertiesEXT::default(); count];
    let mut mod_list = vk::DrmFormatModifierPropertiesListEXT::default()
        .drm_format_modifier_properties(&mut props);
    let mut fmt_props = vk::FormatProperties2::default().push_next(&mut mod_list);
    // SAFETY: second query fills `props` with exact modifier properties.
    unsafe {
        instance.get_physical_device_format_properties2(phys, format, &mut fmt_props);
    }

    // Prefer single-plane modifiers that support both STORAGE_IMAGE and SAMPLED_IMAGE; put LINEAR (0) first.
    let mut usable: Vec<u64> = props
        .into_iter()
        .filter(|p| {
            p.drm_format_modifier_plane_count == 1
                && p.drm_format_modifier_tiling_features.contains(needed_features)
        })
        .map(|p| p.drm_format_modifier)
        .collect();
    if let Some(pos) = usable.iter().position(|&m| m == 0) {
        usable.swap(0, pos);
    }
    usable
}

fn find_memory_type(
    instance: &ash::Instance,
    phys: vk::PhysicalDevice,
    type_bits: u32,
    required: vk::MemoryPropertyFlags,
) -> Option<u32> {
    // SAFETY: `instance` and `phys` are valid Vulkan handles.
    let mem_props = unsafe { instance.get_physical_device_memory_properties(phys) };
    for idx in 0..mem_props.memory_type_count {
        let supported = (type_bits & (1u32 << idx)) != 0;
        let flags = mem_props.memory_types[idx as usize].property_flags;
        if supported && flags.contains(required) {
            return Some(idx);
        }
    }
    None
}

/// Double-buffered `Rgba16Float` DMA-BUF swap pool + `pack_rgba16f` compute pipeline.
pub(super) struct DmaBufPool {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    params: wgpu::Buffer,
    slots: Option<[ExportableSlot; 2]>,
    next_slot: usize,
    generation: u64,
}

impl DmaBufPool {
    pub(super) fn new(gpu: &Gpu) -> Result<Self, String> {
        let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("dmabuf pack_rgba16f"),
            source: wgpu::ShaderSource::Wgsl(PACK_RGBA16F_WGSL.into()),
        });
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("dmabuf pack_rgba16f"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(16),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba16Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("dmabuf pack_rgba16f"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = gpu.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("pack_rgba16f"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("pack_rgba16f"),
            compilation_options: Default::default(),
            cache: gpu.pipeline_cache.as_ref(),
        });
        let params = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dmabuf pack_rgba16f params"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if let Some(err) = pollster::block_on(scope.pop()) {
            return Err(format!("dmabuf pack_rgba16f shader: {err}"));
        }
        Ok(Self {
            pipeline,
            layout,
            params,
            slots: None,
            next_slot: 0,
            generation: 0,
        })
    }

    fn ensure_slots(&mut self, gpu: &Gpu, width: u32, height: u32) -> Result<usize, String> {
        let reuse = self
            .slots
            .as_ref()
            .is_some_and(|[a, _]| a.width == width && a.height == height);
        if !reuse {
            self.slots = None;
            let s0 = ExportableSlot::create(gpu, width, height)?;
            let s1 = ExportableSlot::create(gpu, width, height)?;
            self.slots = Some([s0, s1]);
            self.next_slot = 0;
        }
        let idx = self.next_slot % 2;
        self.next_slot = (idx + 1) % 2;
        Ok(idx)
    }

    /// Encodes `pack_rgba16f` from `px8` into the next double-buffered exportable `VkImage`,
    /// attaches its `SYNC_FD` export semaphore to `gpu.queue`, submits `enc`, and exports
    /// `(DmaBufFrameDesc, dmabuf_fd, sync_fd)` alongside the submission index.
    pub(super) fn encode_submit_and_export(
        &mut self,
        gpu: &Gpu,
        mut enc: wgpu::CommandEncoder,
        px8: &wgpu::Buffer,
        width: u32,
        height: u32,
        color_space: ColorSpaceTag,
    ) -> Result<(wgpu::SubmissionIndex, DmaBufFrameDesc, OwnedFd, Option<OwnedFd>), String> {
        let caps = gpu
            .dmabuf_caps
            .filter(|c| c.is_supported())
            .ok_or_else(|| "GPU lacks Vulkan DMA-BUF / SYNC_FD export support".to_string())?;

        self.generation = self.generation.wrapping_add(1).max(1);
        let generation = self.generation;
        let slot_idx = self.ensure_slots(gpu, width, height)?;
        let slot = &self.slots.as_ref().expect("just initialized")[slot_idx];

        let uniform_words = [width, height, 0u32, 0u32];
        gpu.queue.write_buffer(&self.params, 0, super::as_bytes(&uniform_words));

        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("dmabuf pack_rgba16f bg"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: px8.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&slot.view),
                },
            ],
        });

        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("dmabuf pack_rgba16f"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(width.div_ceil(16), height.div_ceil(16), 1);
        }

        // Register the exportable binary semaphore on wgpu-hal's Vulkan queue immediately
        // before `queue.submit(...)` so the submitted batch signals it atomically.
        {
            // SAFETY: `slot.raw_semaphore` is a valid binary semaphore created on `gpu.device` with
            // `VkExportSemaphoreCreateInfo { handle_types: SYNC_FD }`.
            let hal_queue = unsafe { gpu.queue.as_hal::<wgpu::hal::api::Vulkan>() }
                .ok_or_else(|| "Vulkan HAL queue unavailable for SYNC_FD export".to_string())?;
            hal_queue.add_signal_semaphore(slot.raw_semaphore, None);
        }

        let cmd_buf = enc.finish();
        let submission_index = gpu.queue.submit([cmd_buf]);

        let (dmabuf_fd, sync_fd) = {
            // SAFETY: `gpu.device` is live and `slot` belongs to it; export both descriptors after submit.
            let hal_dev = unsafe { gpu.device.as_hal::<wgpu::hal::api::Vulkan>() }
                .ok_or_else(|| "Vulkan HAL device unavailable".to_string())?;
            let inst = hal_dev.shared_instance().raw_instance();
            let dmabuf_fd = slot.export_dmabuf_fd(inst)?;
            let sync_fd = slot.export_sync_fd(inst).ok();
            (dmabuf_fd, sync_fd)
        };

        let desc = DmaBufFrameDesc {
            generation,
            width: slot.width,
            height: slot.height,
            stride: slot.stride,
            offset: slot.offset,
            vk_format: VK_FORMAT_R16G16B16A16_SFLOAT,
            drm_format: DRM_FORMAT_ABGR16161616F,
            drm_modifier: slot.drm_modifier,
            alloc_size: slot.alloc_size,
            color_space,
            has_sync_fd: sync_fd.is_some(),
            device_uuid: caps.device_uuid,
        };

        Ok((submission_index, desc, dmabuf_fd, sync_fd))
    }
}
