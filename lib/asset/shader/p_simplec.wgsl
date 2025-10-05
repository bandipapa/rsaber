// Simple color shader

// Input

#UNI#

struct VertexIn {
    #VIEW_INDEX_DEF#
    // Per-vertex
    @location(0) pos: vec3<f32>,
    // Per-instance
    @location(11) color: vec3<f32>,
    @location(12) model_m0: vec4<f32>,
    @location(13) model_m1: vec4<f32>,
    @location(14) model_m2: vec4<f32>,
    @location(15) model_m3: vec4<f32>,
}

// Implementation

struct VertexOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec3<f32>,
}

@vertex fn vs_main(in: VertexIn) -> VertexOut {
    let model_m = mat4x4(in.model_m0, in.model_m1, in.model_m2, in.model_m3);

    var out: VertexOut;
    out.pos = uni.view_m[#VIEW_INDEX_VAL#] * model_m * vec4(in.pos, 1);
    out.color = in.color;

    return out;
}

@fragment fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let color = in.color;

    return vec4(color, 1);
}
