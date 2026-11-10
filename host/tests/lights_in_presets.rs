//! The lights in Helio's presets, as a standalone host adds them: hundreds
//! of shadow requests page into the budgeted atlas across frames (the
//! residency; what casts into it, and its update budget, is the meshes'
//! test), and the billboard pass joins once a billboard row is there.
//! (Moved from Helio's preset tests with the lights.)

use std::sync::Arc;

use glam::Vec3;
use helio::{Camera, RendererBuilder, RendererConfig};
use helio_light_host::{BillboardComponent, LightComponent};

fn device() -> Option<(Arc<wgpu::Device>, Arc<wgpu::Queue>)> {
    let instance = wgpu::Instance::new(helio_core::wgpu_instance_descriptor());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .ok()?;
    let features = helio::required_wgpu_features(helio::usable_adapter_features(&adapter));
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: features,
        required_limits: helio::required_wgpu_limits(adapter.limits()),
        experimental_features: helio::required_experimental_features(features),
        ..Default::default()
    }))
    .ok()?;
    Some((Arc::new(device), Arc::new(queue)))
}

/// A SceneDB with a mirror and the light list's and billboards' columns
/// registered up front, as hosts do.
fn scene_db(device: &Arc<wgpu::Device>, queue: &Arc<wgpu::Queue>) -> pulsar_scenedb::SceneDb {
    let mut scene_db = pulsar_scenedb::SceneDb::new();
    let ctx = pulsar_scenedb::gpu::EngineGpuContext::new(device.clone(), queue.clone());
    let mut store = pulsar_scenedb::gpu::SceneGpuStore::new(
        &ctx,
        pulsar_scenedb::gpu::SceneGpuConfig {
            classes: Vec::new(),
            tombstone_headroom: 0,
            max_cells_metadata: 0,
        },
    );
    helio_light_host::register_columns(&mut store, device);
    // Registered before any billboard exists: the column alone must not
    // wake the billboard pass.
    BillboardComponent::register_gpu_columns_growable(&mut store, 64, device);
    scene_db
        .world
        .attach_gpu_mirror(pulsar_scenedb::gpu::GpuMirrorHandle::new(
            Arc::new(store),
            queue.clone(),
        ));
    scene_db
}

fn renderer(
    device: &Arc<wgpu::Device>,
    queue: &Arc<wgpu::Queue>,
    scene: &pulsar_scenedb::SceneDb,
    size: (u32, u32),
) -> helio::Renderer {
    let config = RendererConfig::new(size.0, size.1, wgpu::TextureFormat::Rgba8Unorm);
    RendererBuilder::new(config.clone(), scene.world.gpu_mirror().cloned().unwrap())
        .with_pass_build_context(Box::new(|ctx| {
            helio_presets::Preset::Deferred
                .with_features(&helio_light_host::features())
                .build(ctx)
        }))
        .with_external_device()
        .build(
            device.clone(),
            queue.clone(),
            config.width,
            config.height,
            config.surface_format,
        )
}

fn target(device: &wgpu::Device, size: (u32, u32)) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

fn camera() -> Camera {
    Camera::perspective_look_at(
        Vec3::new(0.0, 1.0, 3.0),
        Vec3::ZERO,
        Vec3::Y,
        60.0_f32.to_radians(),
        1.0,
        0.1,
        100.0,
    )
}

#[test]
fn budgeted_shadows_render_hundreds_of_requests_across_frames() {
    let Some((device, queue)) = device() else {
        eprintln!("GPU_VALIDATION_SKIPPED_NO_ADAPTER: budgeted shadows");
        return;
    };
    let errors = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut scene = scene_db(&device, &queue);
    for i in 0..200 {
        let entity = scene.world.spawn();
        let light = helio_light_api::GpuLight {
            position_range: [(i % 20) as f32 * 0.2 - 2.0, 1.0, (i / 20) as f32 * 0.2, 3.0],
            direction_outer: [0.0, -1.0, 0.0, 0.7],
            shadow_index: 0,
            light_type: 2,
            ..Default::default()
        };
        scene.world.insert(
            entity,
            LightComponent::from(helio_light_api::LightRow::from(light)),
        );
    }
    let mut renderer = renderer(&device, &queue, &scene, (32, 32));
    let view = target(&device, (32, 32));
    let camera = camera();
    for _ in 0..17 {
        scene.world.flush_gpu_mirror(&queue);
        renderer
            .render(&camera, &view)
            .expect("budgeted shadow frame");
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    }
    let residency = renderer
        .find_pass::<helio_pass_shadow_matrix::ShadowMatrixPass>()
        .expect("the lights' shadow views are in the preset")
        .residency();
    assert!(
        residency
            .residents
            .iter()
            .filter(|r| r.owner != 0 && r.tiles.iter().any(|t| t.size > 0))
            .count()
            > 42
    );
    let error = pollster::block_on(errors.pop());
    assert!(error.is_none(), "{error:?}");
}

#[test]
fn billboards_join_with_their_rows() {
    let Some((device, queue)) = device() else {
        eprintln!("GPU_VALIDATION_SKIPPED_NO_ADAPTER: billboards");
        return;
    };
    let mut scene = scene_db(&device, &queue);
    let mut renderer = renderer(&device, &queue, &scene, (64, 48));
    let view = target(&device, (64, 48));
    let camera = camera();
    let mut frames =
        |renderer: &mut helio::Renderer, scene: &mut pulsar_scenedb::SceneDb, count| {
            for _ in 0..count {
                scene.world.flush_gpu_mirror(&queue);
                renderer.render(&camera, &view).unwrap();
                // Row readbacks complete between frames.
                device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            }
        };
    frames(&mut renderer, &mut scene, 4);
    assert!(!renderer.graph_members().contains(&"Billboard"));
    let billboard = scene.world.spawn();
    scene.world.insert(
        billboard,
        BillboardComponent {
            world_pos: [0.0, 1.0, 0.0, 0.0],
            scale_flags: [1.0, 1.0, 0.0, 0.0],
            color: [1.0, 0.0, 0.0, 1.0],
        },
    );
    frames(&mut renderer, &mut scene, 4);
    assert!(
        renderer.graph_members().contains(&"Billboard"),
        "{:?}",
        renderer.graph_members()
    );
}

