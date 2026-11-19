// How a light reaches a point, in full (`helio_light_api`, `//!use
// light_incidence`, with `//!use light_list` and `//!use frame_sky`): the
// light's own falloff (`light_arrival`), its IES profile and its light
// function (gobo), and for a directional light of the world the air between
// the point and space. What a lighting model multiplies by the surface's
// BSDF, shadow and visibility, and what the lights' sampler offers: one
// function, so a pixel's light and a sampled light are the same.
//
// The reader binds the profiles as `ies_textures` (texture_2d_array<f32>,
// R8Unorm, `helio_light_api::IesProfiles` or the stand-in
// `helio_light_api::ies_stand_in`) with `ies_sampler`, and the frame's sky as
// `sky_frame` (a zeroed frame without one).

// `light`'s incidence at frame point `p`, the eye at frame point `eye` (the
// sky frame is the eye's).
fn light_incidence(light: GpuLight, p: vec3<f32>, eye: vec3<f32>) -> LightIncidence {
    var r = light_arrival(light, p);
    if light.light_type == LIGHT_DIRECTIONAL {
        // A distant light crosses the atmosphere, if any, to reach the point.
        if light_space(light) == 0u {
            r.radiance *= sky_transmittance_towards(sky_frame, (p - eye) * 0.001, r.L);
        }
        return r;
    }
    if all(r.radiance <= vec3<f32>(0.0)) { return r; }
    let L = r.L;
    let axis = normalize(light.direction_outer.xyz);
    // IES profile: the angular distribution about the light's axis.
    if light.ies_profile_index >= 0 {
        let theta = acos(clamp(dot(-L, axis), -1.0, 1.0));
        let across = cross(-L, axis);
        let phi = atan2(across.x, across.y);
        let uv = vec2<f32>(phi / 6.2831853 + 0.5, theta * 0.6366198) * light.ies_angle_scale
            + vec2<f32>(light.ies_angle_offset / 360.0, 0.0);
        r.radiance *= textureSampleLevel(ies_textures, ies_sampler, uv, u32(light.ies_profile_index), 0.0).r;
    }
    // Light function (gobo / cookie): projected along the axis over the
    // light's range.
    if light.light_function_index >= 0 {
        let from_light = p - light.position_range.xyz;
        let projected = from_light - dot(from_light, axis) * axis;
        let up_ref = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), abs(dot(axis, vec3<f32>(0.0, 1.0, 0.0))) > 0.99);
        let right = normalize(cross(axis, up_ref));
        let up = cross(right, axis);
        let uv = vec2<f32>(dot(projected, right), dot(projected, up)) / (light.position_range.w * 0.5) + 0.5;
        r.radiance *= textureSampleLevel(ies_textures, ies_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), u32(light.light_function_index), 0.0).r;
    }
    return r;
}
