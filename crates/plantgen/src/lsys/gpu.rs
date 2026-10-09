//! The narrowing of [`super::narrow`] on a GPU, with `wgpu` (feature
//! `gpu`). One device serves the whole process; each narrowing uploads its
//! job, runs the shader `narrow.wgsl` over every point and reads the
//! result back. `PLANTGEN_GPU=off` leaves the GPU unused; a software adapter
//! (lavapipe, llvmpipe) is used only with `PLANTGEN_GPU=any`, since the CPU
//! is faster than one.

use std::future::Future;
use std::sync::{Arc, OnceLock, mpsc};
use std::task::{Context, Poll, Wake, Waker};

use wgpu::util::DeviceExt;

use super::narrow::{Job, STRIDE};

const SHADER: &str = include_str!("narrow.wgsl");
/// Invocations per workgroup, as the shader declares.
const GROUP: u32 = 64;
/// Workgroups along one dimension of a dispatch.
const MOST_GROUPS: u32 = 65_535;

pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    /// The adapter, as `wgpu` describes it.
    pub adapter: String,
}

impl Gpu {
    /// The process's GPU, found on first use; `None` where none answers or
    /// `PLANTGEN_GPU` says not to use one.
    #[must_use]
    pub fn get() -> Option<&'static Self> {
        static GPU: OnceLock<Option<Gpu>> = OnceLock::new();
        GPU.get_or_init(Self::open).as_ref()
    }

    fn open() -> Option<Self> {
        let wish = std::env::var("PLANTGEN_GPU").unwrap_or_default();
        if wish == "off" {
            return None;
        }
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .ok()?;
        let info = adapter.get_info();
        if info.device_type == wgpu::DeviceType::Cpu && wish != "any" {
            return None;
        }
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("plantgen narrowing"),
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .ok()?;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("narrow"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("narrow"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let layout = pipeline.get_bind_group_layout(0);
        let adapter = format!("{info:?}");
        eprintln!("plantgen narrows bud searches on {adapter}");
        Some(Self {
            device,
            queue,
            pipeline,
            layout,
            adapter,
        })
    }

    /// The narrowing of every point of `job`, `STRIDE` values a point, or
    /// `None` if the GPU failed (the CPU then searches in full).
    #[must_use]
    pub fn narrow(&self, job: &Job) -> Option<Vec<u32>> {
        let count = u32::try_from(job.points.len()).ok()?;
        if count == 0 {
            return Some(Vec::new());
        }
        let uniform = uniform(job, count);
        let storage = |label: &str, bytes: Vec<u8>| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: &bytes,
                    usage: wgpu::BufferUsages::STORAGE,
                })
        };
        let settings_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("settings"),
                contents: &uniform,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let buffers = [
            storage("points", floats(&job.points)),
            storage("plant keys", ints(&job.plant_keys)),
            storage("apex keys", ints(&job.apex_keys)),
            storage("plant starts", words(&job.plant.starts)),
            storage("plant items", floats(&job.plant_items)),
            storage("apex starts", words(&job.apices.starts)),
            storage("apex items", floats(&job.apex_items)),
        ];
        let size = u64::from(count) * STRIDE as u64 * 4;
        let narrowed = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("narrowed"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut entries = vec![wgpu::BindGroupEntry {
            binding: 0,
            resource: settings_buffer.as_entire_binding(),
        }];
        for (binding, buffer) in (1..).zip(&buffers) {
            entries.push(wgpu::BindGroupEntry {
                binding,
                resource: buffer.as_entire_binding(),
            });
        }
        entries.push(wgpu::BindGroupEntry {
            binding: 8,
            resource: narrowed.as_entire_binding(),
        });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("narrow"),
            layout: &self.layout,
            entries: &entries,
        });
        let groups = count.div_ceil(GROUP);
        let (x, y) = (groups.min(MOST_GROUPS), groups.div_ceil(MOST_GROUPS));
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("narrow"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("narrow"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(x, y, 1);
        }
        encoder.copy_buffer_to_buffer(&narrowed, 0, &readback, 0, size);
        self.queue.submit([encoder.finish()]);
        let (sender, receiver) = mpsc::channel();
        readback.map_async(wgpu::MapMode::Read, .., move |result| {
            let _ = sender.send(result);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        receiver.recv().ok()?.ok()?;
        let view = readback.get_mapped_range(..);
        let out = view
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| u32::from_le_bytes(*word))
            .collect();
        drop(view);
        readback.unmap();
        Some(out)
    }
}

/// The shader's `Settings`, as bytes in its layout.
fn uniform(job: &Job, count: u32) -> Vec<u8> {
    let settings = job.settings;
    let mut uniform = Vec::with_capacity(64);
    for word in [
        count,
        settings.kill_sq.to_bits(),
        settings.influence_sq.to_bits(),
        settings.cone.to_bits(),
        settings.margin_sq.to_bits(),
        settings.margin_facing.to_bits(),
        settings.near_sq.to_bits(),
        0,
    ] {
        uniform.extend_from_slice(&word.to_le_bytes());
    }
    for dims in [job.plant.dims, job.apices.dims] {
        for value in [dims[0], dims[1], dims[2], 0] {
            uniform.extend_from_slice(&value.to_le_bytes());
        }
    }
    uniform
}

/// Bytes of 32-bit floats; a buffer is never empty.
fn floats(values: &[[f32; 4]]) -> Vec<u8> {
    let mut bytes: Vec<u8> = values
        .iter()
        .flatten()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    if bytes.is_empty() {
        bytes.resize(16, 0);
    }
    bytes
}

fn ints(values: &[[i32; 4]]) -> Vec<u8> {
    let mut bytes: Vec<u8> = values
        .iter()
        .flatten()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    if bytes.is_empty() {
        bytes.resize(16, 0);
    }
    bytes
}

fn words(values: &[u32]) -> Vec<u8> {
    let mut bytes: Vec<u8> = values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    if bytes.is_empty() {
        bytes.resize(16, 0);
    }
    bytes
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
