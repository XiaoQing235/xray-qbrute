use bytemuck::{Pod, Zeroable};

use crate::candidate::CandidateConfig;

use super::{BackendOutcome, ProgressReporter, SearchConfig, SearchError, SearchHit};

const WORKGROUP_SIZE: u64 = 256;
const CANDIDATES_PER_INVOCATION: u64 = 32;
const CANDIDATES_PER_WORKGROUP: u64 = WORKGROUP_SIZE * CANDIDATES_PER_INVOCATION;
const MAX_BATCH: u64 = 1 << 28;
const SEARCH_RESULT_SIZE: u64 = std::mem::size_of::<GpuResult>() as u64;

const SHADER: &str = include_str!("../shaders/sha512_search.wgsl");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WgpuBackend {
    Vulkan,
    Metal,
}

impl WgpuBackend {
    fn backends(self) -> wgpu::Backends {
        match self {
            Self::Vulkan => wgpu::Backends::VULKAN,
            Self::Metal => wgpu::Backends::METAL,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Vulkan => "wgpu-vulkan",
            Self::Metal => "wgpu-metal",
        }
    }

    fn matches(self, actual: wgpu::Backend) -> bool {
        matches!(
            (self, actual),
            (Self::Vulkan, wgpu::Backend::Vulkan) | (Self::Metal, wgpu::Backend::Metal)
        )
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct GpuParams {
    base_lo: u32,
    base_hi: u32,
    commit: u32,
    node_suffix: u32,
    candidate_count: u32,
    leading_zero_bits: u32,
    reserved0: u32,
    reserved1: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct GpuResult {
    claimed: u32,
    index_lo: u32,
    index_hi: u32,
    processed: u32,
}

struct GpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    search_pipeline: wgpu::ComputePipeline,
    #[cfg(test)]
    hash_pipeline: wgpu::ComputePipeline,
    search_bind_group_layout: wgpu::BindGroupLayout,
    #[cfg(test)]
    hash_bind_group_layout: wgpu::BindGroupLayout,
    adapter_name: String,
    backend_name: &'static str,
}

struct SearchBuffers {
    params: wgpu::Buffer,
    result: wgpu::Buffer,
    readback: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

pub fn search(
    config: &SearchConfig,
    selected_backend: WgpuBackend,
    progress: &ProgressReporter,
) -> Result<BackendOutcome, SearchError> {
    let context = create_context(selected_backend)?;
    let buffers = context.create_search_buffers()?;
    let mut base = 0u64;
    let mut processed = 0u64;
    let mut hit = None;

    while base < config.max_index {
        let batch_count = (config.max_index - base).min(MAX_BATCH);
        let result = context.run_search_batch(&buffers, base, batch_count, *config)?;
        let batch_processed = u64::from(result.processed);
        if batch_processed > batch_count
            || (result.claimed == 0 && batch_processed != batch_count)
            || (result.claimed != 0 && batch_processed == 0)
        {
            return Err(SearchError::Runtime(format!(
                "GPU shader reported {batch_processed} processed candidates for a {batch_count}-candidate batch"
            )));
        }
        processed += batch_processed;
        progress(batch_processed);

        if result.claimed != 0 {
            let index = (u64::from(result.index_hi) << 32) | u64::from(result.index_lo);
            if index < base || index >= base + batch_count {
                return Err(SearchError::Runtime(format!(
                    "GPU shader returned index {index} outside batch [{base}, {})",
                    base + batch_count
                )));
            }
            hit = Some(SearchHit { index });
            break;
        }

        base += batch_count;
    }

    Ok(BackendOutcome {
        hit,
        processed,
        backend_name: context.backend_name.to_owned(),
        device_name: Some(context.adapter_name),
        fallback_reasons: Vec::new(),
    })
}

fn create_context(selected_backend: WgpuBackend) -> Result<GpuContext, SearchError> {
    pollster::block_on(create_context_async(selected_backend))
}

async fn create_context_async(selected_backend: WgpuBackend) -> Result<GpuContext, SearchError> {
    let backend_name = selected_backend.name();
    let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    instance_descriptor.backends = selected_backend.backends();
    let instance = wgpu::Instance::new(instance_descriptor);

    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
            apply_limit_buckets: false,
        })
        .await
        .map_err(|error| {
            SearchError::Unavailable(format!("{backend_name} adapter request failed: {error}"))
        })?;
    let info = adapter.get_info();
    if !selected_backend.matches(info.backend) {
        return Err(SearchError::Unavailable(format!(
            "{backend_name} requested but adapter {} uses {:?}",
            info.name, info.backend
        )));
    }
    let required_feature = wgpu::Features::SHADER_INT64;
    if !adapter.features().contains(required_feature) {
        return Err(SearchError::Unavailable(format!(
            "{backend_name} adapter {} does not expose SHADER_INT64",
            info.name
        )));
    }

    let device_label = format!("xray-qbrute {backend_name} device");
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some(&device_label),
            required_features: required_feature,
            required_limits: adapter.limits(),
            ..Default::default()
        })
        .await
        .map_err(|error| {
            SearchError::Unavailable(format!("{backend_name} device request failed: {error}"))
        })?;

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("xray-qbrute SHA-512 WGSL"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });

    let search_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("xray-qbrute search bind group layout"),
            entries: &[storage_binding(0, true), storage_binding(1, false)],
        });
    #[cfg(test)]
    let hash_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("xray-qbrute hash bind group layout"),
            entries: &[storage_binding(0, true), storage_binding(2, false)],
        });

    let search_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("xray-qbrute search pipeline layout"),
        bind_group_layouts: &[Some(&search_bind_group_layout)],
        immediate_size: 0,
    });
    #[cfg(test)]
    let hash_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("xray-qbrute hash pipeline layout"),
        bind_group_layouts: &[Some(&hash_bind_group_layout)],
        immediate_size: 0,
    });

    let search_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("xray-qbrute search pipeline"),
        layout: Some(&search_pipeline_layout),
        module: &shader,
        entry_point: Some("search_main"),
        compilation_options: Default::default(),
        cache: None,
    });
    #[cfg(test)]
    let hash_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("xray-qbrute hash pipeline"),
        layout: Some(&hash_pipeline_layout),
        module: &shader,
        entry_point: Some("hash_main"),
        compilation_options: Default::default(),
        cache: None,
    });

    Ok(GpuContext {
        device,
        queue,
        search_pipeline,
        #[cfg(test)]
        hash_pipeline,
        search_bind_group_layout,
        #[cfg(test)]
        hash_bind_group_layout,
        adapter_name: info.name,
        backend_name,
    })
}

