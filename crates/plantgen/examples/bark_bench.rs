//! How long a GPU takes to draw bark: a close trunk filling the screen,
//! drawn with today's per-pixel bark pattern (as After and `PlantLab` run it)
//! and with the pattern read from textures (the bark tile of
//! `plantgen-claude` `project-files/bark/PROPOSAL-2026-10-19.md`).
//!
//! ```text
//! cargo run --release -p plantgen --features gpu --example bark_bench
//! cargo run --release -p plantgen --features gpu --example bark_bench -- --species pinus-ponderosa
//! ```
//!
//! Each variant draws `--passes` full-screen passes per round over
//! `--rounds` rounds, at 1920 × 1080 and 3840 × 2160, and prints the median
//! milliseconds a pass takes (GPU timestamps where the adapter has them,
//! else the wall clock round a wait for the queue). The textures' contents
//! are the species' pattern sampled on the CPU, so they cache as real bark
//! would; what they hold does not change what a fetch costs. A software
//! adapter (lavapipe) runs only with `PLANTGEN_GPU=any`, and its numbers
//! are a CPU's, not a GPU's.

// A measuring tool: its casts are of sizes and counts far inside each type.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::too_many_lines
)]

use std::future::Future;
use std::sync::{Arc, mpsc};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Instant;

use plantgen::bark::{self, BarkParams};
use plantgen::library;
use plantgen::spec::PlantSpec;
use wgpu::util::DeviceExt;

const SHADER: &str = include_str!("bark_bench.wgsl");
/// Texels along each side of a bark tile.
const TILE: u32 = 1024;
/// Metres of trunk a tile covers before it repeats.
const PERIOD: f32 = 1.0;
/// The trunk's radius, metres: a 0.8 m trunk.
const RADIUS: f32 = 0.4;
/// Metres of trunk the screen shows from top to bottom: a close look.
const SPAN: f32 = 0.7;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

struct Options {
    species: Vec<String>,
    passes: u32,
    rounds: u32,
}

fn options() -> Options {
    let mut options = Options {
        species: Vec::new(),
        passes: 40,
        rounds: 7,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = args.next().unwrap_or_default();
        match arg.as_str() {
            "--species" => options.species.push(value),
            "--passes" => options.passes = value.parse().expect("--passes takes a count"),
            "--rounds" => options.rounds = value.parse().expect("--rounds takes a count"),
            _ => panic!("unknown argument {arg}"),
        }
    }
    if options.species.is_empty() {
        options.species = [
            "pseudotsuga-menziesii",
            "pinus-ponderosa",
            "betula-papyrifera",
        ]
        .map(String::from)
        .to_vec();
    }
    options
}

fn main() {
    let options = options();
    let wish = std::env::var("PLANTGEN_GPU").unwrap_or_default();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .expect("no GPU adapter");
    let info = adapter.get_info();
    assert!(
        info.device_type != wgpu::DeviceType::Cpu || wish == "any",
        "{} is a software adapter; set PLANTGEN_GPU=any to time it anyway",
        info.name
    );
    let timestamps = adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("bark bench"),
        required_features: if timestamps {
            wgpu::Features::TIMESTAMP_QUERY
        } else {
            wgpu::Features::empty()
        },
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("no device");
    println!(
        "adapter: {} ({:?}, {:?}, driver {} {})",
        info.name, info.device_type, info.backend, info.driver, info.driver_info
    );
    println!(
        "timing: {}",
        if timestamps {
            "GPU timestamps"
        } else {
            "wall clock"
        }
    );
    println!(
        "{} passes a round, median of {} rounds; ms per full-screen pass",
        options.passes, options.rounds
    );
    let bench = Bench::new(device, queue, timestamps);
    for id in &options.species {
        let entry = library::species(id).unwrap_or_else(|| panic!("no species {id}"));
        let spec = PlantSpec::from_json(entry.source).expect("its spec");
        let params = spec
            .appearance
            .bark_params()
            .unwrap_or_else(|| panic!("{id} has no bark pattern"));
        println!(
            "\n{id} ({:?})",
            spec.appearance.bark_pattern.as_ref().map(|p| p.kind)
        );
        bench.run(&params, &options);
    }
}

