mod component;
mod mapping;
mod runtime;
mod scene_props;
mod source_row;
mod sub_props;
mod types;

pub use component::{LightComponent, LightComponentGpuMirror};
pub use source_row::{LIGHT_SOURCE_ENABLED_BIT, LIGHT_SOURCES_BUFFER, LightSourceRow};
pub use sub_props::EmitterDiskTexture;

/// The light's emission axis before its owner's world rotation.
pub const LOCAL_LIGHT_DIRECTION: [f32; 3] = [0.0, -1.0, 0.0];
pub use types::{
    IntensityUnits, LightType, MobileQualityLevel, ShadowCacheMode, light_type_to_gpu_u32,
};
