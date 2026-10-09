//! Choosing the GPU by its index, for machines with several of one model
//! (Joshi's two RTX 4090s share a name, and Bevy chooses only by name).
//! `plantlab gpus` lists them; `--gpu I` renders on the I-th.
//!
//! The device is made as Bevy makes it when it chooses for itself, with
//! everything the adapter offers except the experimental features, which
//! the raster looks never use.

use bevy::render::renderer::{
    RenderAdapter, RenderAdapterInfo, RenderDevice, RenderInstance, RenderQueue, WgpuWrapper,
};
use bevy::render::settings::RenderCreation;
use bevy::tasks::block_on;
use std::sync::Arc;

fn instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env())
}

fn adapters(instance: &wgpu::Instance) -> Vec<wgpu::Adapter> {
    let backends = wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY);
    block_on(instance.enumerate_adapters(backends))
}

/// One line per GPU: its index, name, kind and backend.
#[must_use]
pub fn list() -> Vec<String> {
    adapters(&instance())
        .iter()
        .enumerate()
        .map(|(index, adapter)| {
            let info = adapter.get_info();
            format!(
                "{index}: {} ({:?}, {:?})",
                info.name, info.device_type, info.backend
            )
        })
        .collect()
}

/// The renderer on GPU `index`.
///
/// # Errors
///
/// When there is no such GPU or its device cannot be made.
pub fn creation(index: usize) -> Result<RenderCreation, String> {
    let instance = instance();
    let mut found = adapters(&instance);
    let count = found.len();
    if index >= count {
        return Err(format!(
            "there is no GPU {index}: {count} found (run `plantlab gpus`)"
        ));
    }
    let adapter = found.swap_remove(index);
    let info = adapter.get_info();
    let mut features = adapter.features();
    features.remove(wgpu::Features::all_experimental_mask());
    if info.device_type == wgpu::DeviceType::DiscreteGpu {
        // As Bevy: slow across the PCIe bus on a discrete GPU.
        features.remove(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS);
    }
    let descriptor = wgpu::DeviceDescriptor {
        label: Some("plantlab"),
        required_features: features,
        required_limits: adapter.limits(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    };
    let (device, queue) = block_on(adapter.request_device(&descriptor))
        .map_err(|error| format!("cannot open GPU {index} ({}): {error}", info.name))?;
    Ok(RenderCreation::manual(
        RenderDevice::from(device),
        RenderQueue(Arc::new(WgpuWrapper::new(queue))),
        RenderAdapterInfo(WgpuWrapper::new(info)),
        RenderAdapter(Arc::new(WgpuWrapper::new(adapter))),
        RenderInstance(Arc::new(WgpuWrapper::new(instance))),
    ))
}
