//!use light_list
//!use light_incidence
//!use frame_sky
//!use frame_ray_stream
//!use frame_light_samples
//!use sun_transmittance
// The lights' sampler (`helio_pass_light_sampler`, README "Light samples"):
// a world-space light grid built once a frame over the light list, and for
// each point of a consumer's light batch one light chosen among those that
// can reach it in proportion to its unshadowed contribution there, offered
// to the batch's row as `light_incidence` evaluates it (the deferred
// lighting's one evaluation) over the chance it was chosen.
//
// The grid: GRID_LEVELS nested levels of GRID_SIDE^3 cubic cells about the
// eye, level l's cells `base * 2^l` across. A local light lives in the
// finest level whose cells are at least its range (so it covers at most
// 3^3 cells) and whose box holds its whole sphere; directional lights and
// local lights no level holds are the grid's globals. A point is offered
// the globals and the lights of the cell holding it at every level that
// holds it: each light once, every light that can reach it included.

const GRID_LEVELS: u32 = 6u;
const GRID_SIDE: u32 = 32u;
const GRID_GLOBAL: u32 = 0xfffffffeu;
const GRID_SKIP: u32 = 0xffffffffu;
const NO_LIGHT: u32 = 0xffffffffu;

// `eye`: the frame's eye (frame space), w the finest cells' size (m).
// `info`: the light list's rows, then the sampler's number.
struct GridParams {
    eye: vec4<f32>,
    info: vec4<u32>,
}

@group(0) @binding(0) var<uniform> grid: GridParams;
@group(0) @binding(1) var<storage, read> lights: array<GpuLight>;

fn grid_cell_size(level: u32) -> f32 {
    return grid.eye.w * exp2(f32(level));
}

// Level `level`'s lowest corner (frame space), on its cells' lattice.
fn grid_origin(level: u32) -> vec3<f32> {
    let c = grid_cell_size(level);
    return (floor(grid.eye.xyz / c) - f32(GRID_SIDE / 2u)) * c;
}

// The level `light` lives in, `GRID_GLOBAL`, or `GRID_SKIP` (it gives no
// light).
fn grid_level_of(light: GpuLight) -> u32 {
    if light.color_intensity.w <= 0.0 || all(light.color_intensity.rgb <= vec3<f32>(0.0)) { return GRID_SKIP; }
    if light.light_type == LIGHT_DIRECTIONAL { return GRID_GLOBAL; }
    let r = light.position_range.w;
    if !(r > 0.0) { return GRID_SKIP; }
    let p = light.position_range.xyz;
    for (var level = 0u; level < GRID_LEVELS; level++) {
        let c = grid_cell_size(level);
        if r > c { continue; }
        let lo = grid_origin(level);
        let hi = lo + c * f32(GRID_SIDE);
        if all(p - r >= lo) && all(p + r <= hi) { return level; }
    }
    return GRID_GLOBAL;
}

fn grid_cell_index(level: u32, cell: vec3<u32>) -> u32 {
    return ((level * GRID_SIDE + cell.z) * GRID_SIDE + cell.y) * GRID_SIDE + cell.x;
}

// The cells a light of level `level` covers: lowest and highest.
fn grid_cells_of(light: GpuLight, level: u32) -> array<vec3<u32>, 2> {
    let c = grid_cell_size(level);
    let lo = grid_origin(level);
    let r = light.position_range.w;
    let top = f32(GRID_SIDE - 1u);
    let a = clamp(floor((light.position_range.xyz - r - lo) / c), vec3<f32>(0.0), vec3<f32>(top));
    let b = clamp(floor((light.position_range.xyz + r - lo) / c), vec3<f32>(0.0), vec3<f32>(top));
    return array<vec3<u32>, 2>(vec3<u32>(a), vec3<u32>(b));
}

// ── The grid's build (one dispatch each: count, allocate, fill) ─────────────

@group(1) @binding(0) var<storage, read_write> cell_counts: array<atomic<u32>>;
@group(1) @binding(1) var<storage, read_write> cell_offsets: array<u32>;
@group(1) @binding(2) var<storage, read_write> cell_lights: array<u32>;
// Word 0: how many globals, then their rows.
@group(1) @binding(3) var<storage, read_write> grid_globals: array<atomic<u32>>;
// Word 0: entries allocated so far.
@group(1) @binding(4) var<storage, read_write> grid_alloc: array<atomic<u32>, 4>;

// Each light's cells counted (and the globals listed).
@compute @workgroup_size(64)
fn grid_count(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= grid.info.x { return; }
    let light = lights[id.x];
    let level = grid_level_of(light);
    if level == GRID_SKIP { return; }
    if level == GRID_GLOBAL {
        let at = atomicAdd(&grid_globals[0], 1u);
        atomicStore(&grid_globals[1u + at], id.x);
        return;
    }
    let cells = grid_cells_of(light, level);
    for (var z = cells[0].z; z <= cells[1].z; z++) {
        for (var y = cells[0].y; y <= cells[1].y; y++) {
            for (var x = cells[0].x; x <= cells[1].x; x++) {
                atomicAdd(&cell_counts[grid_cell_index(level, vec3<u32>(x, y, z))], 1u);
            }
        }
    }
}

