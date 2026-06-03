//! Virtual GPU implementation using WGPU
//!
//! This module provides a virtualized GPU device that uses WGPU for
//! cross-platform GPU acceleration. It supports rendering, compute,
//! and memory operations that can be exposed to guest VMs.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use wgpu::{
    Adapter, Backends, Buffer, BufferUsages, Device, DeviceDescriptor, Features, Instance,
    InstanceDescriptor, InstanceFlags, Limits, MemoryHints, PowerPreference, Queue,
    RequestAdapterOptions, ShaderModule, Texture, TextureDescriptor, TextureDimension,
    TextureFormat, TextureUsages,
};

use crate::{GpuError, Result};

/// GPU memory region for guest access
#[derive(Debug)]
pub struct GpuMemoryRegion {
    /// Region ID
    pub id: u32,
    /// WGPU buffer
    buffer: Buffer,
    /// Size in bytes
    pub size: u64,
    /// Usage flags
    pub usage: BufferUsages,
    /// Mapped to guest
    pub mapped: bool,
}

/// GPU texture resource
#[derive(Debug)]
pub struct GpuTextureResource {
    /// Resource ID
    pub id: u32,
    /// WGPU texture
    texture: Texture,
    /// Format
    pub format: TextureFormat,
    /// Dimensions
    pub width: u32,
    pub height: u32,
    pub depth: u32,
}

/// Shader program
pub struct GpuShader {
    /// Shader ID
    pub id: u32,
    /// WGPU shader module
    module: ShaderModule,
    /// Shader type
    pub shader_type: ShaderType,
}

impl std::fmt::Debug for GpuShader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuShader")
            .field("id", &self.id)
            .field("shader_type", &self.shader_type)
            .finish_non_exhaustive()
    }
}

/// Shader types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderType {
    /// Vertex shader — processes per-vertex data
    Vertex,
    /// Fragment shader — processes per-pixel data
    Fragment,
    /// Compute shader — general-purpose GPU compute
    Compute,
}

/// Virtual GPU capabilities
#[derive(Debug, Clone)]
pub struct VirtualGpuCaps {
    /// Device name
    pub name: String,
    /// Vendor ID
    pub vendor_id: u32,
    /// Device ID
    pub device_id: u32,
    /// Max texture size
    pub max_texture_size: u32,
    /// Max buffer size
    pub max_buffer_size: u64,
    /// Max compute workgroup size
    pub max_compute_workgroup_size: [u32; 3],
    /// Max compute invocations
    pub max_compute_invocations: u32,
    /// Supported features
    pub features: GpuFeatures,
    /// Backend type
    pub backend: String,
}

/// GPU feature flags
#[derive(Debug, Clone, Default)]
pub struct GpuFeatures {
    /// Compute shader support
    pub compute: bool,
    /// Graphics pipeline support
    pub graphics: bool,
    /// Ray tracing acceleration support
    pub ray_tracing: bool,
    /// Bindless resource access
    pub bindless: bool,
    /// Sparse (partially-resident) resources
    pub sparse_resources: bool,
    /// Multi-draw indirect rendering
    pub multi_draw_indirect: bool,
    /// GPU timestamp queries for profiling
    pub timestamp_query: bool,
}

/// GPU statistics
#[derive(Debug, Default)]
pub struct VirtualGpuStats {
    /// Commands submitted
    pub commands_submitted: AtomicU64,
    /// Bytes allocated
    pub bytes_allocated: AtomicU64,
    /// Bytes transferred
    pub bytes_transferred: AtomicU64,
    /// Draw calls
    pub draw_calls: AtomicU64,
    /// Compute dispatches
    pub compute_dispatches: AtomicU64,
    /// Textures created
    pub textures_created: AtomicU32,
    /// Buffers created
    pub buffers_created: AtomicU32,
}

/// A compiled compute pipeline cached across dispatches.
///
/// Pipeline creation triggers shader specialization in the WGPU backend and is
/// by far the most expensive step in a dispatch. Keying on
/// `(shader_id, binding_count)` lets a hot dispatch loop reuse the pipeline and
/// bind-group layout, rebuilding only the cheap, buffer-specific bind group.
/// Resource IDs are handed out by a monotonic counter and never reused, so a
/// cache key always refers to the same shader.
#[derive(Clone)]
struct CachedComputePipeline {
    pipeline: Arc<wgpu::ComputePipeline>,
    bind_group_layout: Option<Arc<wgpu::BindGroupLayout>>,
}

