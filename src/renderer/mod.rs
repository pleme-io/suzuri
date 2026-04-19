pub mod atlas;

use std::sync::Arc;

use cosmic_text::{
    Attrs, Buffer, Color as CosmicColor, FontSystem, Metrics, Shaping, SwashCache,
};
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::config::{ColorScheme, Config};
use crate::errors::{Result, SuzuriError};
use crate::terminal::Terminal;
use crate::terminal::cell::Color;

/// Vertex for the cell background quad.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
}

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x4,
    ];

    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

/// GPU-accelerated terminal renderer using wgpu.
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    bg_pipeline: wgpu::RenderPipeline,
    /// Font system for text rendering.
    font_system: FontSystem,
    swash_cache: SwashCache,
    /// Cell dimensions.
    cell_width: f32,
    cell_height: f32,
    /// Padding.
    padding: f32,
    /// Current window size.
    pub width: u32,
    pub height: u32,
    /// Color scheme.
    colors: ColorScheme,
    /// Font config.
    font_size: f32,
    font_family: String,
}

impl Renderer {
    pub async fn new(window: Arc<Window>, config: &Config) -> Result<Self> {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL | wgpu::Backends::VULKAN,
            ..Default::default()
        });

        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| SuzuriError::Renderer(e.to_string()))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| SuzuriError::Renderer("No suitable GPU adapter found".into()))?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("suzuri-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
            }, None)
            .await
            .map_err(|e| SuzuriError::Renderer(e.to_string()))?;

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        // Create background quad pipeline
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bg-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../assets/bg.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bg-pipeline-layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        let bg_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("bg-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Vertex::desc()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let font_system = FontSystem::new();
        let swash_cache = SwashCache::new();

        let cell_width = config.font.size * 0.6;
        let cell_height = config.font.size * config.font.line_height;

        Ok(Self {
            surface,
            device,
            queue,
            config: surface_config,
            bg_pipeline,
            font_system,
            swash_cache,
            cell_width,
            cell_height,
            padding: config.window.padding as f32,
            width: size.width,
            height: size.height,
            colors: config.colors.clone(),
            font_size: config.font.size,
            font_family: config.font.family.clone(),
        })
    }

    /// Resize the surface when the window changes size.
    pub fn resize(&mut self, new_width: u32, new_height: u32) {
        if new_width > 0 && new_height > 0 {
            self.width = new_width;
            self.height = new_height;
            self.config.width = new_width;
            self.config.height = new_height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    /// Calculate grid dimensions from current window size.
    pub fn grid_size(&self) -> (usize, usize) {
        let usable_width = (self.width as f32 - self.padding * 2.0).max(0.0);
        let usable_height = (self.height as f32 - self.padding * 2.0).max(0.0);
        let cols = (usable_width / self.cell_width).floor() as usize;
        let rows = (usable_height / self.cell_height).floor() as usize;
        (cols.max(1), rows.max(1))
    }

    /// Render the terminal state to the screen.
    pub fn render(&mut self, terminal: &Terminal) -> Result<()> {
        let output = self
            .surface
            .get_current_texture()
            .map_err(|e| SuzuriError::Renderer(e.to_string()))?;

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Build background vertices
        let mut vertices: Vec<Vertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        let bg_color = self.resolve_rgb(Color::Default, true);

        for row in 0..terminal.rows {
            for col in 0..terminal.cols {
                let cell = terminal.cell(row, col);

                // Resolve cell background color
                let cell_bg = if cell.attrs.inverse {
                    self.resolve_rgb(cell.fg, false)
                } else {
                    self.resolve_rgb(cell.bg, true)
                };

                // Only emit a quad if the cell bg differs from the default
                if cell_bg != bg_color || cell.attrs.inverse {
                    let x = self.padding + col as f32 * self.cell_width;
                    let y = self.padding + row as f32 * self.cell_height;
                    self.push_quad(
                        &mut vertices,
                        &mut indices,
                        x,
                        y,
                        self.cell_width,
                        self.cell_height,
                        cell_bg,
                    );
                }

                // Cursor quad
                if row == terminal.cursor_row && col == terminal.cursor_col {
                    let x = self.padding + col as f32 * self.cell_width;
                    let y = self.padding + row as f32 * self.cell_height;
                    let cursor_color = self.color_to_f32(self.colors.cursor);
                    self.push_quad(
                        &mut vertices,
                        &mut indices,
                        x,
                        y,
                        self.cell_width,
                        self.cell_height,
                        [cursor_color[0], cursor_color[1], cursor_color[2], 0.5],
                    );
                }
            }
        }

        // Build text buffer using cosmic-text
        let metrics = Metrics::new(self.font_size, self.font_size * 1.2);
        let mut text_buffer = Buffer::new(&mut self.font_system, metrics);
        let buffer_width = self.width as f32 - self.padding * 2.0;
        let buffer_height = self.height as f32 - self.padding * 2.0;
        text_buffer.set_size(
            &mut self.font_system,
            Some(buffer_width),
            Some(buffer_height),
        );

        // Build text content from terminal grid
        let mut text_lines: Vec<String> = Vec::new();
        for row in 0..terminal.rows {
            let mut line = String::new();
            for col in 0..terminal.cols {
                line.push(terminal.cell(row, col).ch);
            }
            // Trim trailing spaces for cleaner rendering
            let trimmed = line.trim_end();
            text_lines.push(trimmed.to_string());
        }
        let full_text = text_lines.join("\n");

        let attrs = Attrs::new().family(cosmic_text::Family::Name(&self.font_family));
        text_buffer.set_text(&mut self.font_system, &full_text, attrs, Shaping::Advanced);
        text_buffer.shape_until_scroll(&mut self.font_system, false);

        // Create a pixel buffer for text rendering
        let tex_width = self.width;
        let tex_height = self.height;
        let mut pixels = vec![0u8; (tex_width * tex_height * 4) as usize];

        // Fill with background color
        let [br, bg_g, bb] = self.colors.background;
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[0] = br;
            pixel[1] = bg_g;
            pixel[2] = bb;
            pixel[3] = 255;
        }

        // Draw text glyphs
        let fg = self.colors.foreground;
        let text_color = CosmicColor::rgb(fg[0], fg[1], fg[2]);

        text_buffer.draw(
            &mut self.font_system,
            &mut self.swash_cache,
            text_color,
            |x, y, w, h, color| {
                let px = x + self.padding as i32;
                let py = y + self.padding as i32;
                if px >= 0 && py >= 0 {
                    let px = px as u32;
                    let py = py as u32;
                    for dy in 0..h {
                        for dx in 0..w {
                            let fx = px + dx;
                            let fy = py + dy;
                            if fx < tex_width && fy < tex_height {
                                let idx = ((fy * tex_width + fx) * 4) as usize;
                                let a = color.a() as f32 / 255.0;
                                pixels[idx] =
                                    blend(pixels[idx], color.r(), a);
                                pixels[idx + 1] =
                                    blend(pixels[idx + 1], color.g(), a);
                                pixels[idx + 2] =
                                    blend(pixels[idx + 2], color.b(), a);
                                pixels[idx + 3] = 255;
                            }
                        }
                    }
                }
            },
        );

        // Upload pixel buffer as texture
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("text-texture"),
            size: wgpu::Extent3d {
                width: tex_width,
                height: tex_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * tex_width),
                rows_per_image: Some(tex_height),
            },
            wgpu::Extent3d {
                width: tex_width,
                height: tex_height,
                depth_or_array_layers: 1,
            },
        );

        // For now, use a simple fullscreen blit approach
        // Create bind group for the text texture
        let tex_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let blit_bind_group_layout =
            self.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("blit-bind-group-layout"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                    ],
                });

        let blit_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit-bind-group"),
            layout: &blit_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        // Create blit pipeline
        let blit_shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../assets/blit.wgsl").into()),
        });

        let blit_pipeline_layout =
            self.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("blit-pipeline-layout"),
                    bind_group_layouts: &[&blit_bind_group_layout],
                    push_constant_ranges: &[],
                });

        let blit_pipeline =
            self.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("blit-pipeline"),
                    layout: Some(&blit_pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &blit_shader,
                        entry_point: Some("vs_main"),
                        buffers: &[],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &blit_shader,
                        entry_point: Some("fs_main"),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: self.config.format,
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    }),
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                    cache: None,
                });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render-encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blit-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: self.colors.background[0] as f64 / 255.0,
                            g: self.colors.background[1] as f64 / 255.0,
                            b: self.colors.background[2] as f64 / 255.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Draw fullscreen text blit
            render_pass.set_pipeline(&blit_pipeline);
            render_pass.set_bind_group(0, &blit_bind_group, &[]);
            render_pass.draw(0..6, 0..1);

            // Draw cell background quads on top (for colored cells)
            if !vertices.is_empty() {
                let vertex_buffer =
                    self.device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("bg-vertex-buffer"),
                            contents: bytemuck::cast_slice(&vertices),
                            usage: wgpu::BufferUsages::VERTEX,
                        });
                let index_buffer =
                    self.device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("bg-index-buffer"),
                            contents: bytemuck::cast_slice(&indices),
                            usage: wgpu::BufferUsages::INDEX,
                        });

                render_pass.set_pipeline(&self.bg_pipeline);
                render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                render_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..indices.len() as u32, 0, 0..1);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    /// Push a colored quad into the vertex/index buffers.
    fn push_quad(
        &self,
        vertices: &mut Vec<Vertex>,
        indices: &mut Vec<u32>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: [f32; 4],
    ) {
        // Convert pixel coordinates to NDC (-1..1)
        let ndc_x = (x / self.width as f32) * 2.0 - 1.0;
        let ndc_y = 1.0 - (y / self.height as f32) * 2.0;
        let ndc_w = (w / self.width as f32) * 2.0;
        let ndc_h = (h / self.height as f32) * 2.0;

        let base = vertices.len() as u32;
        vertices.extend_from_slice(&[
            Vertex {
                position: [ndc_x, ndc_y],
                color,
            },
            Vertex {
                position: [ndc_x + ndc_w, ndc_y],
                color,
            },
            Vertex {
                position: [ndc_x + ndc_w, ndc_y - ndc_h],
                color,
            },
            Vertex {
                position: [ndc_x, ndc_y - ndc_h],
                color,
            },
        ]);
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Resolve a terminal Color to [f32; 4] RGBA.
    fn resolve_rgb(&self, color: Color, is_bg: bool) -> [f32; 4] {
        match color {
            Color::Default => {
                let c = if is_bg {
                    self.colors.background
                } else {
                    self.colors.foreground
                };
                self.color_to_f32(c)
            }
            Color::Indexed(idx) => {
                let c = self.colors.ansi_color(idx);
                self.color_to_f32(c)
            }
            Color::Rgb(r, g, b) => [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0],
        }
    }

    fn color_to_f32(&self, c: [u8; 3]) -> [f32; 4] {
        [
            c[0] as f32 / 255.0,
            c[1] as f32 / 255.0,
            c[2] as f32 / 255.0,
            1.0,
        ]
    }

    /// Update configuration (e.g. after hot-reload).
    pub fn update_config(&mut self, config: &Config) {
        self.colors = config.colors.clone();
        self.font_size = config.font.size;
        self.font_family = config.font.family.clone();
        self.cell_width = config.font.size * 0.6;
        self.cell_height = config.font.size * config.font.line_height;
        self.padding = config.window.padding as f32;
    }
}

/// Alpha-blend a foreground value onto a background value.
fn blend(bg: u8, fg: u8, alpha: f32) -> u8 {
    ((bg as f32 * (1.0 - alpha)) + (fg as f32 * alpha)) as u8
}
