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

/// Shader types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderType {
    Vertex,
    Fragment,
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
    pub compute: bool,
    pub graphics: bool,
    pub ray_tracing: bool,
    pub bindless: bool,
    pub sparse_resources: bool,
    pub multi_draw_indirect: bool,
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
    /// Next resource ID
    next_resource_id: AtomicU32,
    /// Statistics
    stats: Arc<VirtualGpuStats>,
    /// Initialized
    initialized: std::sync::atomic::AtomicBool,
}

impl VirtualGpu {
    /// Create a new virtual GPU
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

    /// Submit a command buffer
    pub async fn submit_commands(&self, _commands: &[u8]) -> Result<()> {
        let device = self
            .device
            .as_ref()
            .ok_or_else(|| GpuError::NotAvailable("Device not initialized".into()))?;
        let queue = self
            .queue
            .as_ref()
            .ok_or_else(|| GpuError::NotAvailable("Queue not initialized".into()))?;

        // In a real implementation, we would decode the command buffer
        // and execute the GPU commands. For now, we create a simple encoder.
        let encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("vgpu_command_encoder"),
        });

        // Submit the command buffer
        queue.submit(std::iter::once(encoder.finish()));

        self.stats
            .commands_submitted
            .fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Execute a compute dispatch
    pub async fn dispatch_compute(&self, shader_id: u32, workgroups: [u32; 3]) -> Result<()> {
        let device = self
            .device
            .as_ref()
            .ok_or_else(|| GpuError::NotAvailable("Device not initialized".into()))?;
        let queue = self
            .queue
            .as_ref()
            .ok_or_else(|| GpuError::NotAvailable("Queue not initialized".into()))?;

        let shaders = self.shaders.lock().await;
        let shader = shaders
            .get(&shader_id)
            .ok_or_else(|| GpuError::NotAvailable(format!("Shader {} not found", shader_id)))?;

        if shader.shader_type != ShaderType::Compute {
            return Err(GpuError::Unsupported(
                "Shader is not a compute shader".into(),
            ));
        }

        // Create compute pipeline
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("vgpu_compute_pipeline"),
            layout: None,
            module: &shader.module,
            entry_point: "main",
            compilation_options: Default::default(),
            cache: None,
        });

        // Create command encoder and dispatch
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("vgpu_compute_encoder"),
        });

        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("vgpu_compute_pass"),
                timestamp_writes: None,
            });
            compute_pass.set_pipeline(&pipeline);
            compute_pass.dispatch_workgroups(workgroups[0], workgroups[1], workgroups[2]);
        }

        queue.submit(std::iter::once(encoder.finish()));

        self.stats
            .compute_dispatches
            .fetch_add(1, Ordering::Relaxed);
        tracing::debug!(
            "Dispatched compute: workgroups [{}, {}, {}]",
            workgroups[0],
            workgroups[1],
            workgroups[2]
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
}
