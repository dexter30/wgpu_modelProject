use bytemuck::{Pod, Zeroable};
use image::GenericImageView;
use std::f32::consts;
use wgpu::util::DeviceExt;
use std::fs;
use std::path::Path;
use std::rc::Rc;

use gltf::Gltf;
use wgpu::util::{BufferInitDescriptor};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    _pos: [f32; 4],
    _tex_coord: [f32; 2],
}
pub struct GltfMesh {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub index_count: u32,
    pub center: [f32; 3],
    pub base_color_texture: Option<Vec<u8>>,
    pub base_color_mime_type: Option<String>,
}

pub fn init_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    image_bytes: &[u8],
) -> (wgpu::Texture, wgpu::TextureView, wgpu::Sampler) {
    let img = image::load_from_memory(image_bytes).expect("Failed to decode image");
    let rgba = img.to_rgba8();
    let dimensions = img.dimensions();

    let size = wgpu::Extent3d {
        width: dimensions.0,
        height: dimensions.1,
        depth_or_array_layers: 1,
    };

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("GLTF Texture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    queue.write_texture(
        texture.as_image_copy(),
        &rgba,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(4 * dimensions.0),
            rows_per_image: Some(dimensions.1),
        },
        size,
    );

    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("GLTF Sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });

    (texture, texture_view, sampler)
}


pub fn load_gltf<P: AsRef<Path>>(
    path: P,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> anyhow::Result<GltfMesh> {
    let path = path.as_ref();
    let (gltf, buffers, images) = gltf::import(path)?;

    let mesh = gltf.meshes().next().ok_or_else(|| anyhow::anyhow!("No mesh found"))?;
    let primitive = mesh.primitives().next().ok_or_else(|| anyhow::anyhow!("No primitive found"))?;
    let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));


    let positions: Vec<[f32; 3]> = reader.read_positions().ok_or_else(|| anyhow::anyhow!("No positions"))?.collect();
    let tex_coords: Vec<[f32; 2]> = reader.read_tex_coords(0).ok_or_else(|| anyhow::anyhow!("No tex_coords"))?.into_f32().collect();
    let indices: Vec<u32> = reader.read_indices().ok_or_else(|| anyhow::anyhow!("No indices"))?.into_u32().collect();

    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for pos in &positions {
        for i in 0..3 {
            min[i] = min[i].min(pos[i]);
            max[i] = max[i].max(pos[i]);
        }
    }
    let center = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ];
    
    // Zip position and tex_coord into Vertex structs
    let vertices: Vec<Vertex> = positions
        .into_iter()
        .zip(tex_coords.into_iter())
        .map(|(pos, uv)| Vertex {
            _pos: [pos[0], pos[1], pos[2], 1.0],
            _tex_coord: uv,
        })
        .collect();

    let vertex_buffer = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("GLTF Vertex Buffer"),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });

    let index_buffer = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("GLTF Index Buffer"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });


    let mut base_color_texture = None;
    let mut base_color_mime_type = None;

    if let Some(material) = gltf.materials().next() {
        if let Some(info) = material.pbr_metallic_roughness().base_color_texture() {
            let texture = info.texture();
            let image = texture.source();

            match image.source() {
                gltf::image::Source::Uri { uri, .. } => {
                    let texture_path = path.parent().unwrap_or_else(|| Path::new(".")).join(uri);
                    let bytes = std::fs::read(&texture_path)?;
                    println!("[INFO] Loaded texture: {} ({} bytes)", texture_path.display(), bytes.len());
                    let mime_type = mime_guess::from_path(&texture_path).first_or_octet_stream().to_string();
                    base_color_texture = Some(bytes);
                    base_color_mime_type = Some(mime_type);
                }
                gltf::image::Source::View { .. } => {
                    println!("[WARN] Embedded image views in .glb not supported yet.");
                }
            }
        }
    }

    Ok(GltfMesh {
        vertex_buffer,
        index_buffer,
        index_count: indices.len() as u32,
        center,
        base_color_texture,
        base_color_mime_type,
    })
}