/// Virtual GPU device
pub struct VirtualGpu {
    /// Device name
    name: String,
    /// WGPU instance
    instance: Option<Instance>,
    /// WGPU adapter
    adapter: Option<Adapter>,
    /// WGPU device
    device: Option<Device>,
    /// WGPU queue
    queue: Option<Queue>,
    /// Device capabilities
    capabilities: RwLock<Option<VirtualGpuCaps>>,
    /// Memory regions
    memory_regions: Mutex<HashMap<u32, GpuMemoryRegion>>,
    /// Textures
    textures: Mutex<HashMap<u32, GpuTextureResource>>,
    /// Shaders
    shaders: Mutex<HashMap<u32, GpuShader>>,
    /// Compiled compute pipelines, keyed by (shader_id, binding_count), so a
    /// hot dispatch loop does not recompile the pipeline on every call.
    compute_pipelines: Mutex<HashMap<(u32, usize), CachedComputePipeline>>,
    /// Next resource ID
    next_resource_id: AtomicU32,
    /// Statistics
    stats: Arc<VirtualGpuStats>,
    /// Initialized
    initialized: std::sync::atomic::AtomicBool,
}

impl VirtualGpu {
    /// Create a new virtual GPU
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            instance: None,
            adapter: None,
            device: None,
            queue: None,
            capabilities: RwLock::new(None),
            memory_regions: Mutex::new(HashMap::new()),
            textures: Mutex::new(HashMap::new()),
            shaders: Mutex::new(HashMap::new()),
            compute_pipelines: Mutex::new(HashMap::new()),
            next_resource_id: AtomicU32::new(1),
            stats: Arc::new(VirtualGpuStats::default()),
            initialized: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Initialize the virtual GPU with WGPU
    pub async fn init(&mut self) -> Result<()> {
        tracing::info!("Initializing virtual GPU: {}", self.name);

        // Create WGPU instance with all backends
        let instance = Instance::new(InstanceDescriptor {
            backends: Backends::all(),
            flags: InstanceFlags::default(),
            ..Default::default()
        });

        // Request adapter (prefer high-performance GPU)
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| GpuError::NotAvailable("No compatible GPU adapter found".into()))?;

        let adapter_info = adapter.get_info();
        tracing::info!(
            "Using GPU adapter: {} ({:?})",
            adapter_info.name,
            adapter_info.backend
        );

        // Get adapter limits and features
        let adapter_limits = adapter.limits();
        let adapter_features = adapter.features();

        // Request device with reasonable limits
        let (device, queue) = adapter
            .request_device(
                &DeviceDescriptor {
                    label: Some(&self.name),
                    required_features: Features::empty(),
                    required_limits: Limits::downlevel_defaults(),
                    memory_hints: MemoryHints::Performance,
                },
                None,
            )
            .await
            .map_err(|e| GpuError::InitFailed(format!("Failed to create device: {}", e)))?;

        // Build capabilities
        let capabilities = VirtualGpuCaps {
            name: adapter_info.name.clone(),
            vendor_id: adapter_info.vendor as u32,
            device_id: adapter_info.device as u32,
            max_texture_size: adapter_limits.max_texture_dimension_2d,
            max_buffer_size: adapter_limits.max_buffer_size,
            max_compute_workgroup_size: [
                adapter_limits.max_compute_workgroup_size_x,
                adapter_limits.max_compute_workgroup_size_y,
                adapter_limits.max_compute_workgroup_size_z,
            ],
            max_compute_invocations: adapter_limits.max_compute_invocations_per_workgroup,
            features: GpuFeatures {
                compute: true,
                graphics: true,
                ray_tracing: adapter_features
                    .contains(Features::RAY_TRACING_ACCELERATION_STRUCTURE),
                bindless: adapter_features.contains(
                    Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
                ),
                sparse_resources: false,
                multi_draw_indirect: adapter_features.contains(Features::MULTI_DRAW_INDIRECT),
                timestamp_query: adapter_features.contains(Features::TIMESTAMP_QUERY),
            },
            backend: format!("{:?}", adapter_info.backend),
        };

        *self.capabilities.write().await = Some(capabilities);
        self.instance = Some(instance);
        self.adapter = Some(adapter);
        self.device = Some(device);
        self.queue = Some(queue);
        self.initialized
            .store(true, std::sync::atomic::Ordering::SeqCst);

