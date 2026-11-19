//! The lights' sampler on a real device: a light batch sampled through the
//! frame's contract (`light_samples::sample`, the sampler offered as a kind
//! offers it), its rows read back and checked against a brute-force sum over
//! every light computed on the GPU with the same `light_incidence` the
//! sampler and the deferred lighting call.
//!
//! - **Unbiased**: at each point, the mean over N samples of
//!   `light_sample_radiance * max(n.l, 0)` converges to the sum over all
//!   lights of `light_incidence * max(n.l, 0)`. Tolerance: five standard
//!   errors of the mean (the samples' own spread over sqrt(N)), plus 1e-4 of
//!   the sum for f32 and the samples' packing.
//! - **Delta lights**: every sample's density is 0 (point, spot and
//!   directional lights are deltas); a point no light reaches gets none.
//! - **Spaces**: a point of one coordinate space is lit only by its space's
//!   lights (and a query of every space by all), as the deferred lighting
//!   matches them.
//! - **Sides**: a light behind a one-sided point is never chosen.

use std::sync::Arc;

use bytemuck::Zeroable;
use glam::{DVec3, Vec3};
use helio_core::{
    BufferHandle, GpuCameraUniforms, PassContext, RenderGraph, RenderPass, Result as HelioResult,
    SceneBufferProjection, SceneInput,
};
use helio_feature_api::{BuildEnv, FrameDesc, NodeBuild};
use helio_frame_api::rays::light_samples::{
    self, LightBatch, LightBatchBuffers, LightQuery, LightSample, QUERY_ANY_SIDE,
};
use helio_frame_api::rays::stream::{pack_unit, unpack_unit, StreamHeader};
use helio_light_api::GpuLight;

struct Gpu {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
}

fn gpu() -> Option<&'static Gpu> {
    static GPU: std::sync::OnceLock<Option<Gpu>> = std::sync::OnceLock::new();
    GPU.get_or_init(|| {
        let instance = wgpu::Instance::new(helio_core::wgpu_instance_descriptor());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).ok()?;
        Some(Gpu {
            device: Arc::new(device),
            queue: Arc::new(queue),
        })
    })
    .as_ref()
}

fn read<T: bytemuck::Pod>(gpu: &Gpu, buffer: &wgpu::Buffer) -> Vec<T> {
    let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: buffer.size(),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, buffer.size());
    gpu.queue.submit([encoder.finish()]);
    staging.slice(..).map_async(wgpu::MapMode::Read, |r| r.unwrap());
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    bytemuck::cast_slice(&staging.slice(..).get_mapped_range().unwrap()).to_vec()
}

fn hash(mut x: u32) -> u32 {
    x = x.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    x = ((x >> ((x >> 28) + 4)) ^ x).wrapping_mul(277_803_737);
    (x >> 22) ^ x
}

/// A point of the test: where, its normal, its space (the ray encoding),
/// lit from every side or not.
#[derive(Clone, Copy)]
struct Point {
    at: Vec3,
    normal: Vec3,
    space: u32,
    any_side: bool,
}

/// The test's queries: `samples` rows a point, each with its own numbers.
struct Queries {
    batch: LightBatch,
    buffers: LightBatchBuffers,
    rows: Vec<LightQuery>,
}

impl RenderPass for Queries {
    fn name(&self) -> &'static str {
        "TestQueries"
    }

    fn writes(&self) -> &'static [&'static str] {
        helio_frame_api::static_names(&[self.batch.header, self.batch.queries, self.batch.samples])
    }

    fn execute(&mut self, ctx: &mut PassContext) -> HelioResult<()> {
        ctx.queue.write_buffer(
            &self.buffers.header,
            0,
            bytemuck::bytes_of(&StreamHeader::of(self.rows.len() as u32)),
        );
        ctx.queue
            .write_buffer(&self.buffers.queries, 0, bytemuck::cast_slice(&self.rows));
        let mut cmds = ctx.graphics_cmds();
        self.buffers.clear(&mut cmds);
        Ok(())
    }

    fn publish<'a>(&self, frame: &mut helio_core::ResourceRegistry<'a>) {
        self.buffers.publish(&self.batch, frame, "TestQueries");
    }
}

