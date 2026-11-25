//! The lights' sampler: the lights' answer to the frame's direct-light
//! contract (`helio_frame_api::rays::light_samples`). For each point of a
//! consumer's light batch (GI's probe hits, the reference path tracer's
//! bounces, a reflection's hits) it offers one of the lights that can reach
//! the point, chosen in proportion to the light's unshadowed contribution
//! there, evaluated as the deferred lighting evaluates it (`light_incidence`:
//! the one falloff, cone, IES profile, light function and the air a sun
//! crosses) over the chance it was chosen. Every light here is a delta light
//! (point, spot, directional): the sample's density is 0, and a consumer's
//! BSDF sample never meets one.
//!
//! **The choice** (`light_sampler.wgsl`). A world-space light grid, built
//! before the first batch is sampled ([`LightGridPass`]) and again only when
//! the light list's contents or size, the eye's finest cell or the frame's
//! origin change; a member while any batch's pass can be:
//! six nested levels of 32^3 cells about the eye, level `l`'s cells
//! `2^l` metres across. A local light lives in the finest level whose cells
//! are at least its range wide and whose box holds its sphere (at most 27
//! cells); directional lights and local lights no level holds are globals. A
//! point weighs the globals and the lights of the cell holding it at each
//! level, each by `light_arrival` times the cosine to its normal, and keeps one
//! by weighted reservoir sampling. The choice is isolated in that kernel:
//! r5's resampled choice (ReSTIR, a light tree) replaces it behind the same
//! contract.
//!
//! **What scales with what.** The build is three dispatches over the light
//! list's rows and the grid's 196,608 cells, once however many batches
//! ask; memory is 1.5 MiB of cells and 27 words a light row. A
//! point's cost is the lights that can reach it (the globals, one cell a
//! level), never every light. What the grid does not bound: a scene of many
//! lights whose ranges exceed a kilometre, or far from the eye, are globals
//! every point weighs.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use helio_core::{PassContext, PrepareContext, RenderPass, Result as HelioResult};
use helio_feature_api::{Feature, FeatureRegistrar, NodeBuild};
use helio_frame_api::lighting::sky::{SKY_FRAME, SKY_FRAME_BYTES, SKY_SNIPPET};
use helio_frame_api::lighting::sun_transmittance::{self, SunTransmittance};
use helio_frame_api::rays::light_samples::{
    self, LightBatch, LightSampler, RegisterLightSampler, SamplerId,
};
use helio_frame_api::rays::stream;
use helio_light_api::{IesProfiles, LIGHT_LIST, SCENE_LIGHTS};
use pulsar_scenedb::gpu::BufferKey;

/// The grid's levels, cells a side, and its finest cells' size (metres);
/// `light_sampler.wgsl`'s `GRID_LEVELS` and `GRID_SIDE`.
pub const GRID_LEVELS: u32 = 6;
pub const GRID_SIDE: u32 = 32;
pub const FINEST_CELL: f32 = 1.0;
const CELLS: u32 = GRID_LEVELS * GRID_SIDE * GRID_SIDE * GRID_SIDE;
/// Cells a light covers at most: its entries a row.
const CELLS_A_LIGHT: u64 = 27;

/// The grid's buffers, as its build publishes them for the batches' passes.
const GRID_COUNTS: &str = "light_sample_grid_counts";
const GRID_OFFSETS: &str = "light_sample_grid_offsets";
const GRID_LIGHTS: &str = "light_sample_grid_lights";
const GRID_GLOBALS: &str = "light_sample_grid_globals";
const GRID_PARAMS: &str = "light_sample_grid_params";

/// The snippets `light_sampler.wgsl` is built with.
pub const SNIPPETS: [helio_core::shader::ShaderSnippet; 6] = [
    helio_light_api::LIGHT_LIST_SNIPPET,
    helio_light_api::LIGHT_INCIDENCE_SNIPPET,
    SKY_SNIPPET,
    stream::SNIPPET,
    light_samples::SNIPPET,
    sun_transmittance::SNIPPET,
];