/// The light-field model, as a preset: `Preset::Model(HLFS)` and
/// `HLFS_FXAA` (the pipelines the lights register; Helio has no HLFS preset
/// of its own) build, render and read what the model registered. Ported from
/// Helio's deleted `the_light_field_preset_writes_one_scene_format`
/// (Pulsar-Native#1652); the model's passes and energy are `gpu_hlfs.rs`'s.
///
/// Fails when the model's pipelines regress: not listed, not resolving to the
/// light-field model (the spine's unlit stand-in drawn instead), no HLFS pass,
/// the wrong anti-aliasing for its pipeline, or a scene colour format that
/// two passes disagree on (wgpu validation rejects it at build or render).
fn light_field_renderer(
    device: &Arc<wgpu::Device>,
    queue: &Arc<wgpu::Queue>,
    scene: &pulsar_scenedb::SceneDb,
    pipeline: helio_frame_api::lighting::ModelPipeline,
    features: helio_feature_api::FeatureSet,
    tsr: bool,
) -> helio::Renderer {
    let mut config = RendererConfig::new(64, 48, wgpu::TextureFormat::Rgba8Unorm);
    if tsr {
        config = config.with_tsr_quality(helio::TsrQuality::Quality);
    }
    RendererBuilder::new(config.clone(), scene.world.gpu_mirror().cloned().unwrap())
        .with_pass_build_context(Box::new(move |ctx| {
            helio_presets::Preset::Model(pipeline)
                .with_features(&features)
                .build(ctx)
        }))
        .with_external_device()
        .build(
            device.clone(),
            queue.clone(),
            config.width,
            config.height,
            config.surface_format,
        )
}

#[test]
fn the_light_field_pipelines_build_and_render_on_their_model() {
    use helio_pass_hlfs::HlfsPass;
    let Some((device, queue)) = device() else {
        eprintln!("GPU_VALIDATION_SKIPPED_NO_ADAPTER: light-field preset");
        return;
    };

    // Both pipelines are listed in a host's registry.
    let features = helio_light_host::features();
    let mut listed = helio::RenderPipelineRegistry::new();
    helio_presets::pipelines::register_builtin_pipelines(&mut listed, &features);
    for pipeline in [helio_light_host::HLFS, helio_light_host::HLFS_FXAA] {
        assert_eq!(listed.resolve(Some(pipeline.id)), Some(pipeline.id));
    }

    let view = target(&device, (64, 48));
    let camera = camera();
    for (pipeline, tsr) in [
        (helio_light_host::HLFS, false),
        (helio_light_host::HLFS, true),
        (helio_light_host::HLFS_FXAA, false),
    ] {
        let label = format!("{} tsr={tsr}", pipeline.id);
        let errors = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut scene = scene_db(&device, &queue);
        let entity = scene.world.spawn();
        scene.world.insert(
            entity,
            LightComponent::from(helio_light_api::LightRow::from(helio_light_api::GpuLight {
                position_range: [0.0, 1.0, 0.0, 4.0],
                direction_outer: [0.0, -1.0, 0.0, 0.7],
                light_type: 2,
                ..Default::default()
            })),
        );
        let mut renderer = light_field_renderer(
            &device,
            &queue,
            &scene,
            pipeline,
            helio_light_host::features(),
            tsr,
        );
        // Under TSR the graph reads the transparency reactive mask, which the
        // meshes' transparency draws write: a host without them validates the
        // graph (built above) but cannot run a frame.
        let frames = if tsr { 0 } else { 3 };
        for _ in 0..frames {
            scene.world.flush_gpu_mirror(&queue);
            renderer.render(&camera, &view).expect("light-field frame");
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        }
        // The light-field model's pass lights it, not the stand-in.
        assert!(renderer.find_pass::<HlfsPass>().is_some(), "{label}");
        assert!(!renderer.graph_members().contains(&"Unlit"), "{label}");
        // Its anti-aliasing is its pipeline's: TSR when asked and temporal,
        // FXAA for the FXAA pipeline, none otherwise.
        assert_eq!(
            renderer.find_pass::<helio_pass_tsr::TsrPass>().is_some(),
            tsr && pipeline.id == helio_light_host::HLFS.id,
            "{label}"
        );
        assert_eq!(
            renderer.find_pass::<helio_pass_fxaa::FxaaPass>().is_some(),
            pipeline.id == helio_light_host::HLFS_FXAA.id,
            "{label}"
        );
        let error = pollster::block_on(errors.pop());
        assert!(error.is_none(), "{label}: {error:?}");
    }

    // Without the model registered the same preset draws the stand-in:
    // what the assertions above would see if the registration regressed.
    let scene = scene_db(&device, &queue);
    let mut renderer = light_field_renderer(
        &device,
        &queue,
        &scene,
        helio_light_host::HLFS,
        helio_feature_api::FeatureSet::new(),
        false,
    );
    renderer.render(&camera, &view).expect("unlit frame");
    assert!(renderer.find_pass::<HlfsPass>().is_none());
    assert!(renderer.graph_members().contains(&"Unlit"));
}
