//! `WgpuCompositor` — wgpu-backed scene compositor.
//!
//! Lifecycle:
//!   1. `WgpuCompositor::new(width, height)` — picks an adapter, creates
//!      device/queue, render pipeline, sampler. ~50–200 ms one-shot.
//!   2. `upload_texture(id, &image)` — repeat once per scene texture.
//!      Cheap to call again with the same id (no-op).
//!   3. `compose_to_host(&[SpriteDraw])` — paint a frame, read back to RGBA Vec.
//!      Once per video frame in the typical pipeline.
//!
//! Render target is RGBA8 unorm; output bytes are tightly packed
//! (no per-row padding) so callers don't need to walk linesize.

use crate::sprite::{SpriteDraw, UploadedTexture};
use bytemuck::{Pod, Zeroable};
use image::{DynamicImage, GenericImageView};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(thiserror::Error, Debug)]
pub enum CompositorError {
    #[error("no compatible wgpu adapter found")]
    NoAdapter,

    #[error("device request failed: {0}")]
    DeviceRequest(String),

    #[error("texture id '{0}' has not been uploaded")]
    UnknownTexture(String),

    #[error("readback buffer mapping failed: {0}")]
    Readback(String),
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
struct Uniforms {
    scene_size: [f32; 2],
    sprite_size: [f32; 2],
    pos: [f32; 2],
    scale: [f32; 2],
    anchor: [f32; 2],
    rotation_rad: f32,
    opacity: f32,
}

pub struct WgpuCompositor {
    width: u32,
    height: u32,

    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,

    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,

    /// Persistent render target — reused across compose calls.
    target_texture: wgpu::Texture,
    target_view: wgpu::TextureView,

    /// Buffer for reading the target back to host memory. Sized to
    /// padded-bytes-per-row × height once at construction; reused each
    /// compose. wgpu requires bytes-per-row aligned to 256.
    readback_buffer: wgpu::Buffer,
    bytes_per_row_padded: u32,

    /// Per-sprite uniform buffer, reused across draws within one frame.
    /// We use one buffer + dynamic offset; M4-MVP uses one write per
    /// draw which is fine for ≤ ~50 sprites/frame.
    uniform_buffer: wgpu::Buffer,
    uniform_offset_alignment: u64,

    textures: HashMap<String, UploadedTexture>,
}

impl WgpuCompositor {
    pub fn new(width: u32, height: u32) -> Result<Self, CompositorError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN | wgpu::Backends::METAL | wgpu::Backends::DX12,
            ..Default::default()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .ok_or(CompositorError::NoAdapter)?;
        let info = adapter.get_info();
        log::info!(
            "WgpuCompositor adapter: {} (backend {:?}, type {:?})",
            info.name,
            info.backend,
            info.device_type
        );