/// `GridParams` (`light_sampler.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GridParams {
    eye: [f32; 4],
    info: [u32; 4],
}

/// The lights' sampler as a feature: it registers the sampler every graph's
/// light batches are answered by.
pub struct LightSamplerFeature;

impl Feature for LightSamplerFeature {
    fn id(&self) -> &'static str {
        "helio.lights.sampler"
    }

    fn register(&self, registrar: &mut FeatureRegistrar<'_>) {
        registrar.consumes(SCENE_LIGHTS);
        registrar.swappable::<LightGridPass>();
        registrar.swappable::<LightSamplePass>();
        registrar.light_sampler(sampler());
    }
}

/// The lights' [`LightSampler`] (what [`LightSamplerFeature`] registers; a
/// test or a host without features offers it with
/// [`light_samples::offer`]).
pub fn sampler() -> LightSampler {
    LightSampler {
        name: "lights",
        supported: Arc::new(|_| true),
        sample: Arc::new(build_sample),
    }
}

/// This graph's light grid: whether its pass is added, and the member
/// gates its batches were added under (`None`: always a member). The grid
/// is a member while any batch's pass can be: a consumer that runs only on
/// request (the reference) does not keep it for the others, nor do the
/// others drop it from under that consumer.
#[derive(Default)]
struct Grid {
    added: bool,
    users: Arc<std::sync::Mutex<Vec<Option<helio_core::MemberGate>>>>,
}

fn build_sample(
    build: &mut NodeBuild<'_, '_>,
    batch: &LightBatch,
    id: SamplerId,
) -> Result<(), String> {
    let gate = build.graph.member_gate();
    let grid = build.env.shared(Grid::default);
    grid.users.lock().unwrap().push(gate);
    let added = std::mem::replace(&mut grid.added, true);
    let users = grid.users.clone();
    let device = build.env.device.clone();
    if !added {
        let any: helio_core::MemberGate = Arc::new(move || {
            users
                .lock()
                .unwrap()
                .iter()
                .any(|gate| gate.as_ref().is_none_or(|open| open()))
        });
        let outer = build.graph.set_member_gate(Some(any));
        build
            .graph
            .add_pass(Box::new(LightGridPass::new(&device, id)));
        build.graph.set_member_gate(outer);
    }
    let sun = sun_transmittance::current(build.env);
    let ies = build
        .env
        .setting::<IesProfiles>()
        .map(|profiles| profiles.0.clone());
    let queue = build.env.queue.clone();
    build.graph.add_pass(Box::new(LightSamplePass::new(
        &device, &queue, *batch, sun, ies,
    )));
    Ok(())
}

fn storage(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
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

fn uniform(binding: u32, size: u64) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: wgpu::BufferSize::new(size),
        },
        count: None,
    }
}

fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

fn buffer(
    device: &wgpu::Device,
    label: &str,
    size: u64,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size.max(16),
        usage,
        mapped_at_creation: false,
    })
}

/// Group 0 of both pipelines: the grid's parameters and the light list.
fn common_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Light sampler common"),
        entries: &[
            uniform(0, std::mem::size_of::<GridParams>() as u64),
            storage(1, true),
        ],
    })
}

fn module(device: &wgpu::Device) -> wgpu::ShaderModule {
    helio_core::shader::module_with(
        device,
        "Light sampler",
        helio_core::include_wgsl!("../shaders/light_sampler.wgsl"),
        &SNIPPETS,
    )
}

fn pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layouts: &[Option<&wgpu::BindGroupLayout>],
    entry: &str,
) -> helio_core::shader::DeferredCompute {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(entry),
        bind_group_layouts: layouts,
        immediate_size: 0,
    });
    helio_core::shader::create_compute_pipeline_deferred(
        device,
        wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout: Some(&layout),
            module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        },
    )
}