struct Scene {
    gpu: &'static Gpu,
    camera: wgpu::Buffer,
    camera_data: GpuCameraUniforms,
    buffers: SceneBufferProjection,
}

impl SceneInput for Scene {
    fn world_origin(&self) -> DVec3 {
        DVec3::ZERO
    }
    fn device(&self) -> &Arc<wgpu::Device> {
        &self.gpu.device
    }
    fn queue(&self) -> &Arc<wgpu::Queue> {
        &self.gpu.queue
    }
    fn frame_count(&self) -> u64 {
        0
    }
    fn camera(&self) -> &wgpu::Buffer {
        &self.camera
    }
    fn camera_data(&self) -> &GpuCameraUniforms {
        &self.camera_data
    }
    fn camera_generation(&self) -> u64 {
        0
    }
    fn scene_buffers(&self) -> &SceneBufferProjection {
        &self.buffers
    }
}

/// The brute force: at each point, the sum over every light of
/// `light_incidence * cosine` (the side test and space match the sampler
/// applies), by the lights' own WGSL.
const BRUTE: &str = r#"
//!use light_list
//!use light_incidence
//!use frame_sky
//!use frame_ray_stream
//!use frame_light_samples
@group(0) @binding(0) var<storage, read> lights: array<GpuLight>;
@group(0) @binding(1) var<storage, read> points: array<LightQuery>;
@group(0) @binding(2) var<storage, read_write> sums: array<vec4<f32>>;
@group(0) @binding(3) var<uniform> sky_frame: SkyFrame;
@group(0) @binding(4) var ies_textures: texture_2d_array<f32>;
@group(0) @binding(5) var ies_sampler: sampler;
@group(0) @binding(6) var<uniform> eye: vec4<f32>;

@compute @workgroup_size(64)
fn brute(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= arrayLength(&points) { return; }
    let q = points[id.x];
    var sum = vec3<f32>(0.0);
    for (var i = 0u; i < arrayLength(&lights); i++) {
        let light = lights[i];
        if light.color_intensity.w <= 0.0 { continue; }
        if !light_query_sees_space(q, light_space(light)) { continue; }
        let inc = light_incidence(light, q.position, eye.xyz);
        var cosine = 1.0;
        if (q.space & LIGHT_QUERY_ANY_SIDE) == 0u {
            cosine = max(dot(scene_unpack_unit(q.normal), inc.L), 0.0);
        }
        sum += inc.radiance * cosine;
    }
    sums[id.x] = vec4<f32>(sum, 0.0);
}
"#;

fn brute_force(gpu: &Gpu, lights: &wgpu::Buffer, points: &[LightQuery], eye: Vec3) -> Vec<Vec3> {
    use wgpu::util::DeviceExt;
    let device = &gpu.device;
    let init = |contents: &[u8], usage| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("brute force"),
            contents,
            usage,
        })
    };
    let points_buffer = init(bytemuck::cast_slice(points), wgpu::BufferUsages::STORAGE);
    let sums = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("brute force sums"),
        size: points.len() as u64 * 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let sky = init(
        &vec![0u8; helio_frame_api::lighting::sky::SKY_FRAME_BYTES as usize],
        wgpu::BufferUsages::UNIFORM,
    );
    let eye_buffer = init(bytemuck::cast_slice(&[eye.extend(0.0)]), wgpu::BufferUsages::UNIFORM);
    let ies = helio_light_api::ies_stand_in(device, &gpu.queue);
    let sampler = helio_light_api::ies_sampler(device);
    let module = helio_core::shader::module_with(
        device,
        "brute force",
        BRUTE,
        &helio_pass_light_sampler::SNIPPETS,
    );
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("brute force"),
        layout: None,
        module: &module,
        entry_point: Some("brute"),
        compilation_options: Default::default(),
        cache: None,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("brute force"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: lights.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: points_buffer.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: sums.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: sky.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&ies) },
            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&sampler) },
            wgpu::BindGroupEntry { binding: 6, resource: eye_buffer.as_entire_binding() },
        ],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, Some(&group), &[]);
        pass.dispatch_workgroups((points.len() as u32).div_ceil(64), 1, 1);
    }
    gpu.queue.submit([encoder.finish()]);
    read::<[f32; 4]>(gpu, &sums)
        .into_iter()
        .map(|s| Vec3::new(s[0], s[1], s[2]))
        .collect()
}

