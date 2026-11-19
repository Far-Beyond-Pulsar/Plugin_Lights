//! The lights' IES profiles and light functions: one texture array every
//! reader of a light's full incidence binds (`light_incidence` in WGSL: the
//! deferred lighting and the lights' sampler), and what stands in for it.

/// The lights' IES profiles (a renderer config's `features`): a texture array
/// (`R8Unorm`, `D2Array`, one layer per profile; a light's
/// `ies_profile_index` or `light_function_index` selects the layer). Without
/// it lights sample [`ies_stand_in`]. See `examples/ies_demo.rs`.
#[derive(Clone, Debug)]
pub struct IesProfiles(pub wgpu::TextureView);

/// Side of [`ies_stand_in`]'s layers.
const STAND_IN_SIZE: u32 = 32;

/// What a reader binds without [`IesProfiles`]: 32x32, two layers. Layer 0
/// a spotlight's profile (bright centre, gaussian falloff), layer 1 a
/// checkerboard light function (alternating 4-texel squares). Every reader
/// makes its own from this one definition, so they sample the same values.
pub fn ies_stand_in(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    let side = STAND_IN_SIZE;
    let mut texels = Vec::with_capacity((side * side * 2) as usize);
    for y in 0..side {
        for x in 0..side {
            let u = (x as f32 + 0.5) / side as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / side as f32 * 2.0 - 1.0;
            let gaussian = (-(u * u + v * v) * 4.0).exp();
            texels.push((gaussian.clamp(0.0, 1.0) * 255.0) as u8);
        }
    }
    for y in 0..side {
        for x in 0..side {
            texels.push(if ((x / 4) + (y / 4)) & 1 == 0 { 255 } else { 40 });
        }
    }
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Lights IES stand-in"),
        size: wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 2,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        &texels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(side),
            rows_per_image: Some(side),
        },
        wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 2,
        },
    );
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

/// The sampler `light_incidence` reads the profiles with (`ies_sampler`):
/// linear, wrapping around the axis, clamped along it.
pub fn ies_sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("Lights IES sampler"),
        min_filter: wgpu::FilterMode::Linear,
        mag_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        ..Default::default()
    })
}
