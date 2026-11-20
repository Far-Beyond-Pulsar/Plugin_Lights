// The lights' forward light function (`helio_light_api`, `//!use
// light_forward`; needs `//!use light_list`, `//!use frame_surface` and
// `//!use frame_bsdf`): the direct light a surface point receives from the
// lights of its screen tile, for drawers that light what they draw (the
// forward and transparent phases). The drawer binds the light list as
// `lights: array<GpuLight>` and the binning as `tile_light_lists` /
// `tile_light_counts` (both `array<u32>`), which the lights publish.

// One light's direct radiance at `world_pos`: its falloff (`light_arrival`)
// times the surface's BSDF (`bsdf_eval`).
fn light_forward_one(
    light:     GpuLight,
    world_pos: vec3<f32>,
    N:         vec3<f32>,
    V:         vec3<f32>,
    F0:        vec3<f32>,
    albedo:    vec3<f32>,
    roughness: f32,
    metallic:  f32,
) -> vec3<f32> {
    let incidence = light_arrival(light, world_pos);
    // Light too faint to see is skipped (as the deferred lighting does).
    if all(incidence.radiance < vec3<f32>(0.002)) { return vec3<f32>(0.0); }
    var surface = surface_data(albedo, N, roughness, metallic, vec3<f32>(0.0));
    surface.specular_f0 = F0;
    return bsdf_eval(surface, V, incidence.L) * incidence.radiance;
}

// The direct light at `world_pos` (seen from `V`, normal `N`) from every
// light of the tile holding pixel `frag_xy`, `num_tiles_x` tiles a row.
fn light_forward(
    frag_xy:     vec2<f32>,
    num_tiles_x: u32,
    world_pos:   vec3<f32>,
    N:           vec3<f32>,
    V:           vec3<f32>,
    F0:          vec3<f32>,
    albedo:      vec3<f32>,
    roughness:   f32,
    metallic:    f32,
) -> vec3<f32> {
    let tile = light_tile_index(frag_xy, num_tiles_x);
    let count = min(tile_light_counts[tile], LIGHT_MAX_PER_TILE);
    var lit = vec3<f32>(0.0);
    for (var i = 0u; i < count; i++) {
        let light = lights[tile_light_lists[tile * LIGHT_MAX_PER_TILE + i]];
        lit += light_forward_one(light, world_pos, N, V, F0, albedo, roughness, metallic);
    }
    return lit;
}