fn vertex(pos: [i8; 3], tc: [i8; 2]) -> Vertex {
    Vertex {
        _pos: [pos[0] as f32, pos[1] as f32, pos[2] as f32, 1.0],
        _tex_coord: [tc[0] as f32, tc[1] as f32],
    }
}

fn create_vertices() -> (Vec<Vertex>, Vec<u16>) {
    let vertex_data = [
        // top (0, 0, 1)
        vertex([-1, -1, 1], [0, 0]),
        vertex([1, -1, 1], [1, 0]),
        vertex([1, 1, 1], [1, 1]),
        vertex([-1, 1, 1], [0, 1]),
        // bottom (0, 0, -1)
        vertex([-1, 1, -1], [1, 0]),
        vertex([1, 1, -1], [0, 0]),
        vertex([1, -1, -1], [0, 1]),
        vertex([-1, -1, -1], [1, 1]),
        // right (1, 0, 0)
        vertex([1, -1, -1], [0, 0]),
        vertex([1, 1, -1], [1, 0]),
        vertex([1, 1, 1], [1, 1]),
        vertex([1, -1, 1], [0, 1]),
        // left (-1, 0, 0)
        vertex([-1, -1, 1], [1, 0]),
        vertex([-1, 1, 1], [0, 0]),
        vertex([-1, 1, -1], [0, 1]),
        vertex([-1, -1, -1], [1, 1]),
        // front (0, 1, 0)
        vertex([1, 1, -1], [1, 0]),
        vertex([-1, 1, -1], [0, 0]),
        vertex([-1, 1, 1], [0, 1]),
        vertex([1, 1, 1], [1, 1]),
        // back (0, -1, 0)
        vertex([1, -1, 1], [0, 0]),
        vertex([-1, -1, 1], [1, 0]),
        vertex([-1, -1, -1], [1, 1]),
        vertex([1, -1, -1], [0, 1]),
    ];

    let index_data: &[u16] = &[
        0, 1, 2, 2, 3, 0, // top
        4, 5, 6, 6, 7, 4, // bottom
        8, 9, 10, 10, 11, 8, // right
        12, 13, 14, 14, 15, 12, // left
        16, 17, 18, 18, 19, 16, // front
        20, 21, 22, 22, 23, 20, // back
    ];

    (vertex_data.to_vec(), index_data.to_vec())
}

fn create_texels(size: usize) -> Vec<u8> {
    (0..size * size)
        .map(|id| {
            // get high five for recognizing this ;)
            let cx = 3.0 * (id % size) as f32 / (size - 1) as f32 - 2.0;
            let cy = 2.0 * (id / size) as f32 / (size - 1) as f32 - 1.0;
            let (mut x, mut y, mut count) = (cx, cy, 0);
            while count < 0xFF && x * x + y * y < 4.0 {
                let old_x = x;
                x = x * x - y * y + cx;
                y = 2.0 * old_x * y + cy;
                count += 1;
            }
            count
        })
        .collect()
}

struct Example {
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    index_count: usize,
    bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,
    pipeline: wgpu::RenderPipeline,
    pipeline_wire: Option<wgpu::RenderPipeline>,
    model_center: glam::Vec3,
    mouse_pressed: bool,
    last_cursor_position: Option<(f64, f64)>,
    eye_offset: glam::Vec3,
    aspect_ratio: f32,
}

impl Example {

    fn generate_matrix(aspect_ratio: f32, center: glam::Vec3, scale: f32, eye_offset: glam::Vec3,) -> glam::Mat4 {
        let projection = glam::Mat4::perspective_rh(consts::PI/3.0, aspect_ratio, 0.1, 100.0);
        //let eye_offset = glam::vec3(50.0, -100.0, 5.0); 
        let view = glam::Mat4::look_at_rh(
            center + eye_offset,
            center,
            glam::Vec3::Z,
        );
        let model = glam::Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2)
            * glam::Mat4::from_translation(-center);

        //let model = rotation;//glam::Mat4::from_translation(-center) ;//* glam::Mat4::from_scale(glam::Vec3::splat(1.0));
        let rotation = glam::Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2);

        projection * view* rotation//* model 
    }
}