/// The light grid's build (module docs), once a graph's frame, before the
/// first batch's samples. Its buffers persist over graph rebuilds and grow
/// with the light list.
pub struct LightGridPass {
    common: wgpu::BindGroupLayout,
    layout: wgpu::BindGroupLayout,
    count: helio_core::shader::DeferredCompute,
    allocate: helio_core::shader::DeferredCompute,
    fill: helio_core::shader::DeferredCompute,
    params: wgpu::Buffer,
    counts: wgpu::Buffer,
    offsets: wgpu::Buffer,
    entries: wgpu::Buffer,
    globals: wgpu::Buffer,
    alloc: wgpu::Buffer,
    /// The light list's rows this frame (none: no list).
    rows: u32,
    sampler: SamplerId,
    /// What the grid in the buffers was built from (the light list's
    /// contents and size, the eye's finest cell, the frame's origin), and
    /// this frame's: the grid is rebuilt only when they differ.
    built: Option<[u64; 8]>,
    wanted: [u64; 8],
    cache: helio_core::gpu_cache::GpuCache,
}

impl LightGridPass {
    /// The build, for samples sampler `sampler` writes.
    pub fn new(device: &wgpu::Device, sampler: SamplerId) -> Self {
        let common = common_layout(device);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Light grid build"),
            entries: &[
                storage(0, false),
                storage(1, false),
                storage(2, false),
                storage(3, false),
                storage(4, false),
            ],
        });
        let module = module(device);
        let layouts = [Some(&common), Some(&layout)];
        let storage_usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        Self {
            count: pipeline(device, &module, &layouts, "grid_count"),
            allocate: pipeline(device, &module, &layouts, "grid_allocate"),
            fill: pipeline(device, &module, &layouts, "grid_fill"),
            params: buffer(
                device,
                "Light grid params",
                std::mem::size_of::<GridParams>() as u64,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            ),
            counts: buffer(
                device,
                "Light grid counts",
                u64::from(CELLS) * 4,
                storage_usage,
            ),
            offsets: buffer(
                device,
                "Light grid offsets",
                u64::from(CELLS) * 4,
                storage_usage,
            ),
            entries: buffer(device, "Light grid entries", 16, storage_usage),
            globals: buffer(device, "Light grid globals", 16, storage_usage),
            alloc: buffer(device, "Light grid allocation", 16, storage_usage),
            common,
            layout,
            rows: 0,
            sampler,
            built: None,
            wanted: [0; 8],
            cache: helio_core::gpu_cache::GpuCache::new(),
        }
    }
}

