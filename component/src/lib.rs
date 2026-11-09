//! Lights (`pulsar.lights`): the component crate of the plugin that owns
//! lights.
//!
//! - The components: [`LightComponent`] (a directional, point, spot or area
//!   light; its derived source row, [`LightSourceRow`]) and
//!   [`ReflectionCaptureComponent`] (a parallax reflection probe's volume).
//! - Its scene derivation: the lights' join ([`light_join`] over
//!   `helio_pass_light_cull::scene_rows`: the light list, its directional
//!   light and the editor's light icons) and the columns it reads
//!   ([`register_columns`]).
//! - Directional lights are not a level singleton: a level may hold several
//!   (the sky draws each as an emitter; [`directional_lights`] in level order).
//! - The plugin's Helio features ([`render_features`]: the shadow views, the
//!   light binning, the lighting models, the billboards), their settings
//!   ([`configure_renderer`] from [`helio_light_api::SHADOWS`]) and a light
//!   object's editor icon ([`light_icon`]).
//!
//! This crate registers nothing itself: the plugin's root crate
//! (`lights_plugin`) submits the component classes, the join, the features,
//! the settings group, the view icon and the movability, in one place.

mod light;
mod reflection_capture;

pub use light::*;
pub use reflection_capture::*;

use engine_backend::scene::helio_bridge::ObjectJoinContext;
use helio_pass_light_cull::scene_rows::{LightJoin, LightJoinKeys};
use pulsar_scene_model::attachments;
use pulsar_scenedb::gpu::BufferKey;
use pulsar_scenedb::{Entity, World};

/// The light component's class name.
pub const LIGHT_CLASS: &str = "LightComponent";

/// Whether `light`, on an enabled instance, is a directional light that
/// casts.
pub fn is_directional(light: &LightComponent) -> bool {
    light.general.enabled && light.general.light_type == LightType::Directional
}

/// Every casting directional light: the enabled `LightComponent` instances
/// whose light is enabled and directional, in level order (the first is the
/// primary sun of whatever reads one).
pub fn directional_lights(world: &World) -> Vec<Entity> {
    engine_backend::scene::level_rules::attached_in_level_order(
        world,
        world
            .query::<&LightComponent>()
            .filter(|(instance, light)| {
                is_directional(light) && attachments::is_enabled(world, *instance)
            })
            .map(|(instance, _)| instance),
    )
}

/// The lights' join over the engine's rows: the light list from the light
/// instances' source rows ([`LIGHT_SOURCES_BUFFER`]), placed by their
/// owners; in the editor, a light icon a light.
pub fn light_join(
    device: &wgpu::Device,
    context: &ObjectJoinContext,
) -> Box<dyn helio_scene_join::SceneDerivation> {
    Box::new(LightJoin::new(
        device,
        LightJoinKeys {
            owners: context.owners,
            generations: context.generations,
            hidden: context.hidden,
            transforms: context.transforms,
            light_sources: BufferKey::of(LIGHT_SOURCES_BUFFER),
        },
        context.editor,
    ))
}

/// Registers the light source rows' SceneDB columns the join reads.
pub fn register_columns(
    store: &mut pulsar_scenedb::gpu::SceneGpuStore,
    device: &std::sync::Arc<wgpu::Device>,
) {
    LightSourceRow::register_gpu_columns_growable(store, 64, device);
}

/// The lights' features: the shadow views, the light binning, deferred and
/// light-field lighting, and the billboards (the editor's light icons).
pub fn render_features() -> helio_feature_api::FeatureSet {
    helio_feature_api::FeatureSet::new()
        .with(helio_pass_shadow_matrix::ShadowViewsFeature)
        .with(helio_pass_light_cull::LightCullFeature)
        .with(helio_pass_deferred_light::DeferredLightingFeature)
        .with(helio_pass_hlfs::HlfsFeature)
        .with(helio_pass_billboard::BillboardFeature)
}

/// The light join's billboard size: the quad's larger side, in world units
/// per unit of view depth (`scene_join_lights.wgsl`'s billboard scale).
const ICON_SCREEN_SIZE: f32 = 0.04;

/// A light object's editor icon (its billboard, which picking tests): the
/// light image's aspect at the join's billboard size.
pub fn light_icon(world: &World, object: Entity) -> Option<[f32; 2]> {
    attachments::instances(world, object)
        .into_iter()
        .any(|instance| world.get::<LightComponent>(instance).is_some())
        .then(|| {
            let [width, height] = helio_pass_billboard::feature::light_icon_dimensions();
            let scale = ICON_SCREEN_SIZE / width.max(height);
            [width * scale, height * scale]
        })
}

/// The shadows' settings from the project's shadow settings.
pub fn configure_renderer(config: helio::RendererConfig) -> helio::RendererConfig {
    use engine_backend::subsystems::render::helio_renderer::project_settings;
    config.with_feature(helio_light_api::ShadowSettings::from_settings(
        &project_settings::resolved(&helio_light_api::SHADOWS),
    ))
}
