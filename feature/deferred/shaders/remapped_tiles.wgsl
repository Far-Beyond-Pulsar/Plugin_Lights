//!use helio_prelude
//!use light_list

@group(0) @binding(0) var<storage, read> cameras: array<Camera, 2>;
@group(0) @binding(1) var<storage, read> lights: array<GpuLight>;
@group(0) @binding(2) var<storage, read> active_lights: array<u32>;
@group(0) @binding(3) var surface_tags: texture_2d<f32>;
@group(0) @binding(4) var depth_tex: texture_depth_2d;
@group(0) @binding(5) var<uniform> view_remaps: HelioViewRemaps;
@group(0) @binding(6) var<uniform> remap_spaces: HelioViewRemapSpaces;
@group(0) @binding(7) var<storage, read_write> tile_lights: array<u32>;

const LANES: u32 = 64u;
var<workgroup> lower: array<vec3<f32>, LANES>;
var<workgroup> upper: array<vec3<f32>, LANES>;
var<workgroup> domains: array<atomic<u32>, 8>;
var<workgroup> count: atomic<u32>;

@compute @workgroup_size(64)
fn bin_remapped_tiles(
    @builtin(workgroup_id) tile: vec3<u32>,
    @builtin(local_invocation_index) lane: u32,
) {
    let size = textureDimensions(depth_tex);
    let tiles_x = (size.x + LIGHT_TILE_SIZE - 1u) / LIGHT_TILE_SIZE;
    let base = (tile.y * tiles_x + tile.x) * (LIGHT_MAX_PER_TILE + 1u);
    if base + LIGHT_MAX_PER_TILE >= arrayLength(&tile_lights) {
        return;
    }
    if lane < 8u { atomicStore(&domains[lane], 0u); }
    if lane == 0u { atomicStore(&count, 0u); }
    workgroupBarrier();

    var lo = vec3<f32>(3.402823e38);
    var hi = -lo;
    for (var i = lane; i < LIGHT_TILE_SIZE * LIGHT_TILE_SIZE; i += LANES) {
        let p = tile.xy * LIGHT_TILE_SIZE + vec2<u32>(i % LIGHT_TILE_SIZE, i / LIGHT_TILE_SIZE);
        if any(p >= size) { continue; }
        let remap = gbuffer_remap_of(textureLoad(surface_tags, vec2<i32>(p), 0).xy);
        if remap < 0 || remap >= 1024 { continue; }
        let depth = textureLoad(depth_tex, vec2<i32>(p), 0);
        if helio_is_sky(depth) { continue; }
        let uv = (vec2<f32>(p) + 0.5) / vec2<f32>(size);
        let h = cameras[0].view_proj_inv * vec4<f32>(helio_uv_to_ndc(uv), depth, 1.0);
        let position = (view_remaps[remap] * vec4<f32>(h.xyz / h.w, 1.0)).xyz;
        lo = min(lo, position);
        hi = max(hi, position);
        let r = u32(remap);
        let space = remap_spaces[r / 4u][r % 4u];
        atomicOr(&domains[space / 32u], 1u << (space % 32u));
    }
    lower[lane] = lo;
    upper[lane] = hi;
    workgroupBarrier();
    for (var stride = LANES / 2u; stride > 0u; stride /= 2u) {
        if lane < stride {
            lower[lane] = min(lower[lane], lower[lane + stride]);
            upper[lane] = max(upper[lane], upper[lane + stride]);
        }
        workgroupBarrier();
    }
    if all(lower[0] <= upper[0]) {
        for (var i = lane; i < active_lights[0]; i += LANES) {
            let light_id = active_lights[i + 1u];
            let light = lights[light_id];
            let space = light_space(light);
            if (atomicLoad(&domains[space / 32u]) & (1u << (space % 32u))) == 0u { continue; }
            if light.light_type != LIGHT_DIRECTIONAL {
                let p = light.position_range.xyz;
                let delta = p - clamp(p, lower[0], upper[0]);
                if dot(delta, delta) > light.position_range.w * light.position_range.w { continue; }
            }
            let slot = atomicAdd(&count, 1u);
            if slot < LIGHT_MAX_PER_TILE {
                tile_lights[base + 1u + slot] = light_id;
            }
        }
    }
    workgroupBarrier();
    if lane == 0u { tile_lights[base] = atomicLoad(&count); }
}