impl RenderPass for LightGridPass {
    fn name(&self) -> &'static str {
        "LightSampleGrid"
    }

    fn writes(&self) -> &'static [&'static str] {
        &[
            GRID_PARAMS,
            GRID_COUNTS,
            GRID_OFFSETS,
            GRID_LIGHTS,
            GRID_GLOBALS,
        ]
    }

    fn scene_buffers(&self) -> &'static [&'static str] {
        &[LIGHT_LIST]
    }

    fn inherit_persistent_state(&mut self, previous: &mut dyn RenderPass) -> bool {
        let Some(previous) = previous.as_any_mut().downcast_mut::<Self>() else {
            return false;
        };
        std::mem::swap(&mut self.entries, &mut previous.entries);
        std::mem::swap(&mut self.globals, &mut previous.globals);
        std::mem::swap(&mut self.counts, &mut previous.counts);
        std::mem::swap(&mut self.offsets, &mut previous.offsets);
        self.built = previous.built.take();
        true
    }

    fn prepare(&mut self, ctx: &PrepareContext) -> HelioResult<()> {
        let list = ctx.scene_buffers.get(BufferKey::of(LIGHT_LIST));
        self.rows = list.map_or(0, |lights| lights.row_capacity());
        let storage_usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        let rows = u64::from(self.rows.max(1));
        if self.entries.size() < rows * CELLS_A_LIGHT * 4 {
            self.entries = buffer(
                ctx.device,
                "Light grid entries",
                (rows * CELLS_A_LIGHT * 4).next_power_of_two(),
                storage_usage,
            );
        }
        if self.globals.size() < (rows + 1) * 4 {
            self.globals = buffer(
                ctx.device,
                "Light grid globals",
                ((rows + 1) * 4).next_power_of_two(),
                storage_usage,
            );
        }
        let eye = &ctx.camera_data.position_near;
        // The grid's levels are placed on the eye's finest cell (every
        // coarser level's cell follows from it), so the eye moving within
        // that cell changes no cell's lights.
        let cell = |i: usize| (f64::from(eye[i]) / f64::from(FINEST_CELL)).floor() as i64 as u64;
        let origin = ctx.world_origin;
        self.wanted = [
            list.map_or(0, |l| l.epoch),
            list.map_or(0, |l| l.content_generation),
            u64::from(self.rows) | (u64::from(self.sampler.0) << 32),
            cell(0),
            cell(1),
            cell(2),
            origin.x.to_bits() ^ origin.y.to_bits().rotate_left(21),
            origin.z.to_bits(),
        ];
        let params = GridParams {
            eye: [eye[0], eye[1], eye[2], FINEST_CELL],
            info: [self.rows, self.sampler.0, 0, 0],
        };
        helio_core::upload::write_buffer(ctx.queue, &self.params, 0, bytemuck::bytes_of(&params));
        Ok(())
    }

    fn execute(&mut self, ctx: &mut PassContext) -> HelioResult<()> {
        let Some(lights) = ctx
            .scene_buffers
            .get(BufferKey::of(LIGHT_LIST))
            .map(|handle| handle.buffer.clone())
        else {
            return Ok(());
        };
        self.cache.end_frame();
        // The grid in the buffers is this frame's: nothing to build.
        if self.built == Some(self.wanted) {
            return Ok(());
        }
        self.built = Some(self.wanted);
        let common = self.cache.bind_group(
            ctx.device,
            &wgpu::BindGroupDescriptor {
                label: Some("Light sampler common"),
                layout: &self.common,
                entries: &[entry(0, &self.params), entry(1, &lights)],
            },
        );
        let group = self.cache.bind_group(
            ctx.device,
            &wgpu::BindGroupDescriptor {
                label: Some("Light grid build"),
                layout: &self.layout,
                entries: &[
                    entry(0, &self.counts),
                    entry(1, &self.offsets),
                    entry(2, &self.entries),
                    entry(3, &self.globals),
                    entry(4, &self.alloc),
                ],
            },
        );
        let mut cmds = ctx.graphics_cmds();
        cmds.clear_buffer(&self.counts, 0, None);
        cmds.clear_buffer(&self.globals, 0, Some(16));
        cmds.clear_buffer(&self.alloc, 0, None);
        let mut pass = cmds.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Light grid build"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, Some(&common), &[]);
        pass.set_bind_group(1, Some(&group), &[]);
        let light_groups = self.rows.div_ceil(64);
        pass.set_pipeline(&self.count);
        pass.dispatch_workgroups(light_groups, 1, 1);
        pass.set_pipeline(&self.allocate);
        pass.dispatch_workgroups(CELLS.div_ceil(64), 1, 1);
        pass.set_pipeline(&self.fill);
        pass.dispatch_workgroups(light_groups, 1, 1);
        Ok(())
    }

    fn publish<'a>(&self, frame: &mut helio_core::ResourceRegistry<'a>) {
        frame.write_buffer_binding(GRID_PARAMS, &self.params, "LightSampleGrid");
        frame.write_buffer_binding(GRID_COUNTS, &self.counts, "LightSampleGrid");
        frame.write_buffer_binding(GRID_OFFSETS, &self.offsets, "LightSampleGrid");
        frame.write_buffer_binding(GRID_LIGHTS, &self.entries, "LightSampleGrid");
        frame.write_buffer_binding(GRID_GLOBALS, &self.globals, "LightSampleGrid");
    }
}

