// Light join: one thread per light-instance row. A placed, enabled light is
// copied to the same row of the output with its position at the owner's
// position and its direction the owner's rotation of -Y; every other row
// stays zero (zero color and intensity: no light). Editor billboards are
// written alongside when enabled.

/// `helio_pass_billboard::BillboardComponent` (48 bytes).
struct Billboard {
    world_pos: array<f32, 4>,
    scale_flags: array<f32, 4>,
    color: array<f32, 4>,
}

struct LightJoinUniforms {
    light_rows: u32,
    billboards: u32,
    _pad0: u32,
    _pad1: u32,
    // The frame's world origin (hi + lo), zero outside camera-relative frames.
    origin_hi: vec4<f32>,
    origin_lo: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: LightJoinUniforms;
@group(0) @binding(1) var<storage, read> owners: array<Owner>;
@group(0) @binding(2) var<storage, read> generations: array<u32>;
@group(0) @binding(3) var<storage, read> hidden: array<u32>;
@group(0) @binding(4) var<storage, read> transforms: array<ObjectTransform>;
@group(0) @binding(5) var<storage, read> sources: array<GpuLight>;
@group(0) @binding(6) var<storage, read_write> lights_out: array<GpuLight>;
@group(0) @binding(7) var<storage, read_write> billboards_out: array<Billboard>;
@group(0) @binding(8) var<storage, read> object_spaces: array<u32>;

const ENABLED_BIT: u32 = 0x80000000u;

@compute @workgroup_size(64)
fn cs_join_lights(@builtin(global_invocation_id) gid: vec3<u32>) {
    let row = gid.x;
    if row >= u.light_rows || row >= arrayLength(&sources) || row >= arrayLength(&owners) {
        return;
    }
    var light = sources[row];
    if (light._pad & ENABLED_BIT) == 0u {
        return;
    }
    let owner = owners[row];
    if owner.enabled == 0u {
        return;
    }
    let index = owner.owner_index;
    if index >= arrayLength(&generations) || generations[index] != owner.owner_generation {
        return;
    }
    if index < arrayLength(&hidden) && hidden[index] != 0u {
        return;
    }
    if index >= arrayLength(&transforms) {
        return;
    }
    let t = transforms[index];
    let relative = object_relative(t, u.origin_hi, u.origin_lo);
    // The authored row carries a per-light local direction. This allows
    // multiple LightComponents on one sky object to have independent emitter
    // directions while inheriting the object's world rotation.
    let local_direction = normalize(light.direction_outer.xyz);
    let direction = object_rotation(t) * local_direction;
    light.position_range = vec4<f32>(relative, light.position_range.w);
    light.direction_outer = vec4<f32>(direction, light.direction_outer.w);
    var space = 0u;
    if index < arrayLength(&object_spaces) {
        space = object_spaces[index];
    }
    light._pad = light_flags_with_space(light._pad & ~ENABLED_BIT, space);
    lights_out[row] = light;
    if u.billboards != 0u && row < arrayLength(&billboards_out) {
        billboards_out[row] = Billboard(
            array<f32, 4>(relative.x, relative.y, relative.z, f32(space)),
            array<f32, 4>(0.04, 0.04, 1.0, 0.0),
            array<f32, 4>(light.color_intensity[0], light.color_intensity[1], light.color_intensity[2], 1.0),
        );
    }
}
