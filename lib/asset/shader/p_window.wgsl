// Window shader

enable wgpu_binding_array;

#COMMON#

// Input

#UNI#

@group(1) @binding(0) var bind_samplers: binding_array<sampler>;
@group(1) @binding(1) var bind_textures: binding_array<texture_2d<f32>>;

struct VertexIn {
    #VIEW_INDEX_DEF#
    // Per-vertex
    @location(0) pos: vec3<f32>,
    // Per-instance
    @location(12) bind_id: vec2<u32>,
    @location(13) model_scale: vec3<f32>,
    @location(14) model_rot: vec4<f32>,
    @location(15) model_pos: vec3<f32>,
}

// Implementation

struct VertexOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) @interpolate(flat) sampler_id: u32,
    @location(1) @interpolate(flat) texture_id: u32,
    @location(2) orig_pos: vec2<f32>,
    @location(3) tex_coord: vec2<f32>,
}

@vertex fn vs_main(in: VertexIn) -> VertexOut {
    let pos = in.pos;

    var out: VertexOut;
    out.pos = uni.view_m[#VIEW_INDEX_VAL#] * vec4(apply_all(pos, in.model_scale, in.model_rot, in.model_pos), 1);
    out.sampler_id = in.bind_id[0];
    out.texture_id = in.bind_id[1];
    out.orig_pos = pos.xz;
    out.tex_coord = vec2(pos.x + 0.5, 1 - (pos.z + 0.5));

    return out;
}

@fragment fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let sampl = bind_samplers[in.sampler_id];
    let texture = bind_textures[in.texture_id];
    var color = textureSample(texture, sampl, in.tex_coord).rgb;

    // Do antialias at the edges.

    let orig_pos_abs = abs(in.orig_pos);
    let fw = fwidth(orig_pos_abs);
    let start = 0.5 - fw;
    let diff = orig_pos_abs - start;
    
    if (diff.x >= 0) {
        color *= 1.0 - diff.x / fw.x;
    }
    
    if (diff.y >= 0) {
        color *= 1.0 - diff.y / fw.y;
    }

    return vec4(color, 1);
}
