//!use helio_prelude
//!use portal_chain
//! Billboard shader - camera-facing instanced quads
//!
//! The vertex index supplies the quad and the instance index selects the row.

struct Globals {
    instance_count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}
@group(0) @binding(0) var<storage, read> cameras: array<Camera, 2>;
@group(0) @binding(1) var<uniform> globals: Globals;
@group(0) @binding(2) var<storage, read> instances: array<BillboardInstance>;

// Group 1: sprite texture
@group(1) @binding(0) var sprite_tex:     texture_2d<f32>;
@group(1) @binding(1) var sprite_sampler: sampler;

@group(2) @binding(0) var<storage, read> chains: array<GpuPortalChain>;
@group(2) @binding(1) var<storage, read> stages: array<GpuPortalStage>;
@group(2) @binding(2) var aperture_mask: texture_2d<u32>;

struct BillboardInstance {
    world_pos_pad: vec4<f32>,
    scale_flags:   vec4<f32>,
    color:         vec4<f32>,
}

// ── Vertex output ───────────────────────────────────────────────────────────

struct VertexOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0)       uv:       vec2<f32>,
    @location(1)       color:    vec4<f32>,
    @location(2)       drawn_position: vec3<f32>,
    @location(3) @interpolate(flat) occurrence: u32,
}

// ── Vertex shader ───────────────────────────────────────────────────────────

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32, @builtin(instance_index) instance_index: u32) -> VertexOut {
    let inst = instances[instance_index];
    return billboard_vertex(vertex_index, inst, inst.world_pos_pad.xyz, 0u);
}

@vertex
fn vs_portal(@builtin(vertex_index) vertex_index: u32, @builtin(instance_index) instance_index: u32) -> VertexOut {
    // Blend the direct foreground over the occurrences behind the aperture.
    let occurrence = (instance_index / globals.instance_count + 1u) % (arrayLength(&chains) + 1u);
    let inst = instances[instance_index % globals.instance_count];
    var center = inst.world_pos_pad.xyz;
    if occurrence != 0u {
        let chain = chains[occurrence - 1u];
        if u32(inst.world_pos_pad.w) != chain.destination_space || inst.color.a == 0.0 {
            var hidden: VertexOut;
            hidden.clip_pos = vec4<f32>(0.0, 0.0, -1.0, 1.0);
            return hidden;
        }
        center = (chain.content_to_drawn * vec4<f32>(center, 1.0)).xyz;
    }
    return billboard_vertex(vertex_index, inst, center, occurrence);
}

fn billboard_vertex(vertex_index: u32, inst: BillboardInstance, world_pos: vec3<f32>, occurrence: u32) -> VertexOut {
    let quad_positions = array<vec2<f32>, 6>(
        vec2<f32>(-0.5, -0.5), vec2<f32>(0.5, -0.5), vec2<f32>(-0.5, 0.5),
        vec2<f32>(-0.5, 0.5), vec2<f32>(0.5, -0.5), vec2<f32>(0.5, 0.5));
    let quad_uvs = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0));
    let quad_position = quad_positions[vertex_index];
    let quad_uv = quad_uvs[vertex_index];
    // Fit the source image inside the requested bounds without stretching.
    let dimensions = vec2<f32>(textureDimensions(sprite_tex));
    let scale = dimensions * min(abs(inst.scale_flags.x) / dimensions.x,
                                 abs(inst.scale_flags.y) / dimensions.y);
    let screen_scale = inst.scale_flags.z > 0.5;

    // Build camera-facing (billboard) basis vectors
    let cam_pos = cameras[0].position_near.xyz;
    // Parallel to the image plane, including camera roll. Facing the eye
    // instead distorts off-axis icons and degenerates when looking down/up.
    let view = cameras[0].view;
    let right = vec3<f32>(view[0].x, view[1].x, view[2].x);
    let up = vec3<f32>(view[0].y, view[1].y, view[2].y);

    // Offset in world space using the quad's local position
    var offset = right * quad_position.x * scale.x
               + up    * quad_position.y * scale.y;

    // Optional: constant screen-space scaling.
    // We must scale by VIEW-AXIS depth (dot with forward), NOT Euclidean distance.
    // Perspective division uses view-depth, so: world_size = scale * view_depth
    // → projected NDC size = scale * view_depth / view_depth = scale (constant).
    // Using Euclidean distance instead causes off-axis billboards to appear larger.
    // TODO: Can we use a branchless multiplication here somehow?
    if screen_scale {
        let view_depth = max(dot(cameras[0].forward_far.xyz, world_pos - cam_pos), 0.001);
        offset        *= view_depth;
    }

    let final_pos = world_pos + offset;

    var out: VertexOut;
    out.clip_pos = cameras[0].view_proj * vec4<f32>(final_pos, 1.0); // view_proj at offset 128
    // An instance at the eye cannot be projected: collapse it outside
    // the clip volume.
    let rel = cam_pos - world_pos;
    if !(dot(rel, rel) > 1e-8) ||
        (occurrence == 0u && !helio_camera_sees_space(cameras[0], u32(inst.world_pos_pad.w))) {
        out.clip_pos = vec4<f32>(0.0, 0.0, -1.0, 1.0);
    }
    out.uv       = quad_uv;
    out.color    = inst.color;
    out.drawn_position = final_pos;
    out.occurrence = occurrence;
    return out;
}

// ── Fragment shader ─────────────────────────────────────────────────────────

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return billboard_color(in, textureSample(sprite_tex, sprite_sampler, in.uv));
}

@fragment
fn fs_portal(in: VertexOut) -> @location(0) vec4<f32> {
    let uv_dx = dpdx(in.uv);
    let uv_dy = dpdy(in.uv);
    // The overlay and mask can have different extents when render scaling is
    // enabled. Project to the mask's normalized coordinates.
    let clip = cameras[0].view_proj * vec4<f32>(in.drawn_position, 1.0);
    let uv = clip.xy / clip.w * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    let extent = vec2<i32>(textureDimensions(aperture_mask));
    let pixel = clamp(vec2<i32>(uv * vec2<f32>(extent)), vec2<i32>(0), extent - vec2<i32>(1));
    let aperture = textureLoad(aperture_mask, pixel, 0).rg;
    let mask = aperture.r;
    if in.occurrence == 0u {
        // Reversed Z: smaller depths are behind the aperture.
        if mask != 0u && in.clip_pos.z <= bitcast<f32>(aperture.g) { discard; }
    } else {
        let chain = chains[in.occurrence - 1u];
        let p = transpose(portal_chain_rotation(chain)) *
            (in.drawn_position - chain.content_to_drawn[3].xyz);
        if !portal_outer_shows(chain, stages[chain.first_stage], p, mask) { discard; }
        for (var i = 1u; i < chain.stage_count; i += 1u) {
            if !portal_stage_shows(stages[chain.first_stage + i], p) { discard; }
        }
        for (var i = 0u; i < chain.hole_count; i += 1u) {
            if portal_hole_covers(stages[chain.first_hole + i], p) { discard; }
        }
    }
    return billboard_color(in, textureSampleGrad(sprite_tex, sprite_sampler, in.uv, uv_dx, uv_dy));
}

fn billboard_color(in: VertexOut, tex_color: vec4<f32>) -> vec4<f32> {
    // Tint the sprite by the per-instance color; use texture alpha for transparency
    let rgb   = tex_color.rgb * in.color.rgb;
    let alpha = tex_color.a   * in.color.a;
    if alpha < 0.01 { discard; }
    return vec4<f32>(rgb, alpha);
}
