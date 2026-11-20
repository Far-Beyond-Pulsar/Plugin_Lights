//! The lights' public contract (`pulsar.lights`): what another kind reads
//! of the lights, and the only place it reads it from.
//!
//! - the light list ([`LIGHT_LIST`], [`GpuLight`]) and the directional
//!   lights published with it ([`DIRECTIONAL_LIGHTS`]);
//! - the screen-tile binning ([`binning`]: [`ClusterLightGrid`]);
//! - the shadow views ([`shadows`]): their per-face matrices, atlas budget,
//!   settings ([`ShadowSettings`], host group [`SHADOWS`]) and the
//!   per-caster dirty flags what casts into them reads;
//! - the sun's occlusion by terrain that does not draw into the shadow maps
//!   ([`sun_occlusion`]), which such a terrain publishes and media read;
//! - the IES profiles and light functions ([`IesProfiles`], the stand-in
//!   [`ies_stand_in`]);
//! - in WGSL: [`LIGHT_LIST_SNIPPET`] (`//!use light_list`, with the one
//!   falloff `light_arrival`), [`LIGHT_INCIDENCE_SNIPPET`]
//!   (`//!use light_incidence`, a light's full incidence at a point),
//!   [`LIGHT_FORWARD_SNIPPET`] (`//!use light_forward`, the forward light
//!   function), [`SHADOW_ATLAS_SNIPPET`] (`//!use shadow_atlas`),
//!   [`SHADOW_LOOKUP_SNIPPET`] (`//!use shadow_lookup`) and
//!   [`SUN_OCCLUSION_SNIPPET`] (`//!use sun_occlusion`).
//!
//! No passes: a kind that reads lights depends on this crate, never on the
//! lights' feature crates. How others draw the line: Bevy's `bevy_pbr`
//! exports its light and cluster structs and `#import`able WGSL to any
//! crate shading against them; Unreal's light data is one renderer-owned
//! definition (`LightData.ush`) every shader includes. Here the owner is
//! the lights plugin and the import is its snippet, so a change to a row
//! lands in one file and every reader compiles against it. With
//! `host-rows`, the light list's row as a SceneDB component for standalone
//! hosts ([`host_rows`]).

pub mod binning;
mod ies;
mod light;
mod models;
mod settings;
pub mod shadows;
pub mod sun_occlusion;

pub use binning::*;
pub use ies::{ies_sampler, ies_stand_in, IesProfiles};
pub use light::*;
pub use models::LIGHT_FIELD_MODEL;
pub use settings::SHADOWS;
pub use shadows::*;
pub use sun_occlusion::*;

#[cfg(feature = "host-rows")]
pub mod host_rows;

/// The light list's row in WGSL (`//!use light_list`): `GpuLight`, the
/// `LIGHT_*` kinds, `LIGHT_NO_SHADOW`, the binning's `LIGHT_TILE_SIZE` /
/// `LIGHT_MAX_PER_TILE`, `DirectionalLight`, `light_shadow_requested`,
/// `light_is_sun`, `light_tile_index`.
pub const LIGHT_LIST_SNIPPET: helio_core::shader::ShaderSnippet =
    helio_core::wgsl_snippet!("//!use light_list", "shaders/light_list.wgsl");

/// A light's full incidence at a point (`//!use light_incidence`, with
/// `light_list` and the frame's `frame_sky`): `light_incidence(light, p,
/// eye)`, the falloff (`light_arrival`), the IES profile, the light function
/// and the air a directional light crosses. The deferred lighting and the
/// lights' sampler both call it. The reader binds `ies_textures`,
/// `ies_sampler` and `sky_frame`.
pub const LIGHT_INCIDENCE_SNIPPET: helio_core::shader::ShaderSnippet =
    helio_core::wgsl_snippet!("//!use light_incidence", "shaders/light_incidence.wgsl");

/// The forward light function (`//!use light_forward`, with `light_list`
/// and the frame's `frame_surface` and `frame_bsdf`): `light_forward(frag_xy, num_tiles_x, ...)` sums the
/// direct light of the pixel's tile, for drawers that light what they draw.
pub const LIGHT_FORWARD_SNIPPET: helio_core::shader::ShaderSnippet =
    helio_core::wgsl_snippet!("//!use light_forward", "shaders/light_forward.wgsl");

/// The shadow atlas (`//!use shadow_atlas`): `LightMatrix`,
/// `point_light_face`, the budgeted atlas's addressing (`budget_*`) and
/// sampling (`budget_compare`, `budget_depth`, `budget_glass_tint`).
pub const SHADOW_ATLAS_SNIPPET: helio_core::shader::ShaderSnippet =
    helio_core::wgsl_snippet!("//!use shadow_atlas", "shaders/shadow_atlas.wgsl");

/// A shadow lookup outside the lights' passes (`//!use shadow_lookup`, over
/// the prelude and [`SHADOW_ATLAS_SNIPPET`]): cascade selection
/// (`shadow_csm_select`) and projection into a face (`shadow_project`).
pub const SHADOW_LOOKUP_SNIPPET: helio_core::shader::ShaderSnippet =
    helio_core::wgsl_snippet!("//!use shadow_lookup", "shaders/shadow_lookup.wgsl");