struct Bench {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    module: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    sampler: wgpu::Sampler,
    timestamps: bool,
}

impl Bench {
    fn new(device: wgpu::Device, queue: wgpu::Queue, timestamps: bool) -> Self {
        let uniform = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bark bench"),
            entries: &[
                uniform(0),
                uniform(1),
                texture(2),
                texture(3),
                texture(4),
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bark bench"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bark bench"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("tiles"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        Self {
            device,
            queue,
            layout,
            module,
            pipeline_layout,
            sampler,
            timestamps,
        }
    }

    fn pipeline(&self, entry: &str) -> wgpu::RenderPipeline {
        self.device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&self.pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &self.module,
                    entry_point: Some("vertex"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &self.module,
                    entry_point: Some(entry),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(FORMAT.into())],
                }),
                multiview_mask: None,
                cache: None,
            })
    }

    fn run(&self, params: &BarkParams, options: &Options) {
        let tiles = Tiles::new(&self.device, &self.queue, params);
        let bark_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("bark"),
                contents: &bark_bytes(params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let variants = [
            ("plain", "lighting only, no pattern", &tiles.shade),
            ("procedural", "today: pattern per pixel", &tiles.shade),
            (
                "tile",
                "one RGBA16F tile (shade, inner, slopes)",
                &tiles.shade,
            ),
            (
                "pbr",
                "glTF set: colour + normal + ORM, RGBA8",
                &tiles.colour,
            ),
            ("pbr_two", "glTF set, two stages cross-faded", &tiles.colour),
        ];
        for (width, height) in [(1920_u32, 1080_u32), (3840, 2160)] {
            println!("  {width} x {height}");
            let view_buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("view"),
                    contents: &view_bytes(width, height),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let target = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("target"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let target = target.create_view(&wgpu::TextureViewDescriptor::default());
            let mut plain = None;
            for (entry, what, first) in variants {
                let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(entry),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: bark_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: view_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(first),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(&tiles.normal),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: wgpu::BindingResource::TextureView(&tiles.orm),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                    ],
                });
                let pipeline = self.pipeline(entry);
                let ms = self.time(&pipeline, &bind_group, &target, options);
                let over = plain.map_or(String::new(), |base: f64| {
                    format!("  ({:+.3} ms over plain)", ms - base)
                });
                if entry == "plain" {
                    plain = Some(ms);
                }
                println!("    {entry:<11} {ms:>8.3} ms{over}   {what}");
            }
        }
    }

    /// The median milliseconds one pass of `pipeline` takes.
    fn time(
        &self,
        pipeline: &wgpu::RenderPipeline,
        bind_group: &wgpu::BindGroup,
        target: &wgpu::TextureView,
        options: &Options,
    ) -> f64 {
        let passes = options.passes;
        let queries = self.timestamps.then(|| {
            let set = self.device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("passes"),
                ty: wgpu::QueryType::Timestamp,
                count: 2 * passes,
            });
            let size = u64::from(2 * passes) * 8;
            let resolve = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("resolve"),
                size,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("readback"),
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            (set, resolve, readback, size)
        });
        let period = f64::from(self.queue.get_timestamp_period());
        let mut rounds = Vec::new();
        // One round more than asked: the first warms caches and compiles.
        for round in 0..=options.rounds {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            for pass_index in 0..passes {
                let timestamp_writes =
                    queries
                        .as_ref()
                        .map(|(set, ..)| wgpu::RenderPassTimestampWrites {
                            query_set: set,
                            beginning_of_pass_write_index: Some(2 * pass_index),
                            end_of_pass_write_index: Some(2 * pass_index + 1),
                        });
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            if let Some((set, resolve, readback, size)) = &queries {
                encoder.resolve_query_set(set, 0..2 * passes, resolve, 0);
                encoder.copy_buffer_to_buffer(resolve, 0, readback, 0, *size);
            }
            let start = Instant::now();
            self.queue.submit([encoder.finish()]);
            self.device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("the queue finishes");
            let wall = start.elapsed().as_secs_f64() * 1000.0 / f64::from(passes);
            let ms = match &queries {
                Some((_, _, readback, _)) => {
                    let (sender, receiver) = mpsc::channel();
                    readback.map_async(wgpu::MapMode::Read, .., move |result| {
                        let _ = sender.send(result);
                    });
                    self.device
                        .poll(wgpu::PollType::wait_indefinitely())
                        .expect("the map finishes");
                    receiver.recv().expect("mapped").expect("mapped");
                    let view = readback.get_mapped_range(..);
                    let stamps: Vec<u64> = view
                        .as_chunks::<8>()
                        .0
                        .iter()
                        .map(|word| u64::from_le_bytes(*word))
                        .collect();
                    drop(view);
                    readback.unmap();
                    let total: u64 = stamps
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|[begin, end]| end.saturating_sub(*begin))
                        .sum();
                    total as f64 * period / 1.0e6 / f64::from(passes)
                }
                None => wall,
            };
            if round > 0 {
                rounds.push(ms);
            }
        }
        rounds.sort_by(f64::total_cmp);
        rounds[rounds.len() / 2]
    }
}

