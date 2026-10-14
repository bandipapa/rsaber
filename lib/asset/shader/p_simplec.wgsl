// Simple color shader

#COMMON#

// Input

#UNI#

struct VertexIn {
    #VIEW_INDEX_DEF#
    // Per-vertex
    @location(0) pos: vec3<f32>,
    // Per-instance
    @location(12) color: vec3<f32>,
    @location(13) model_scale: vec3<f32>,
    @location(14) model_rot: vec4<f32>,
    @location(15) model_pos: vec3<f32>,
}

// Implementation

struct VertexOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec3<f32>,
}

@vertex fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.pos = uni.view_m[#VIEW_INDEX_VAL#] * vec4(apply_all(in.pos, in.model_scale, in.model_rot, in.model_pos), 1);
    out.color = in.color;

    return out;
}

@fragment fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return vec4(in.color, 1);
}