        let limits = wgpu::Limits::default();
        let uniform_offset_alignment = adapter
            .limits()
            .min_uniform_buffer_offset_alignment as u64;
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("gamereel-compositor"),
                required_features: wgpu::Features::empty(),
                required_limits: limits,
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ))
        .map_err(|e| CompositorError::DeviceRequest(e.to_string()))?;

        // ---- Persistent render target ----
        let target_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("compose-target"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target_texture.create_view(&wgpu::TextureViewDescriptor::default());

        // wgpu requires bytes-per-row aligned to 256 in COPY_BUFFER_TO_TEXTURE
        // and reverse. RGBA8 = 4 bytes/px.
        let bytes_per_row_unpadded = width * 4;
        let bytes_per_row_padded = ((bytes_per_row_unpadded + 255) / 256) * 256;

        let readback_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("compose-readback"),
            size: (bytes_per_row_padded as u64) * (height as u64),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // ---- Pipeline ----
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("composite"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/composite.wgsl").into(),
            ),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("compose-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<Uniforms>() as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("compose-pl"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("compose-pipe"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("compose-smp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // Uniform buffer sized for many sprites with dynamic offset.
        // Cap at 256 sprites/frame for the M4-MVP — ample for hs-mvp.
        const MAX_SPRITES_PER_FRAME: u64 = 256;
        let uniform_stride =
            ((std::mem::size_of::<Uniforms>() as u64 + uniform_offset_alignment - 1) / uniform_offset_alignment)
                * uniform_offset_alignment;
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("compose-uniforms"),
            size: uniform_stride * MAX_SPRITES_PER_FRAME,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Self {
            width,
            height,
            instance,
            adapter,
            device,
            queue,
            pipeline,
            bind_group_layout,
            sampler,
            target_texture,
            target_view,
            readback_buffer,
            bytes_per_row_padded,
            uniform_buffer,
            uniform_offset_alignment: uniform_stride,
            textures: HashMap::new(),
        })
    }

    pub fn width(&self) -> u32 { self.width }
    pub fn height(&self) -> u32 { self.height }

    /// Upload a host RGBA image as a sampled texture. Idempotent on
    /// repeat calls with the same id (newer image replaces older).
    pub fn upload_texture(
        &mut self,
        id: impl Into<String>,
        image: &DynamicImage,
    ) -> Result<(), CompositorError> {
        let id = id.into();
        let rgba = image.to_rgba8();
        let (w, h) = rgba.dimensions();
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("tex:{id}")),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba.as_raw(),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.textures.insert(
            id,
            UploadedTexture {
                width: w,
                height: h,
                view: Arc::new(view),
                texture: Arc::new(texture),
            },
        );
        Ok(())
    }

    /// Compose one frame from the supplied sprite list and read the
    /// result back to a tightly-packed RGBA8 Vec. Order matters:
    /// later sprites paint over earlier ones (back-to-front).
    pub fn compose_to_host(
        &mut self,
        draws: &[SpriteDraw],
    ) -> Result<Vec<u8>, CompositorError> {
        // 1) Pack uniforms for every draw into the persistent buffer.
        for (i, d) in draws.iter().enumerate() {
            let tex = self
                .textures
                .get(&d.texture_id)
                .ok_or_else(|| CompositorError::UnknownTexture(d.texture_id.clone()))?;
            let sprite_size = d.size.unwrap_or([tex.width as f32, tex.height as f32]);
            let u = Uniforms {
                scene_size: [self.width as f32, self.height as f32],
                sprite_size,
                pos: d.pos,
                scale: d.scale,
                anchor: d.anchor,
                rotation_rad: d.rotation_deg.to_radians(),
                opacity: (d.opacity as f32) / 255.0,
            };
            let offset = (i as u64) * self.uniform_offset_alignment;
            self.queue.write_buffer(&self.uniform_buffer, offset, bytemuck::bytes_of(&u));
        }

        // 2) Pre-create per-draw bind groups (need the texture view, so
        //    we can't share a single bind group across draws).
        let mut bind_groups: Vec<wgpu::BindGroup> = Vec::with_capacity(draws.len());
        for d in draws {
            let tex = self.textures.get(&d.texture_id).expect("verified above");
            let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("compose-bg"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &self.uniform_buffer,
                            offset: 0, // dynamic offset supplied at draw time
                            size: wgpu::BufferSize::new(
                                std::mem::size_of::<Uniforms>() as u64,
                            ),
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&tex.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            bind_groups.push(bg);
        }

        // 3) Encode the render pass + readback copy.
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("compose-encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("compose-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            for (i, bg) in bind_groups.iter().enumerate() {
                let dyn_offset = (i as u32) * (self.uniform_offset_alignment as u32);
                pass.set_bind_group(0, bg, &[dyn_offset]);
                pass.draw(0..6, 0..1);
            }
        }

        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &self.target_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &self.readback_buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(self.bytes_per_row_padded),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );

        self.queue.submit([encoder.finish()]);

        // 4) Map + read back. Block via pollster — the worker thread is
        //    happy to block (CUDA/wgpu both happen on dedicated threads).
        let buffer_slice = self.readback_buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });
        self.device.poll(wgpu::Maintain::Wait);
        let map_res = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| CompositorError::Readback(format!("recv timeout: {e}")))?;
        map_res.map_err(|e| CompositorError::Readback(format!("{e}")))?;

        let row_bytes = (self.width * 4) as usize;
        let h = self.height as usize;
        let mut out = Vec::with_capacity(row_bytes * h);
        {
            let data = buffer_slice.get_mapped_range();
            for y in 0..h {
                let off = y * (self.bytes_per_row_padded as usize);
                out.extend_from_slice(&data[off..off + row_bytes]);
            }
        }
        self.readback_buffer.unmap();
        Ok(out)
    }
}