/// Samples `samples` rows at each of `points` among `lights`, the eye at
/// `eye`: the rows, and the brute-force sum at each point.
fn sample(
    gpu: &'static Gpu,
    lights: &[GpuLight],
    points: &[Point],
    samples: u32,
    eye: Vec3,
) -> (Vec<LightSample>, Vec<Vec3>) {
    let query = |p: &Point, pick: u32, random: u32| LightQuery {
        position: p.at.to_array(),
        width: 0.0,
        normal: pack_unit(p.normal.to_array()),
        random,
        pick,
        space: p.space | if p.any_side { QUERY_ANY_SIDE } else { 0 },
    };
    let mut rows = Vec::new();
    for (k, p) in points.iter().enumerate() {
        for s in 0..samples {
            let n = (k as u32) * samples + s;
            rows.push(query(p, hash(n ^ 0x51ed_270b), hash(n ^ 0x2c1b_3c6d)));
        }
    }
    let batch = LightBatch::new("test_lights", rows.len() as u32);
    let buffers = LightBatchBuffers::new(&gpu.device, &batch);
    let samples_buffer = buffers.samples.clone();
    let stats = helio_core::FrameStats::new(&gpu.device);
    let camera = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test camera"),
        size: std::mem::size_of::<GpuCameraUniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let frame = FrameDesc {
        width: 16,
        height: 16,
        internal_width: 16,
        internal_height: 16,
        surface_format: wgpu::TextureFormat::Rgba8Unorm,
        xr: false,
    };
    let mut env = BuildEnv::new(&gpu.device, &gpu.queue, frame, &camera, &camera, &stats, true);
    light_samples::offer(&mut env, helio_pass_light_sampler::sampler());
    let mut graph = RenderGraph::new(&gpu.device, &gpu.queue);
    graph.add_pass(Box::new(Queries {
        batch,
        buffers,
        rows: rows.clone(),
    }));
    let params = helio_feature_api::Params::default();
    let answered = light_samples::sample(
        &mut NodeBuild {
            graph: &mut graph,
            env: &mut env,
            params: &params,
            node_id: "test",
        },
        &batch,
    )
    .unwrap();
    assert_eq!(answered, 1);
    graph.lock(16, 16);
    use wgpu::util::DeviceExt;
    let light_buffer = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("test lights"),
            contents: bytemuck::cast_slice(lights),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let mut camera_data = GpuCameraUniforms::zeroed();
    camera_data.position_near = [eye.x, eye.y, eye.z, 0.1];
    let scene = Scene {
        gpu,
        camera,
        camera_data,
        buffers: SceneBufferProjection::from_handles([(
            helio_light_api::SCENE_LIGHTS.key(),
            BufferHandle {
                buffer: light_buffer.clone(),
                epoch: 0,
                row_bytes: std::mem::size_of::<GpuLight>() as u64,
                content_generation: 1,
            },
        )]),
    };
    let texture = |format| {
        gpu.device
            .create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default())
    };
    let (target, depth) = (
        texture(wgpu::TextureFormat::Rgba8Unorm),
        texture(wgpu::TextureFormat::Depth32Float),
    );
    let mut registry = helio_core::ResourceRegistry::empty();
    graph
        .execute_with_registry(&scene, &target, &depth, &mut registry)
        .expect("the frame renders");
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    let out = read::<LightSample>(gpu, &samples_buffer);
    let firsts: Vec<LightQuery> = points
        .iter()
        .map(|p| query(p, 0, 0))
        .collect();
    let brute = brute_force(gpu, &light_buffer, &firsts, eye);
    (out[..rows.len()].to_vec(), brute)
}

fn point_light(at: Vec3, color: Vec3, intensity: f32, range: f32) -> GpuLight {
    GpuLight {
        position_range: [at.x, at.y, at.z, range],
        color_intensity: [color.x, color.y, color.z, intensity],
        shadow_index: u32::MAX,
        light_type: 1,
        ..Default::default()
    }
}

