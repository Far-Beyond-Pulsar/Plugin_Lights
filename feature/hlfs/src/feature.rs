//! HLFS as a feature: hybrid light-field shading, the `light-field`
//! lighting model ([`LIGHT_FIELD_MODEL`]) and the pipelines that choose it.
//! It asks for its scene colour format when a graph uses it.

use std::sync::Arc;

use helio_feature_api::{pass_node, ExtensionPoint, Feature, FeatureRegistrar, SceneColorFormat};
use helio_frame_api::lighting::{
    lighting_model, Antialiasing, InputsOrder, LightingModel, ModelPipeline, RegisterLightingModel,
    Shading, SpecularLayers,
};
use helio_light_api::LIGHT_FIELD_MODEL;

use crate::HlfsPass;

/// Where the light-field model's pass joins the graph.
pub const POINT: ExtensionPoint = ExtensionPoint("lighting.model.light_field");

/// The light-field pipeline: TSR when the config asks for it, no
/// anti-aliasing otherwise.
pub const HLFS: ModelPipeline = ModelPipeline {
    id: "helio.hlfs",
    label: "HLFS",
    description: "Hybrid light-field shading pipeline.",
    model: LIGHT_FIELD_MODEL,
    aa: Antialiasing::TemporalOrNone,
};

/// The light-field pipeline with FXAA, never TSR.
pub const HLFS_FXAA: ModelPipeline = ModelPipeline {
    id: "helio.fxaa_hlfs",
    label: "HLFS + FXAA",
    description: "Hybrid light-field shading with FXAA.",
    model: LIGHT_FIELD_MODEL,
    aa: Antialiasing::Fxaa,
};

/// Hybrid light-field shading ([`crate`]).
pub struct HlfsFeature;

impl Feature for HlfsFeature {
    fn id(&self) -> &'static str {
        "helio.hlfs"
    }

    fn register(&self, registrar: &mut FeatureRegistrar<'_>) {
        // What shader hot reload may swap without rebuilding the graph.
        registrar.swappable::<HlfsPass>();
        registrar.prepare(Arc::new(|env| {
            if lighting_model(env) == Some(LIGHT_FIELD_MODEL) {
                let format = HlfsPass::preferred_output_format(env.device);
                env.insert_setting(SceneColorFormat(format));
            }
        }));
        // It does not read the specular layers (publishers composite after
        // it), its inputs run before it, and it leaves transparency to TSR's
        // reactive mask.
        registrar.lighting_model(
            LightingModel::new(LIGHT_FIELD_MODEL, Shading::GBuffer)
                .at(POINT)
                .with_specular_layers(SpecularLayers::CompositedAfter)
                .with_inputs_order(InputsOrder::BeforeLighting)
                .with_reactive_transparency()
                .with_pipeline(HLFS)
                .with_pipeline(HLFS_FXAA),
        );
        registrar.node(
            pass_node(
                "hlfs",
                "Lighting: HLFS",
                "Passes: Lighting",
                "Hybrid light-field shading of the GBuffer: the lighting of the HLFS pipelines.",
                |build| {
                    let (iw, ih) = build.env.internal_size();
                    let queue = build.env.queue;
                    let quality = build
                        .env
                        .setting::<helio_light_api::ShadowSettings>()
                        .copied()
                        .unwrap_or_default()
                        .quality;
                    let mut pass =
                        HlfsPass::new(build.env.device, queue, iw, ih, build.env.scene_format());
                    pass.set_shadow_quality(quality, queue);
                    build.graph.add_pass(Box::new(pass));
                    Ok(())
                },
            )
            .at(POINT, 0),
        );
    }
}