// Each cell's place in the entries; its count reset for the fill. A light
// covers at most 27 cells, so 27 entries a row always suffice.
@compute @workgroup_size(64)
fn grid_allocate(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= GRID_LEVELS * GRID_SIDE * GRID_SIDE * GRID_SIDE { return; }
    let count = atomicLoad(&cell_counts[id.x]);
    var offset = 0u;
    if count > 0u { offset = atomicAdd(&grid_alloc[0], count); }
    cell_offsets[id.x] = offset;
    atomicStore(&cell_counts[id.x], 0u);
}

// Each light's row into its cells.
@compute @workgroup_size(64)
fn grid_fill(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= grid.info.x { return; }
    let light = lights[id.x];
    let level = grid_level_of(light);
    if level >= GRID_LEVELS { return; }
    let cells = grid_cells_of(light, level);
    for (var z = cells[0].z; z <= cells[1].z; z++) {
        for (var y = cells[0].y; y <= cells[1].y; y++) {
            for (var x = cells[0].x; x <= cells[1].x; x++) {
                let cell = grid_cell_index(level, vec3<u32>(x, y, z));
                cell_lights[cell_offsets[cell] + atomicAdd(&cell_counts[cell], 1u)] = id.x;
            }
        }
    }
}

// ── A batch's samples ───────────────────────────────────────────────────────

@group(2) @binding(0) var<storage, read> grid_counts: array<u32>;
@group(2) @binding(1) var<storage, read> grid_offsets: array<u32>;
@group(2) @binding(2) var<storage, read> grid_lights: array<u32>;
@group(2) @binding(3) var<storage, read> globals: array<u32>;
@group(2) @binding(4) var<storage, read> batch_header: RayStreamHeader;
@group(2) @binding(5) var<storage, read> queries: array<LightQuery>;
@group(2) @binding(6) var<storage, read_write> samples: array<LightSample>;
@group(2) @binding(7) var<uniform> sky_frame: SkyFrame;
@group(2) @binding(8) var ies_textures: texture_2d_array<f32>;
@group(2) @binding(9) var ies_sampler: sampler;
@group(2) @binding(10) var sun_transmittance_map: texture_2d<f32>;
@group(2) @binding(11) var<uniform> sun_transmittance_frame: SunTransmittanceFrame;

// A weighted reservoir over the candidates: their summed weight, the one
// kept and its weight, the candidates seen.
struct Choice {
    total: f32,
    light: u32,
    weight: f32,
    seen: u32,
}

// What light `index` can bring to query `q`, before anything between them
// and its profile, light function and air (each at most one): its falloff
// (`light_arrival`) times the cosine on the side the point is lit from (one
// for a point lit from every side). Zero for a light the query's space does
// not see.
fn candidate_weight(q: LightQuery, light: GpuLight) -> f32 {
    if !light_query_sees_space(q, light_space(light)) { return 0.0; }
    let reach = light_arrival(light, q.position);
    var cosine = 1.0;
    if (q.space & LIGHT_QUERY_ANY_SIDE) == 0u {
        cosine = max(dot(scene_unpack_unit(q.normal), reach.L), 0.0);
    }
    return light_luminance(reach.radiance) * cosine;
}

fn consider(c: Choice, q: LightQuery, index: u32) -> Choice {
    let w = candidate_weight(q, lights[index]);
    if !(w > 0.0) { return c; }
    var out = c;
    out.total += w;
    out.seen += 1u;
    // The sampler's own choices draw from 16 up: below are the samplers'.
    if light_query_uniform(q, 16u + c.seen) * out.total < w {
        out.light = index;
        out.weight = w;
    }
    return out;
}

@compute @workgroup_size(64)
fn light_sample(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= batch_header.count { return; }
    let q = queries[id.x];
    let p = q.position;
    var c = Choice(0.0, NO_LIGHT, 0.0, 0u);
    for (var g = 0u; g < globals[0]; g++) {
        c = consider(c, q, globals[1u + g]);
    }
    for (var level = 0u; level < GRID_LEVELS; level++) {
        let cell = floor((p - grid_origin(level)) / grid_cell_size(level));
        if any(cell < vec3<f32>(0.0)) || any(cell >= vec3<f32>(f32(GRID_SIDE))) { continue; }
        let at = grid_cell_index(level, vec3<u32>(cell));
        let first = grid_offsets[at];
        for (var k = 0u; k < grid_counts[at]; k++) {
            c = consider(c, q, grid_lights[first + k]);
        }
    }
    if c.light == NO_LIGHT { return; }
    let light = lights[c.light];
    var incidence = light_incidence(light, p, grid.eye.xyz);
    // The sun through the media above (the clouds' transmittance), as the
    // deferred lighting takes it through the sun's visibility.
    if light.light_type == LIGHT_DIRECTIONAL && dot(-sun_transmittance_frame.forward.xyz, incidence.L) > 0.99999 {
        incidence.radiance *= sun_transmittance_at(p - grid.eye.xyz);
    }
    let distance = select(incidence.distance, LIGHT_SAMPLE_FAR, light.light_type == LIGHT_DIRECTIONAL);
    let sampler_id = grid.info.y;
    let offer = light_sample_of(incidence.radiance * (c.total / c.weight), incidence.L, distance, 0.0, sampler_id, c.light);
    samples[id.x] = light_sample_offer(samples[id.x], offer, q, sampler_id);
}