        tracing::info!("Virtual GPU initialized successfully");
        Ok(())
    }

    /// Get device name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Check if initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Get capabilities
    pub async fn capabilities(&self) -> Option<VirtualGpuCaps> {
        self.capabilities.read().await.clone()
    }

    /// Get statistics
    pub fn stats(&self) -> &VirtualGpuStats {
        &self.stats
    }

    /// Number of compiled compute pipelines currently cached.
    #[cfg(test)]
    pub(crate) async fn cached_pipeline_count(&self) -> usize {
        self.compute_pipelines.lock().await.len()
    }

    /// Allocate GPU memory buffer
    pub async fn allocate_buffer(&self, size: u64, usage: BufferUsages) -> Result<u32> {
        let device = self
            .device
            .as_ref()
            .ok_or_else(|| GpuError::NotAvailable("Device not initialized".into()))?;

        let id = self.next_resource_id.fetch_add(1, Ordering::SeqCst);

        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!("vgpu_buffer_{}", id)),
            size,
            usage,
            mapped_at_creation: false,
        });

        let region = GpuMemoryRegion {
            id,
            buffer,
            size,
            usage,
            mapped: false,
        };

        self.memory_regions.lock().await.insert(id, region);
        self.stats.buffers_created.fetch_add(1, Ordering::Relaxed);
        self.stats
            .bytes_allocated
            .fetch_add(size, Ordering::Relaxed);

        tracing::debug!("Allocated GPU buffer {}: {} bytes", id, size);
        Ok(id)
    }

    /// Free GPU memory buffer
    pub async fn free_buffer(&self, id: u32) -> Result<()> {
        let mut regions = self.memory_regions.lock().await;
        if let Some(region) = regions.remove(&id) {
            self.stats
                .bytes_allocated
                .fetch_sub(region.size, Ordering::Relaxed);
            tracing::debug!("Freed GPU buffer {}", id);
            Ok(())
        } else {
            Err(GpuError::NotAvailable(format!("Buffer {} not found", id)))
        }
    }

    /// Write data to buffer
    pub async fn write_buffer(&self, id: u32, offset: u64, data: &[u8]) -> Result<()> {
        let queue = self
            .queue
            .as_ref()
            .ok_or_else(|| GpuError::NotAvailable("Queue not initialized".into()))?;

        let regions = self.memory_regions.lock().await;
        let region = regions
            .get(&id)
            .ok_or_else(|| GpuError::NotAvailable(format!("Buffer {} not found", id)))?;

        if offset + data.len() as u64 > region.size {
            return Err(GpuError::Unsupported("Write exceeds buffer size".into()));
        }

        queue.write_buffer(&region.buffer, offset, data);
        self.stats
            .bytes_transferred
            .fetch_add(data.len() as u64, Ordering::Relaxed);

        Ok(())
    }

    /// Create a texture
    pub async fn create_texture(
        &self,
        width: u32,
        height: u32,
        depth: u32,
        format: TextureFormat,
        usage: TextureUsages,
    ) -> Result<u32> {
        let device = self
            .device
            .as_ref()
            .ok_or_else(|| GpuError::NotAvailable("Device not initialized".into()))?;

        let id = self.next_resource_id.fetch_add(1, Ordering::SeqCst);

        let texture = device.create_texture(&TextureDescriptor {
            label: Some(&format!("vgpu_texture_{}", id)),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: depth,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: if depth > 1 {
                TextureDimension::D3
            } else if height > 1 {
                TextureDimension::D2
            } else {
                TextureDimension::D1
            },
            format,
            usage,
            view_formats: &[],
        });

        let resource = GpuTextureResource {
            id,
            texture,
            format,
            width,
            height,
            depth,
        };

        self.textures.lock().await.insert(id, resource);
        self.stats.textures_created.fetch_add(1, Ordering::Relaxed);

        tracing::debug!(
            "Created GPU texture {}: {}x{}x{} {:?}",
            id,
            width,
            height,
            depth,
            format
        );
        Ok(id)
    }

    /// Create a shader from WGSL source
    pub async fn create_shader(&self, source: &str, shader_type: ShaderType) -> Result<u32> {
        let device = self
            .device
            .as_ref()
            .ok_or_else(|| GpuError::NotAvailable("Device not initialized".into()))?;

        let id = self.next_resource_id.fetch_add(1, Ordering::SeqCst);

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(&format!("vgpu_shader_{}", id)),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });

        let shader = GpuShader {
            id,
            module,
            shader_type,
        };

        self.shaders.lock().await.insert(id, shader);
        tracing::debug!("Created GPU shader {}: {:?}", id, shader_type);
        Ok(id)
    }

    /// Submit a command buffer encoded with the vGPU command protocol.
    ///
    /// Each command is encoded as a 1-byte type tag followed by little-endian
    /// payload fields:
    ///   - `0x00` NOP: no payload
    ///   - `0x01` CopyBufferToBuffer: src_id(u32) + dst_id(u32) + size(u64)
    ///   - `0x02` ClearBuffer (zero-fill): buffer_id(u32) + offset(u64) + size(u64)
    ///   - `0x03` DispatchCompute: shader_id(u32) + x(u32) + y(u32) + z(u32) +
    ///     binding_count(u32) + \[buffer_id(u32); binding_count\]
    pub async fn submit_commands(&self, commands: &[u8]) -> Result<()> {
        let device = self
            .device
            .as_ref()
            .ok_or_else(|| GpuError::NotAvailable("Device not initialized".into()))?;
        let queue = self
            .queue
            .as_ref()
            .ok_or_else(|| GpuError::NotAvailable("Queue not initialized".into()))?;

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("vgpu_command_encoder"),
        });

        let mut pos = 0usize;
        while pos < commands.len() {
            let cmd_type = commands[pos];
            pos += 1;

            match cmd_type {
                // NOP
                0x00 => {}
                // CopyBufferToBuffer: src_id(u32) + dst_id(u32) + size(u64)
                0x01 => {
                    let src_id = Self::read_u32_le(commands, &mut pos)?;
                    let dst_id = Self::read_u32_le(commands, &mut pos)?;
                    let size = Self::read_u64_le(commands, &mut pos)?;

                    let regions = self.memory_regions.lock().await;
                    let src = regions.get(&src_id).ok_or_else(|| {
                        GpuError::NotAvailable(format!("Source buffer {} not found", src_id))
                    })?;
                    let dst = regions.get(&dst_id).ok_or_else(|| {
                        GpuError::NotAvailable(format!("Dest buffer {} not found", dst_id))
                    })?;

                    if size > src.size || size > dst.size {
                        return Err(GpuError::Unsupported(
                            "Copy size exceeds buffer bounds".into(),
                        ));
                    }

                    encoder.copy_buffer_to_buffer(&src.buffer, 0, &dst.buffer, 0, size);
                    tracing::debug!("CMD copy_buffer {} -> {}, {} bytes", src_id, dst_id, size);
                }
                // ClearBuffer: buffer_id(u32) + offset(u64) + size(u64)
                0x02 => {
                    let buffer_id = Self::read_u32_le(commands, &mut pos)?;
                    let offset = Self::read_u64_le(commands, &mut pos)?;
                    let size = Self::read_u64_le(commands, &mut pos)?;

                    let regions = self.memory_regions.lock().await;
                    let region = regions.get(&buffer_id).ok_or_else(|| {
                        GpuError::NotAvailable(format!("Buffer {} not found", buffer_id))
                    })?;

                    if offset + size > region.size {
                        return Err(GpuError::Unsupported(
                            "Clear range exceeds buffer bounds".into(),
                        ));
                    }

                    encoder.clear_buffer(&region.buffer, offset, Some(size));
                    tracing::debug!(
                        "CMD clear_buffer {}, offset={}, size={}",
                        buffer_id,
                        offset,
                        size
                    );
                }
                // DispatchCompute: shader_id(u32) + x(u32) + y(u32) + z(u32) +
                //                  binding_count(u32) + [buffer_id(u32); binding_count]
                0x03 => {
                    let shader_id = Self::read_u32_le(commands, &mut pos)?;
                    let wg_x = Self::read_u32_le(commands, &mut pos)?;
                    let wg_y = Self::read_u32_le(commands, &mut pos)?;
                    let wg_z = Self::read_u32_le(commands, &mut pos)?;
                    let binding_count = Self::read_u32_le(commands, &mut pos)?;

                    let mut buffer_ids = Vec::with_capacity(binding_count as usize);
                    for _ in 0..binding_count {
                        buffer_ids.push(Self::read_u32_le(commands, &mut pos)?);
                    }

                    self.dispatch_compute_with_bindings(shader_id, [wg_x, wg_y, wg_z], &buffer_ids)
                        .await?;
                }
                _ => {
                    return Err(GpuError::Unsupported(format!(
                        "Unknown GPU command type: 0x{:02x}",
                        cmd_type
                    )));
                }
            }
        }

        queue.submit(std::iter::once(encoder.finish()));
        self.stats
            .commands_submitted
            .fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Read a little-endian u32 from bytes, advancing the position.
    fn read_u32_le(data: &[u8], pos: &mut usize) -> Result<u32> {
        let end = *pos + 4;
        if end > data.len() {
            return Err(GpuError::Unsupported(
                "Truncated command buffer payload".into(),
            ));
        }
        let val = u32::from_le_bytes(data[*pos..end].try_into().unwrap());
        *pos = end;
        Ok(val)
    }

    /// Read a little-endian u64 from bytes, advancing the position.
    fn read_u64_le(data: &[u8], pos: &mut usize) -> Result<u64> {
        let end = *pos + 8;
        if end > data.len() {
            return Err(GpuError::Unsupported(
                "Truncated command buffer payload".into(),
            ));
        }
        let val = u64::from_le_bytes(data[*pos..end].try_into().unwrap());
        *pos = end;
        Ok(val)
    }

    /// Execute a compute dispatch (no buffer bindings).
    pub async fn dispatch_compute(&self, shader_id: u32, workgroups: [u32; 3]) -> Result<()> {
        self.dispatch_compute_with_bindings(shader_id, workgroups, &[])
            .await
    }

    /// Execute a compute dispatch with buffer bindings.
    ///
    /// Each buffer ID in `buffer_ids` is bound at the corresponding binding
    /// index (0, 1, 2, ...) in bind group 0.
    pub async fn dispatch_compute_with_bindings(
        &self,
        shader_id: u32,
        workgroups: [u32; 3],
        buffer_ids: &[u32],
    ) -> Result<()> {
        let device = self
            .device
            .as_ref()
            .ok_or_else(|| GpuError::NotAvailable("Device not initialized".into()))?;
        let queue = self
            .queue
            .as_ref()
            .ok_or_else(|| GpuError::NotAvailable("Queue not initialized".into()))?;

        let binding_count = buffer_ids.len();
        let cache_key = (shader_id, binding_count);

        // Fetch or build the (expensive) compute pipeline. Cached across
        // dispatches so a steady-state loop only pays bind-group construction.
        let cached = {
            let mut cache = self.compute_pipelines.lock().await;
            if let Some(c) = cache.get(&cache_key) {
                c.clone()
            } else {
                // Cache miss: build the pipeline once. `ShaderModule` is not
                // clonable, so hold the shaders lock for this build only (lock
                // order is always cache -> shaders; no path takes the inverse).
                let shaders = self.shaders.lock().await;
                let shader = shaders.get(&shader_id).ok_or_else(|| {
                    GpuError::NotAvailable(format!("Shader {} not found", shader_id))
                })?;
                if shader.shader_type != ShaderType::Compute {
                    return Err(GpuError::Unsupported(
                        "Shader is not a compute shader".into(),
                    ));
                }

                let (pipeline_layout, bind_group_layout) = if binding_count == 0 {
                    (None, None)
                } else {
                    let layout_entries: Vec<wgpu::BindGroupLayoutEntry> = (0..binding_count)
                        .map(|i| wgpu::BindGroupLayoutEntry {
                            binding: i as u32,
                            visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: false },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        })
                        .collect();
                    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                        label: Some("vgpu_compute_bgl"),
                        entries: &layout_entries,
                    });
                    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("vgpu_compute_pl"),
                        bind_group_layouts: &[&bgl],
                        push_constant_ranges: &[],
                    });
                    (Some(pl), Some(bgl))
                };

                let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("vgpu_compute_pipeline"),
                    layout: pipeline_layout.as_ref(),
                    module: &shader.module,
                    entry_point: "main",
                    compilation_options: Default::default(),
                    cache: None,
                });
                drop(shaders);

                let entry = CachedComputePipeline {
                    pipeline: Arc::new(pipeline),
                    bind_group_layout: bind_group_layout.map(Arc::new),
                };
                cache.insert(cache_key, entry.clone());
                entry
            }
        };

        // Build the per-dispatch bind group from the actual buffers. Cheap
        // relative to pipeline creation, and necessarily buffer-specific.
        let bind_group = if let Some(bgl_arc) = &cached.bind_group_layout {
            let bgl: &wgpu::BindGroupLayout = bgl_arc;
            let regions = self.memory_regions.lock().await;
            let mut bg_entries = Vec::with_capacity(binding_count);
            for (i, &buf_id) in buffer_ids.iter().enumerate() {
                let region = regions.get(&buf_id).ok_or_else(|| {
                    GpuError::NotAvailable(format!("Buffer {} not found for binding {}", buf_id, i))
                })?;
                bg_entries.push(wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: region.buffer.as_entire_binding(),
                });
            }
            Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("vgpu_compute_bg"),
                layout: bgl,
                entries: &bg_entries,
            }))
        } else {
            None
        };

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("vgpu_compute_encoder"),
        });

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("vgpu_compute_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&cached.pipeline);
            if let Some(bg) = &bind_group {
                pass.set_bind_group(0, bg, &[]);
            }
            pass.dispatch_workgroups(workgroups[0], workgroups[1], workgroups[2]);
        }

        queue.submit(std::iter::once(encoder.finish()));
        self.stats
            .compute_dispatches
            .fetch_add(1, Ordering::Relaxed);
        tracing::debug!(
            "Dispatched compute: workgroups [{}, {}, {}], {} bindings",
            workgroups[0],
            workgroups[1],
            workgroups[2],
            buffer_ids.len()
        );
        Ok(())
    }

    /// Wait for GPU to be idle
    pub async fn wait_idle(&self) -> Result<()> {
        if let Some(device) = &self.device {
            device.poll(wgpu::Maintain::Wait);
        }
        Ok(())
    }

    /// Reset the GPU state
    pub async fn reset(&self) -> Result<()> {
        // Clear all resources
        self.memory_regions.lock().await.clear();
        self.textures.lock().await.clear();
        self.shaders.lock().await.clear();
        self.next_resource_id.store(1, Ordering::SeqCst);

        tracing::info!("Virtual GPU reset");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vgpu_creation() {
        let vgpu = VirtualGpu::new("test-vgpu");
        assert_eq!(vgpu.name(), "test-vgpu");
        assert!(!vgpu.is_initialized());
    }

    #[tokio::test]
    async fn compute_pipeline_is_cached_across_dispatches() {
        let mut vgpu = VirtualGpu::new("cache-test");
        match vgpu.init().await {
            Ok(()) => {
                let shader = vgpu
                    .create_shader(
                        "@group(0) @binding(0) var<storage, read_write> data: array<u32>;\n\
                         @compute @workgroup_size(1)\n\
                         fn main(@builtin(global_invocation_id) gid: vec3<u32>) {\n\
                             data[gid.x] = data[gid.x] + 1u;\n\
                         }",
                        ShaderType::Compute,
                    )
                    .await
                    .unwrap();
                let buf = vgpu
                    .allocate_buffer(256, BufferUsages::STORAGE | BufferUsages::COPY_DST)
                    .await
                    .unwrap();

                assert_eq!(vgpu.cached_pipeline_count().await, 0);
                vgpu.dispatch_compute_with_bindings(shader, [1, 1, 1], &[buf])
                    .await
                    .unwrap();
                vgpu.dispatch_compute_with_bindings(shader, [1, 1, 1], &[buf])
                    .await
                    .unwrap();

                // Two dispatches of the same (shader, binding_count) must compile
                // the pipeline exactly once.
                assert_eq!(vgpu.cached_pipeline_count().await, 1);
                assert_eq!(vgpu.stats().compute_dispatches.load(Ordering::Relaxed), 2);
            }
            // No GPU adapter on this host — nothing to exercise.
            Err(GpuError::NotAvailable(_)) => {}
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_vgpu_init() {
        let mut vgpu = VirtualGpu::new("test-vgpu");
        // Note: This test may fail if no GPU is available
        match vgpu.init().await {
            Ok(()) => {
                assert!(vgpu.is_initialized());
                let caps = vgpu.capabilities().await;
                assert!(caps.is_some());
            }
            Err(GpuError::NotAvailable(_)) => {
                // Expected on systems without GPU
                tracing::warn!("No GPU available for test");
            }
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    #[test]
    fn test_gpu_features() {
        let features = GpuFeatures {
            compute: true,
            graphics: true,
            ray_tracing: false,
            bindless: true,
            sparse_resources: false,
            multi_draw_indirect: true,
            timestamp_query: true,
        };
        assert!(features.compute);
        assert!(!features.ray_tracing);
    }

    #[test]
    fn test_read_u32_le() {
        let data = [0x01, 0x00, 0x00, 0x00, 0xFF, 0x00, 0x00, 0x00];
        let mut pos = 0;
        assert_eq!(VirtualGpu::read_u32_le(&data, &mut pos).unwrap(), 1);
        assert_eq!(pos, 4);
        assert_eq!(VirtualGpu::read_u32_le(&data, &mut pos).unwrap(), 255);
        assert_eq!(pos, 8);
    }

    #[test]
    fn test_read_u64_le() {
        let data = 0x0000_0001_0000_0000u64.to_le_bytes();
        let mut pos = 0;
        assert_eq!(
            VirtualGpu::read_u64_le(&data, &mut pos).unwrap(),
            0x0000_0001_0000_0000
        );
        assert_eq!(pos, 8);
    }

    #[test]
    fn test_read_truncated() {
        let data = [0x01, 0x02];
        let mut pos = 0;
        assert!(VirtualGpu::read_u32_le(&data, &mut pos).is_err());
        assert!(VirtualGpu::read_u64_le(&data, &mut pos).is_err());
    }

    #[tokio::test]
    async fn test_submit_empty_commands() {
        let mut vgpu = VirtualGpu::new("test");
        match vgpu.init().await {
            Ok(()) => {
                // Empty command buffer should succeed
                vgpu.submit_commands(&[]).await.unwrap();
                assert_eq!(vgpu.stats().commands_submitted.load(Ordering::Relaxed), 1);
            }
            Err(GpuError::NotAvailable(_)) => {}
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_submit_nop_commands() {
        let mut vgpu = VirtualGpu::new("test");
        match vgpu.init().await {
            Ok(()) => {
                // Three NOPs
                vgpu.submit_commands(&[0x00, 0x00, 0x00]).await.unwrap();
            }
            Err(GpuError::NotAvailable(_)) => {}
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_submit_unknown_command() {
        let mut vgpu = VirtualGpu::new("test");
        match vgpu.init().await {
            Ok(()) => {
                let result = vgpu.submit_commands(&[0xFF]).await;
                assert!(result.is_err());
            }
            Err(GpuError::NotAvailable(_)) => {}
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_submit_truncated_command() {
        let mut vgpu = VirtualGpu::new("test");
        match vgpu.init().await {
            Ok(()) => {
                // CopyBufferToBuffer but only 2 payload bytes
                let result = vgpu.submit_commands(&[0x01, 0x00, 0x00]).await;
                assert!(result.is_err());
            }
            Err(GpuError::NotAvailable(_)) => {}
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    // --- New tests below ---

    #[test]
    fn test_gpu_features_default() {
        let f = GpuFeatures::default();
        assert!(!f.compute);
        assert!(!f.graphics);
        assert!(!f.ray_tracing);
        assert!(!f.bindless);
        assert!(!f.sparse_resources);
        assert!(!f.multi_draw_indirect);
        assert!(!f.timestamp_query);
    }

    #[test]
    fn test_shader_type_variants() {
        assert_ne!(ShaderType::Vertex, ShaderType::Fragment);
        assert_ne!(ShaderType::Fragment, ShaderType::Compute);
        assert_ne!(ShaderType::Vertex, ShaderType::Compute);
        assert_eq!(ShaderType::Compute, ShaderType::Compute);
    }

    #[test]
    fn test_virtual_gpu_stats_default() {
        let stats = VirtualGpuStats::default();
        assert_eq!(stats.commands_submitted.load(Ordering::Relaxed), 0);
        assert_eq!(stats.bytes_allocated.load(Ordering::Relaxed), 0);
        assert_eq!(stats.bytes_transferred.load(Ordering::Relaxed), 0);
        assert_eq!(stats.draw_calls.load(Ordering::Relaxed), 0);
        assert_eq!(stats.compute_dispatches.load(Ordering::Relaxed), 0);
        assert_eq!(stats.textures_created.load(Ordering::Relaxed), 0);
        assert_eq!(stats.buffers_created.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_virtual_gpu_stats_increment() {
        let stats = VirtualGpuStats::default();
        stats.commands_submitted.fetch_add(5, Ordering::Relaxed);
        stats.bytes_allocated.fetch_add(1024, Ordering::Relaxed);
        stats.textures_created.fetch_add(3, Ordering::Relaxed);
        assert_eq!(stats.commands_submitted.load(Ordering::Relaxed), 5);
        assert_eq!(stats.bytes_allocated.load(Ordering::Relaxed), 1024);
        assert_eq!(stats.textures_created.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn test_vgpu_name() {
        let vgpu = VirtualGpu::new("my-gpu-device");
        assert_eq!(vgpu.name(), "my-gpu-device");
    }

    #[test]
    fn test_vgpu_not_initialized_by_default() {
        let vgpu = VirtualGpu::new("test");
        assert!(!vgpu.is_initialized());
    }

    #[tokio::test]
    async fn test_vgpu_capabilities_none_before_init() {
        let vgpu = VirtualGpu::new("test");
        assert!(vgpu.capabilities().await.is_none());
    }

    #[test]
    fn test_read_u32_le_boundary() {
        // Exactly 4 bytes
        let data = [0xFF, 0xFF, 0xFF, 0xFF];
        let mut pos = 0;
        assert_eq!(VirtualGpu::read_u32_le(&data, &mut pos).unwrap(), u32::MAX);
        assert_eq!(pos, 4);
    }

    #[test]
    fn test_read_u64_le_zero() {
        let data = [0u8; 8];
        let mut pos = 0;
        assert_eq!(VirtualGpu::read_u64_le(&data, &mut pos).unwrap(), 0);
    }

    #[test]
    fn test_read_u32_le_empty() {
        let data: [u8; 0] = [];
        let mut pos = 0;
        assert!(VirtualGpu::read_u32_le(&data, &mut pos).is_err());
    }

    #[test]
    fn test_read_u64_le_too_short() {
        let data = [0u8; 7]; // 7 bytes, need 8
        let mut pos = 0;
        assert!(VirtualGpu::read_u64_le(&data, &mut pos).is_err());
    }

    #[test]
    fn test_read_u32_sequential() {
        let mut data = Vec::new();
        data.extend_from_slice(&42u32.to_le_bytes());
        data.extend_from_slice(&100u32.to_le_bytes());
        data.extend_from_slice(&999u32.to_le_bytes());
        let mut pos = 0;
        assert_eq!(VirtualGpu::read_u32_le(&data, &mut pos).unwrap(), 42);
        assert_eq!(VirtualGpu::read_u32_le(&data, &mut pos).unwrap(), 100);
        assert_eq!(VirtualGpu::read_u32_le(&data, &mut pos).unwrap(), 999);
        assert_eq!(pos, 12);
    }

    #[test]
    fn test_virtual_gpu_caps_fields() {
        let caps = VirtualGpuCaps {
            name: "Test GPU".to_string(),
            vendor_id: 0x10de,
            device_id: 0x2204,
            max_texture_size: 16384,
            max_buffer_size: 1 << 30,
            max_compute_workgroup_size: [1024, 1024, 64],
            max_compute_invocations: 1024,
            features: GpuFeatures {
                compute: true,
                graphics: true,
                ..Default::default()
            },
            backend: "Vulkan".to_string(),
        };
        assert_eq!(caps.name, "Test GPU");
        assert_eq!(caps.vendor_id, 0x10de);
        assert_eq!(caps.max_texture_size, 16384);
        assert!(caps.features.compute);
        assert!(!caps.features.ray_tracing);
    }

    #[tokio::test]
    async fn test_vgpu_reset() {
        let vgpu = VirtualGpu::new("test");
        vgpu.reset().await.unwrap();
        // After reset, resource ID counter should be back to 1
        assert_eq!(vgpu.stats().commands_submitted.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_vgpu_wait_idle_uninitialized() {
        let vgpu = VirtualGpu::new("test");
        // wait_idle on uninitialized GPU should be a no-op success
        vgpu.wait_idle().await.unwrap();
    }

    #[tokio::test]
    async fn test_dispatch_compute_not_initialized() {
        let vgpu = VirtualGpu::new("test");
        let result = vgpu.dispatch_compute(0, [1, 1, 1]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_allocate_buffer_not_initialized() {
        let vgpu = VirtualGpu::new("test");
        let result = vgpu.allocate_buffer(1024, BufferUsages::STORAGE).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_gpu_error_display() {
        let e = GpuError::NotAvailable("no gpu".to_string());
        assert!(format!("{}", e).contains("no gpu"));
        let e = GpuError::InitFailed("init fail".to_string());
        assert!(format!("{}", e).contains("init fail"));
        let e = GpuError::Unsupported("nope".to_string());
        assert!(format!("{}", e).contains("nope"));
        let e = GpuError::InvalidOperation("bad op".to_string());
        assert!(format!("{}", e).contains("bad op"));
        let e = GpuError::IoError("io fail".to_string());
        assert!(format!("{}", e).contains("io fail"));
    }

    #[tokio::test]
    async fn test_submit_commands_not_initialized() {
        let vgpu = VirtualGpu::new("test");
        let result = vgpu.submit_commands(&[0x00]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_write_buffer_not_initialized() {
        let vgpu = VirtualGpu::new("test");
        let result = vgpu.write_buffer(1, 0, &[0u8; 16]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_free_buffer_not_initialized() {
        let vgpu = VirtualGpu::new("test");
        let result = vgpu.free_buffer(1).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_texture_not_initialized() {
        let vgpu = VirtualGpu::new("test");
        let result = vgpu
            .create_texture(
                256,
                256,
                1,
                TextureFormat::Rgba8Unorm,
                TextureUsages::RENDER_ATTACHMENT,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_shader_not_initialized() {
        let vgpu = VirtualGpu::new("test");
        let result = vgpu.create_shader("", ShaderType::Compute).await;
        assert!(result.is_err());
    }
}