impl crate::framework::Example for Example {
    fn optional_features() -> wgpu::Features {
        wgpu::Features::POLYGON_MODE_LINE
    }

    fn init(
        config: &wgpu::SurfaceConfiguration,
        _adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Self {
        // Create the vertex and index buffers
        let model_path = "assets/mia.gltf";
        println!("Current working dir: {:?}", std::env::current_dir());
        let gltf_mesh = load_gltf(model_path, device, queue).expect("Failed to load GLTF model");
        

        let vertex_buf = gltf_mesh.vertex_buffer;
        let index_buf = gltf_mesh.index_buffer;
        let index_count = gltf_mesh.index_count as usize;



        // Create pipeline layout
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Model BindGroupLayout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(64),
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
            label: None,
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        // Create the texture
        let size = 256u32;
        let texels = create_texels(size as usize);
        let texture_extent = wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: texture_extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        queue.write_texture(
            texture.as_image_copy(),
            &texels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size),
                rows_per_image: None,
            },
            texture_extent,
        );

        // Create other resources
        let model_center = glam::Vec3::from_array(gltf_mesh.center);
        let radius = glam::Vec3::from_array([
            gltf_mesh.center[0].abs(),
            gltf_mesh.center[1].abs(),
            gltf_mesh.center[2].abs(),
        ]).length();
        let scale = 1.0 / radius.max(1.0); // avoid divide-by-zero

        let mx_total = Self::generate_matrix(config.width as f32 / config.height as f32, model_center,scale, glam::vec3(0.0, -100.0, 5.0));
        let mx_ref: &[f32; 16] = mx_total.as_ref();
        let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Uniform Buffer"),
            contents: bytemuck::cast_slice(mx_ref),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let (texture_view, sampler) = if let Some(bytes) = &gltf_mesh.base_color_texture {
            let (_, view, sampler) = crate::model_load::init_texture(device, queue, bytes);
            (view, sampler)
        } else {
            let white_pixel = [255u8, 255, 255, 255];
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Fallback Texture"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                texture.as_image_copy(),
                &white_pixel,
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(4),
                    rows_per_image: Some(1),
                },
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
            (view, sampler)
        };
        

        // Create bind group
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
            label: Some("Model Bind Group"),
        });
        

        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));

        let vertex_buffers = [wgpu::VertexBufferLayout {
            array_stride: size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 12,
                    shader_location: 1,
                },
            ],
        }];
        

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &vertex_buffers,
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(config.view_formats[0].into())],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let pipeline_wire = if device
            .features()
            .contains(wgpu::Features::POLYGON_MODE_LINE)
        {
            let pipeline_wire = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &vertex_buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_wire"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.view_formats[0],
                        blend: Some(wgpu::BlendState {
                            color: wgpu::BlendComponent {
                                operation: wgpu::BlendOperation::Add,
                                src_factor: wgpu::BlendFactor::SrcAlpha,
                                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            },
                            alpha: wgpu::BlendComponent::REPLACE,
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: Some(wgpu::Face::Back),
                    polygon_mode: wgpu::PolygonMode::Line,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });
            Some(pipeline_wire)
        } else {
            None
        };
        let aspect_ratio = config.width as f32 / config.height as f32;

        // Done
        Example {
            vertex_buf,
            index_buf,
            index_count,
            bind_group,
            uniform_buf,
            pipeline,
            pipeline_wire,
            model_center,
            mouse_pressed: false,
            last_cursor_position: None,
            eye_offset: glam::vec3(0.0, -100.0, 5.0), // initial eye position like before
            aspect_ratio,
            
        }
    }

    fn update(&mut self, _event: winit::event::WindowEvent) {
        match _event {
            winit::event::WindowEvent::MouseInput { button, state, .. } => {
                if button == winit::event::MouseButton::Right {
                    self.mouse_pressed = state == winit::event::ElementState::Pressed;
                    println!("mouse pressed");
                }
            }
            winit::event::WindowEvent::CursorMoved { position, .. } => {
                if self.mouse_pressed {
                    if let Some((last_x, _last_y)) = self.last_cursor_position {
                        let dx = position.x - last_x;
                        let dy = position.y - _last_y;
                        self.eye_offset.x += dx as f32 * 0.01; // adjust sensitivity here
                        self.eye_offset.z += dy as f32 * 0.25; // adjust sensitivity here
                    }
                    self.last_cursor_position = Some((position.x, position.y));
                }
            }
            _ => {}
        }
    }

    fn resize(
        &mut self,
        config: &wgpu::SurfaceConfiguration,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) {
        let radius = self.model_center.length().max(1.0);
        let scale = 1.0 ;
        self.aspect_ratio = config.width as f32 / config.height as f32;
        let mx_total = Self::generate_matrix(self.aspect_ratio, self.model_center, scale, glam::vec3(0.0, -100.0, 5.0));
        let mx_ref: &[f32; 16] = mx_total.as_ref();
        queue.write_buffer(&self.uniform_buf, 0, bytemuck::cast_slice(mx_ref));
    }