/// The bark tile's textures, with their mipmaps: the four numbers of a
/// sample (RGBA16F), and the glTF set (colour, normal, ORM; RGBA8).
struct Tiles {
    shade: wgpu::TextureView,
    colour: wgpu::TextureView,
    normal: wgpu::TextureView,
    orm: wgpu::TextureView,
}

impl Tiles {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, params: &BarkParams) -> Self {
        // The pattern on a stem whose circumference is the tile's period,
        // so its round is one tile wide and every texel is sampled once.
        let radius = PERIOD / std::f32::consts::TAU;
        let texel = PERIOD / TILE as f32;
        let mut samples = Vec::with_capacity((TILE * TILE) as usize);
        for y in 0..TILE {
            for x in 0..TILE {
                let u = (x as f32 + 0.5) / TILE as f32;
                let v = (y as f32 + 0.5) * texel;
                samples.push(bark::sample(params, u, v, radius, texel));
            }
        }
        let half = |value: f32| f16_bits(value);
        let shade: Vec<[u16; 4]> = samples
            .iter()
            .map(|s| {
                [
                    half(s.shade),
                    half(s.inner),
                    half(s.slope[0]),
                    half(s.slope[1]),
                ]
            })
            .collect();
        let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        let colour: Vec<[u8; 4]> = samples
            .iter()
            .map(|s| {
                let c = 0.3 * s.shade;
                [byte(c), byte(c * 0.7), byte(c * 0.5), 255]
            })
            .collect();
        let normal: Vec<[u8; 4]> = samples
            .iter()
            .map(|s| {
                [
                    byte(0.5 + 0.5 * s.slope[0]),
                    byte(0.5 + 0.5 * s.slope[1]),
                    255,
                    255,
                ]
            })
            .collect();
        let orm: Vec<[u8; 4]> = samples
            .iter()
            .map(|s| [byte(s.shade.min(1.0)), byte(0.9), 0, 255])
            .collect();
        Self {
            shade: upload_half(device, queue, &shade),
            colour: upload(device, queue, &colour),
            normal: upload(device, queue, &normal),
            orm: upload(device, queue, &orm),
        }
    }
}