fn spot_light(at: Vec3, dir: Vec3, intensity: f32, range: f32, outer: f32, inner: f32) -> GpuLight {
    GpuLight {
        position_range: [at.x, at.y, at.z, range],
        direction_outer: [dir.x, dir.y, dir.z, outer],
        color_intensity: [1.0, 0.9, 0.7, intensity],
        inner_angle: inner,
        shadow_index: u32::MAX,
        light_type: 2,
        ..Default::default()
    }
}

fn sun(dir: Vec3, illuminance: f32) -> GpuLight {
    GpuLight {
        direction_outer: [dir.x, dir.y, dir.z, 0.0],
        color_intensity: [1.0, 0.95, 0.9, illuminance],
        shadow_index: u32::MAX,
        light_type: 0,
        ..Default::default()
    }
}

fn in_space(mut light: GpuLight, slot: u32) -> GpuLight {
    light._pad = (light._pad & !(255 << 23)) | (slot << 23);
    light
}

/// `(mean, standard error)` of `light_sample_radiance * cosine` (luminance
/// and colour) over a point's rows.
fn estimate(rows: &[LightSample], point: &Point) -> (Vec3, Vec3) {
    let n = rows.len() as f64;
    let mut sum = [0.0f64; 3];
    let mut sq = [0.0f64; 3];
    for s in rows {
        let c = s.contribution();
        let dir = Vec3::from(unpack_unit(s.dir));
        let cosine = if point.any_side {
            1.0
        } else {
            point.normal.dot(dir).max(0.0)
        };
        for k in 0..3 {
            let v = f64::from(c[k] * cosine);
            sum[k] += v;
            sq[k] += v * v;
        }
    }
    let mean = sum.map(|s| s / n);
    let se = [0, 1, 2].map(|k| ((sq[k] / n - mean[k] * mean[k]).max(0.0) / n).sqrt());
    (
        Vec3::new(mean[0] as f32, mean[1] as f32, mean[2] as f32),
        Vec3::new(se[0] as f32, se[1] as f32, se[2] as f32),
    )
}

fn check(rows: &[LightSample], brute: &[Vec3], points: &[Point], samples: usize) {
    for (k, point) in points.iter().enumerate() {
        let rows = &rows[k * samples..(k + 1) * samples];
        let (mean, se) = estimate(rows, point);
        let truth = brute[k];
        let tolerance = se * 5.0 + truth * 1e-4 + Vec3::splat(1e-6);
        assert!(
            (mean - truth).abs().cmple(tolerance).all(),
            "point {k} at {}: mean {mean} (standard error {se}) against the sum {truth}",
            point.at
        );
        for s in rows {
            // Every light here is a delta light.
            assert_eq!(s.pdf, 0.0);
            assert_eq!(s.pdf(), 0.0);
        }
    }
}

