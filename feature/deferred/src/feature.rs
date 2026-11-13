//! Deferred lighting as a feature: the lighting model of the deferred
//! presets (`helio_frame_api::lighting::model::DEFERRED`), lighting
//! the G-buffer with every light, its shadows, the frame's lighting inputs
//! and the IES profiles ([`crate::IesProfiles`]).

use helio_feature_api::ExtensionPoint;
use helio_feature_api::{pass_node, Feature, FeatureRegistrar};
use helio_frame_api::lighting::{
    lighting_settings, model, LightingModel, RegisterLightingModel, Shading,
};
use helio_light_api::ShadowSettings;

use crate::{DeferredLightPass, IesProfiles};

/// Where the deferred model's pass joins the graph.
pub const POINT: ExtensionPoint = ExtensionPoint("lighting.model.deferred");

/// Deferred lighting ([`crate`]).
pub struct DeferredLightingFeature;

impl Feature for DeferredLightingFeature {
    fn id(&self) -> &'static str {
        "helio.lights.deferred"
    }

    fn register(&self, registrar: &mut FeatureRegistrar<'_>) {
        registrar.consumes(helio_light_api::SCENE_LIGHTS);
        // What shader hot reload may swap without rebuilding the graph.
        registrar.swappable::<DeferredLightPass>();
        // The lighting loops per screen tile (the binning's counts), for the
        // performance overlay's tile and shading-cost views.
        registrar.prepare(std::sync::Arc::new(|env| {
            helio_frame_api::diagnostics::offer_tile_counter(
                env,
                helio_frame_api::diagnostics::TileCounter {
                    resource: helio_light_api::TILE_LIGHT_COUNTS,
                    tile_px: helio_light_api::TILE_SIZE,
                },
            );
        }));
        // This pass resolves the G-buffer, reading the specular layers and
        // the frame's inputs, which run before it.
        registrar.lighting_model(LightingModel::new(model::DEFERRED, Shading::GBuffer).at(POINT));
        registrar.node(
            pass_node(
                "deferred_light",
                "Lighting: Deferred",
                "Passes: Lighting",
                "Lights the GBuffer with every light, shadow, reflection and the atmosphere.",
                |build| {
                    let lighting = lighting_settings(build.env);
                    let shadows = build
                        .env
                        .setting::<ShadowSettings>()
                        .copied()
                        .unwrap_or_default();
                    let queue = build.env.queue;
                    let mut pass = DeferredLightPass::new(
                        build.env.device,
                        queue,
                        build.env.camera_buffer,
                        build.env.scene_format(),
                    );
                    pass.set_shadow_quality(shadows.quality, queue);
                    pass.set_ies_profiles(
                        build
                            .env
                            .setting::<IesProfiles>()
                            .map(|profiles| profiles.0.clone()),
                    );
                    pass.debug_mode = lighting.debug_mode;
                    pass.set_env_reflections(lighting.environment_reflections);
                    build.graph.add_pass(Box::new(pass));
                    Ok(())
                },
            )
            .with_marker_after()
            .at(POINT, 0),
        );
    }
}