/// One batch's samples (module docs): an indirect dispatch over the batch's
/// header, after the grid's build and the consumer's queries.
pub struct LightSamplePass {
    batch: LightBatch,
    common: wgpu::BindGroupLayout,
    layout: wgpu::BindGroupLayout,
    empty: wgpu::BindGroupLayout,
    pipeline: helio_core::shader::DeferredCompute,
    sun: Option<SunTransmittance>,
    no_sun: (wgpu::TextureView, wgpu::Buffer),
    no_sky: wgpu::Buffer,
    ies: wgpu::TextureView,
    ies_sampler: wgpu::Sampler,
    cache: helio_core::gpu_cache::GpuCache,
    reads: &'static [&'static str],
    writes: &'static [&'static str],
}

impl LightSamplePass {
    /// The pass sampling `batch`, the sun through the media `sun`
    /// published (none: no medium), the lights' profiles `ies` (none: the
    /// stand-in).
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: LightBatch,
        sun: Option<SunTransmittance>,
        ies: Option<wgpu::TextureView>,
    ) -> Self {
        let common = common_layout(device);
        let texture = |binding, sample_type, view_dimension| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Light samples"),
            entries: &[
                storage(0, true),
                storage(1, true),
                storage(2, true),
                storage(3, true),
                storage(4, true),
                storage(5, true),
                storage(6, false),
                uniform(7, SKY_FRAME_BYTES),
                texture(
                    8,
                    wgpu::TextureSampleType::Float { filterable: true },
                    wgpu::TextureViewDimension::D2Array,
                ),
                wgpu::BindGroupLayoutEntry {
                    binding: 9,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture(
                    10,
                    wgpu::TextureSampleType::Float { filterable: false },
                    wgpu::TextureViewDimension::D2,
                ),
                uniform(11, sun_transmittance::FRAME_BYTES),
            ],
        });
        let empty = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Light samples (no build)"),
            entries: &[],
        });
        let pipeline = pipeline(
            device,
            &module(device),
            &[Some(&common), Some(&empty), Some(&layout)],
            "light_sample",
        );
        let mut reads = vec![
            GRID_PARAMS,
            GRID_COUNTS,
            GRID_OFFSETS,
            GRID_LIGHTS,
            GRID_GLOBALS,
            batch.header,
            batch.queries,
            batch.samples,
            SKY_FRAME,
        ];
        if let Some(sun) = sun {
            reads.extend([sun.map, sun.frame]);
        }
        Self {
            batch,
            common,
            layout,
            empty,
            pipeline,
            sun,
            no_sun: sun_transmittance::stand_in(device),
            no_sky: buffer(
                device,
                "Light samples no sky",
                SKY_FRAME_BYTES,
                wgpu::BufferUsages::UNIFORM,
            ),
            ies: ies.unwrap_or_else(|| helio_light_api::ies_stand_in(device, queue)),
            ies_sampler: helio_light_api::ies_sampler(device),
            cache: helio_core::gpu_cache::GpuCache::new(),
            reads: helio_frame_api::static_names(&reads),
            writes: helio_frame_api::static_names(&[batch.samples]),
        }
    }
}