/// Many lights of many ranges (some past the grid's finest levels, some
/// past all of them, a spot, two suns), points inside and outside the grid:
/// the mean of the samples converges to the sum.
#[test]
fn the_samples_are_unbiased_against_the_sum_of_every_light() {
    let Some(gpu) = gpu() else { return };
    let eye = Vec3::new(3.0, 2.0, -1.0);
    let mut lights = Vec::new();
    for i in 0..48u32 {
        let f = i as f32;
        let at = Vec3::new((f * 7.3).sin() * 20.0, 1.0 + (f * 1.7).cos().abs() * 4.0, (f * 3.1).cos() * 20.0);
        let range = [0.6, 1.5, 3.0, 8.0, 20.0, 70.0][i as usize % 6];
        let color = Vec3::new(1.0, 0.5 + 0.5 * (f * 0.9).sin().abs(), 0.3);
        lights.push(point_light(at, color, 5.0 + f, range));
    }
    // Past every level: a global.
    lights.push(point_light(Vec3::new(400.0, 50.0, 0.0), Vec3::ONE, 2.0e5, 900.0));
    lights.push(spot_light(Vec3::new(0.0, 6.0, 0.0), Vec3::new(0.0, -1.0, 0.0), 300.0, 15.0, 0.7, 0.9));
    lights.push(sun(Vec3::new(0.3, -1.0, 0.2).normalize(), 3.0));
    lights.push(sun(Vec3::new(-0.6, -0.4, 0.0).normalize(), 0.5));
    // An unused (zeroed) row and a black light: no light.
    lights.push(bytemuck::Zeroable::zeroed());
    lights.push(point_light(Vec3::ZERO, Vec3::ONE, 0.0, 5.0));
    let points: Vec<Point> = [
        (Vec3::new(0.0, 0.0, 0.0), Vec3::Y),
        (Vec3::new(5.0, 1.0, 3.0), Vec3::new(0.2, 1.0, 0.1).normalize()),
        (Vec3::new(-12.0, 0.5, 8.0), Vec3::new(1.0, 0.3, 0.0).normalize()),
        (Vec3::new(18.0, 3.0, -15.0), Vec3::new(-1.0, 0.0, 0.2).normalize()),
        // Outside the finest levels, and outside every level.
        (Vec3::new(150.0, 2.0, 40.0), Vec3::Y),
        (Vec3::new(1500.0, 0.0, -900.0), Vec3::Y),
    ]
    .into_iter()
    .map(|(at, normal)| Point {
        at,
        normal,
        space: 0,
        any_side: false,
    })
    .chain([Point {
        at: Vec3::new(1.0, 2.0, 1.0),
        normal: Vec3::Y,
        space: 0,
        any_side: true,
    }])
    .collect();
    let samples = 8192;
    let (rows, brute) = sample(gpu, &lights, &points, samples as u32, eye);
    check(&rows, &brute, &points, samples);
    assert!(brute.iter().all(|b| b.max_element() > 0.0));
}

/// A light behind a one-sided point is never chosen, and a point no light
/// reaches gets no sample.
#[test]
fn lights_behind_a_point_and_out_of_range_give_nothing() {
    let Some(gpu) = gpu() else { return };
    let lights = vec![
        point_light(Vec3::new(0.0, -2.0, 0.0), Vec3::ONE, 50.0, 10.0),
        point_light(Vec3::new(0.0, 3.0, 0.0), Vec3::ONE, 50.0, 10.0),
    ];
    let up = Point { at: Vec3::ZERO, normal: Vec3::Y, space: 0, any_side: false };
    let far = Point { at: Vec3::new(40.0, 0.0, 0.0), normal: Vec3::Y, space: 0, any_side: false };
    let samples = 512;
    let (rows, brute) = sample(gpu, &lights, &[up, far], samples, Vec3::ZERO);
    for s in &rows[..samples as usize] {
        assert!(s.found());
        // The light above, whose id is row 1.
        assert_eq!(s.light & 0x0fff_ffff, 1);
        assert!(Vec3::from(unpack_unit(s.dir)).dot(Vec3::Y) > 0.99);
    }
    assert!(rows[samples as usize..].iter().all(|s| !s.found()));
    assert_eq!(brute[1], Vec3::ZERO);
}

/// Lights in two coordinate spaces at the same place: a point of each space
/// is lit by its own space's light, a point of every space by both.
#[test]
fn a_point_is_lit_by_the_lights_of_its_space() {
    let Some(gpu) = gpu() else { return };
    let lights = vec![
        in_space(point_light(Vec3::new(0.0, 2.0, 0.0), Vec3::new(1.0, 0.0, 0.0), 40.0, 10.0), 0),
        in_space(point_light(Vec3::new(0.0, 2.0, 0.0), Vec3::new(0.0, 0.0, 1.0), 40.0, 10.0), 3),
    ];
    let at = |space| Point { at: Vec3::ZERO, normal: Vec3::Y, space, any_side: false };
    // World (slot 0 + 1), slot 3 + 1, every space (0).
    let points = [at(1), at(4), at(0)];
    let samples = 1024;
    let (rows, brute) = sample(gpu, &lights, &points, samples as u32, Vec3::ZERO);
    check(&rows, &brute, &points, samples);
    let mean = |k: usize| estimate(&rows[k * samples..(k + 1) * samples], &points[k]).0;
    assert!(mean(0).z == 0.0 && mean(0).x > 0.0, "{}", mean(0));
    assert!(mean(1).x == 0.0 && mean(1).z > 0.0, "{}", mean(1));
    assert!(mean(2).x > 0.0 && mean(2).z > 0.0, "{}", mean(2));
}
