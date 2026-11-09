//! The ids of the lighting models this plugin registers
//! (`helio_frame_api::lighting::LightingModel`). The frame names `deferred`
//! and `forward` (`helio_frame_api::lighting::model`); the light-field model is
//! the lights' own, so its id is here for the features that tell it apart.

/// Hybrid light-field shading (HLFS): shades the G-buffer from the lights
/// binned into its own grid, so under it the tiled light binning builds
/// nothing.
pub const LIGHT_FIELD_MODEL: &str = "light-field";