fn storage_binding(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

impl GpuContext {
    fn create_search_buffers(&self) -> Result<SearchBuffers, SearchError> {
        let params = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("xray-qbrute search params"),
            size: std::mem::size_of::<GpuParams>() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let result = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("xray-qbrute search result"),
            size: SEARCH_RESULT_SIZE,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("xray-qbrute search readback"),
            size: SEARCH_RESULT_SIZE,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("xray-qbrute search bind group"),
            layout: &self.search_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: result.as_entire_binding(),
                },
            ],
        });

        Ok(SearchBuffers {
            params,
            result,
            readback,
            bind_group,
        })
    }

    fn run_search_batch(
        &self,
        buffers: &SearchBuffers,
        base: u64,
        candidate_count: u64,
        config: SearchConfig,
    ) -> Result<GpuResult, SearchError> {
        let params = make_params(
            base,
            candidate_count,
            config.candidate,
            config.leading_zero_bits,
        );
        let zero_result = GpuResult::zeroed();
        self.queue
            .write_buffer(&buffers.params, 0, bytemuck::bytes_of(&params));
        self.queue
            .write_buffer(&buffers.result, 0, bytemuck::bytes_of(&zero_result));

        let workgroups = candidate_count.div_ceil(CANDIDATES_PER_WORKGROUP) as u32;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("xray-qbrute search encoder"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("xray-qbrute search pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.search_pipeline);
            pass.set_bind_group(0, &buffers.bind_group, &[]);
            pass.dispatch_workgroups(workgroups, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&buffers.result, 0, &buffers.readback, 0, SEARCH_RESULT_SIZE);
        self.queue.submit(Some(encoder.finish()));

        let bytes = readback(
            &self.device,
            &buffers.readback,
            SEARCH_RESULT_SIZE,
            self.backend_name,
        )?;
        Ok(bytemuck::pod_read_unaligned(&bytes))
    }

    #[cfg(test)]
    fn hash_first_words(
        &self,
        base: u64,
        candidate_count: u64,
        config: CandidateConfig,
    ) -> Result<Vec<u64>, SearchError> {
        let params = make_params(base, candidate_count, config, 0);
        let output_size = candidate_count * std::mem::size_of::<u64>() as u64;
        let params_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("xray-qbrute hash params"),
            size: std::mem::size_of::<GpuParams>() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let output = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("xray-qbrute hash output"),
            size: output_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("xray-qbrute hash readback"),
            size: output_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("xray-qbrute hash bind group"),
            layout: &self.hash_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: output.as_entire_binding(),
                },
            ],
        });
        self.queue
            .write_buffer(&params_buffer, 0, bytemuck::bytes_of(&params));

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("xray-qbrute hash encoder"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("xray-qbrute hash pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.hash_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback_buffer, 0, output_size);
        self.queue.submit(Some(encoder.finish()));

        let bytes = readback(
            &self.device,
            &readback_buffer,
            output_size,
            self.backend_name,
        )?;
        Ok(bytemuck::cast_slice(&bytes).to_vec())
    }
}

fn make_params(
    base: u64,
    candidate_count: u64,
    config: CandidateConfig,
    leading_zero_bits: u32,
) -> GpuParams {
    GpuParams {
        base_lo: base as u32,
        base_hi: (base >> 32) as u32,
        commit: config.commit,
        node_suffix: config.node_suffix,
        candidate_count: candidate_count as u32,
        leading_zero_bits,
        reserved0: 0,
        reserved1: 0,
    }
}

fn readback(
    device: &wgpu::Device,
    buffer: &wgpu::Buffer,
    size: u64,
    backend_name: &str,
) -> Result<Vec<u8>, SearchError> {
    let slice = buffer.slice(0..size);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|error| {
            SearchError::Unavailable(format!("{backend_name} device poll failed: {error}"))
        })?;
    let map_result = receiver.recv().map_err(|error| {
        SearchError::Unavailable(format!("{backend_name} map callback failed: {error}"))
    })?;
    map_result.map_err(|error| {
        SearchError::Unavailable(format!("{backend_name} readback map failed: {error}"))
    })?;

    let view = slice.get_mapped_range().map_err(|error| {
        SearchError::Unavailable(format!("{backend_name} mapped range failed: {error}"))
    })?;
    let bytes = view.to_vec();
    drop(view);
    buffer.unmap();
    Ok(bytes)
}

#[cfg(test)]
mod test;