/// A square RGBA8 texture of [`TILE`] texels a side with its box-filtered
/// mipmaps.
fn upload(device: &wgpu::Device, queue: &wgpu::Queue, texels: &[[u8; 4]]) -> wgpu::TextureView {
    let levels = mips(texels.iter().map(|t| t.map(f32::from)).collect());
    let bytes: Vec<Vec<u8>> = levels
        .iter()
        .map(|level| {
            level
                .iter()
                .flat_map(|t| t.map(|c| c.round() as u8))
                .collect()
        })
        .collect();
    texture(device, queue, wgpu::TextureFormat::Rgba8Unorm, 4, &bytes)
}

fn upload_half(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texels: &[[u16; 4]],
) -> wgpu::TextureView {
    let levels = mips(texels.iter().map(|t| t.map(f32_of_half)).collect());
    let bytes: Vec<Vec<u8>> = levels
        .iter()
        .map(|level| {
            level
                .iter()
                .flat_map(|t| t.map(f16_bits))
                .flat_map(u16::to_le_bytes)
                .collect()
        })
        .collect();
    texture(device, queue, wgpu::TextureFormat::Rgba16Float, 8, &bytes)
}

/// Every mip level of a square texture, from the full one down to 1 × 1.
fn mips(full: Vec<[f32; 4]>) -> Vec<Vec<[f32; 4]>> {
    let mut levels = vec![full];
    let mut side = TILE as usize;
    while side > 1 {
        let last = levels.last().expect("a level");
        let next_side = side / 2;
        let mut next = Vec::with_capacity(next_side * next_side);
        for y in 0..next_side {
            for x in 0..next_side {
                let mut sum = [0.0; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let texel = last[(2 * y + dy) * side + 2 * x + dx];
                    for c in 0..4 {
                        sum[c] += texel[c] / 4.0;
                    }
                }
                next.push(sum);
            }
        }
        levels.push(next);
        side = next_side;
    }
    levels
}

fn texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
    bytes_per_texel: u32,
    levels: &[Vec<u8>],
) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tile"),
        size: wgpu::Extent3d {
            width: TILE,
            height: TILE,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, bytes) in levels.iter().enumerate() {
        let side = (TILE >> level).max(1);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(side * bytes_per_texel),
                rows_per_image: Some(side),
            },
            wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 1,
            },
        );
    }
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// The shader's `Bark`, as bytes in its layout.
fn bark_bytes(params: &BarkParams) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(64);
    for value in [params.inner[0], params.inner[1], params.inner[2], 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&params.kind.to_le_bytes());
    for value in [
        params.scale,
        params.elongation,
        params.fissure,
        params.depth,
        params.contrast,
        params.variation,
        params.mean,
        params.smooth_below,
        params.lenticels,
        params.peeled,
        params.roughness,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// The shader's `View`, as bytes in its layout.
fn view_bytes(width: u32, height: u32) -> Vec<u8> {
    [
        width as f32,
        height as f32,
        RADIUS,
        SPAN,
        PERIOD,
        0.0,
        0.0,
        0.0,
    ]
    .iter()
    .flat_map(|value| value.to_le_bytes())
    .collect()
}

/// `value` as an IEEE half, rounded toward zero (enough for a benchmark).
fn f16_bits(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mantissa = bits & 0x007f_ffff;
    if exponent <= 0 {
        sign
    } else if exponent >= 31 {
        sign | 0x7c00
    } else {
        sign | ((exponent as u16) << 10) | ((mantissa >> 13) as u16)
    }
}

fn f32_of_half(bits: u16) -> f32 {
    let sign = u32::from(bits & 0x8000) << 16;
    let exponent = u32::from((bits >> 10) & 0x1f);
    let mantissa = u32::from(bits & 0x3ff);
    if exponent == 0 {
        return f32::from_bits(sign);
    }
    f32::from_bits(sign | ((exponent + 127 - 15) << 23) | (mantissa << 13))
}

/// Run a future to its end on this thread.
fn block_on<F: Future>(future: F) -> F::Output {
    struct Wakes(std::thread::Thread);
    impl Wake for Wakes {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(Wakes(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        std::thread::park();
    }
}