fn render(&mut self, view: &wgpu::TextureView, device: &wgpu::Device, queue: &wgpu::Queue) {
    let mx_total = Self::generate_matrix(self.aspect_ratio, self.model_center, 1.0, self.eye_offset);
    let mx_ref: &[f32; 16] = mx_total.as_ref();
    queue.write_buffer(&self.uniform_buf, 0, bytemuck::cast_slice(mx_ref));
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Render Encoder"),
    });

    {
        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("GLTF Model Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.1,
                        g: 0.2,
                        b: 0.3,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        // Draw main pipeline
        rpass.set_pipeline(&self.pipeline);
        rpass.set_bind_group(0, &self.bind_group, &[]);
        rpass.set_vertex_buffer(0, self.vertex_buf.slice(..));
        rpass.set_index_buffer(self.index_buf.slice(..), wgpu::IndexFormat::Uint32);
        rpass.draw_indexed(0..self.index_count as u32, 0, 0..1);

        // Optional wireframe overlay
        if let Some(ref wire_pipeline) = self.pipeline_wire {
            rpass.set_pipeline(wire_pipeline);
            rpass.draw_indexed(0..self.index_count as u32, 0, 0..1);
        }
    }

    queue.submit(Some(encoder.finish()));
}

}

pub fn main() {
    crate::framework::run::<Example>("model_load");
}

#[cfg(test)]
#[wgpu_test::gpu_test]
static TEST: crate::framework::ExampleTestParams = crate::framework::ExampleTestParams {
    name: "model_load",
    // Generated on 1080ti on Vk/Windows
    image_path: "/examples/features/src/model_load/screenshot.png",
    width: 1024,
    height: 768,
    optional_features: wgpu::Features::default(),
    base_test_parameters: wgpu_test::TestParameters::default(),
    comparisons: &[
        wgpu_test::ComparisonType::Mean(0.04), // Bounded by Intel 630 on Vk/Windows
    ],
    _phantom: std::marker::PhantomData::<Example>,
};

#[cfg(test)]
#[wgpu_test::gpu_test]
static TEST_LINES: crate::framework::ExampleTestParams = crate::framework::ExampleTestParams {
    name: "model_load-lines",
    // Generated on 1080ti on Vk/Windows
    image_path: "/examples/features/src/model_load/screenshot-lines.png",
    width: 1024,
    height: 768,
    optional_features: wgpu::Features::POLYGON_MODE_LINE,
    base_test_parameters: wgpu_test::TestParameters::default(),
    // We're looking for tiny changes here, so we focus on a spike in the 95th percentile.
    comparisons: &[
        wgpu_test::ComparisonType::Mean(0.05), // Bounded by Intel 630 on Vk/Windows
        wgpu_test::ComparisonType::Percentile {
            percentile: 0.95,
            threshold: 0.36,
        }, // Bounded by 1080ti on DX12
    ],
    _phantom: std::marker::PhantomData::<Example>,
};