impl RenderPass for LightSamplePass {
    fn name(&self) -> &'static str {
        "LightSamples"
    }

    fn reads(&self) -> &'static [&'static str] {
        self.reads
    }

    fn writes(&self) -> &'static [&'static str] {
        self.writes
    }

    fn scene_buffers(&self) -> &'static [&'static str] {
        &[LIGHT_LIST]
    }

    fn execute(&mut self, ctx: &mut PassContext) -> HelioResult<()> {
        let binding = |name: &str| ctx.registry.buffer_binding(name);
        let (
            Some(lights),
            Some(params),
            Some(counts),
            Some(offsets),
            Some(entries),
            Some(globals),
            Some(header),
            Some(queries),
            Some(samples),
        ) = (
            ctx.scene_buffers
                .get(BufferKey::of(LIGHT_LIST))
                .map(|handle| handle.buffer.clone()),
            binding(GRID_PARAMS),
            binding(GRID_COUNTS),
            binding(GRID_OFFSETS),
            binding(GRID_LIGHTS),
            binding(GRID_GLOBALS),
            binding(self.batch.header),
            binding(self.batch.queries),
            binding(self.batch.samples),
        )
        else {
            // No light list: no light to offer.
            return Ok(());
        };
        let sky = ctx
            .registry
            .get::<&wgpu::Buffer>(helio_core::ResourceKey::new(SKY_FRAME))
            .unwrap_or(&self.no_sky);
        let (sun_map, sun_frame) = self
            .sun
            .and_then(|sun| {
                ctx.registry
                    .texture_binding(sun.map)
                    .zip(binding(sun.frame))
            })
            .unwrap_or((&self.no_sun.0, &self.no_sun.1));
        self.cache.end_frame();
        let common = self.cache.bind_group(
            ctx.device,
            &wgpu::BindGroupDescriptor {
                label: Some("Light sampler common"),
                layout: &self.common,
                entries: &[entry(0, params), entry(1, &lights)],
            },
        );
        let group = self.cache.bind_group(
            ctx.device,
            &wgpu::BindGroupDescriptor {
                label: Some("Light samples"),
                layout: &self.layout,
                entries: &[
                    entry(0, counts),
                    entry(1, offsets),
                    entry(2, entries),
                    entry(3, globals),
                    entry(4, header),
                    entry(5, queries),
                    entry(6, samples),
                    entry(7, sky),
                    wgpu::BindGroupEntry {
                        binding: 8,
                        resource: wgpu::BindingResource::TextureView(&self.ies),
                    },
                    wgpu::BindGroupEntry {
                        binding: 9,
                        resource: wgpu::BindingResource::Sampler(&self.ies_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 10,
                        resource: wgpu::BindingResource::TextureView(sun_map),
                    },
                    entry(11, sun_frame),
                ],
            },
        );
        let empty = self.cache.bind_group(
            ctx.device,
            &wgpu::BindGroupDescriptor {
                label: Some("Light samples (no build)"),
                layout: &self.empty,
                entries: &[],
            },
        );
        let header = header.clone();
        let mut cmds = ctx.graphics_cmds();
        let mut pass = cmds.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Light samples"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, Some(&common), &[]);
        pass.set_bind_group(1, Some(&empty), &[]);
        pass.set_bind_group(2, Some(&group), &[]);
        pass.set_pipeline(&self.pipeline);
        pass.dispatch_workgroups_indirect(&header, 0);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sampler's WGSL parses and validates as its passes compose it, and
    /// copies no per-frame table into invocation memory.
    #[test]
    fn the_shader_validates() {
        let text = helio_core::shader::resolve_with(
            include_str!("../shaders/light_sampler.wgsl"),
            &SNIPPETS,
        );
        let module = wgpu::naga::front::wgsl::parse_str(&text)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&text)));
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&text)));
        let copies = helio_core::shader::value_array_copies(&text).unwrap();
        assert!(copies.is_empty(), "{copies:#?}");
    }

    /// The shader's grid constants are this crate's.
    #[test]
    fn grid_constants_match() {
        let source = include_str!("../shaders/light_sampler.wgsl");
        assert!(source.contains(&format!("const GRID_LEVELS: u32 = {GRID_LEVELS}u;")));
        assert!(source.contains(&format!("const GRID_SIDE: u32 = {GRID_SIDE}u;")));
        assert_eq!(std::mem::size_of::<GridParams>(), 32);
    }
}
