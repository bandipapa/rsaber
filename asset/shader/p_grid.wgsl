// Grid shader

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
    @location(0) orig_pos: vec3<f32>,
    @location(1) color: vec3<f32>,
}

@vertex fn vs_main(in: VertexIn) -> VertexOut {
    let pos = in.pos;
    let model_m = mat4x4<f32>(in.model_m0, in.model_m1, in.model_m2, in.model_m3);

    var out: VertexOut;
    out.pos = uni.view_m[#VIEW_INDEX_VAL#] * model_m * vec4<f32>(pos, 1);
    out.orig_pos = pos;
    out.color = in.color;

    return out;
}

@fragment fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    // Compute anti-aliased grid lines. This excellent shader is
    // taken from https://madebyevan.com/shaders/grid/ .

    let orig_pos = in.orig_pos.xy;

    let grid = abs(fract(orig_pos - 0.5) - 0.5) / fwidth(orig_pos);
    let line = min(grid.x, grid.y);
    let factor = 1 - min(line, 1);
    let color = factor * in.color;

    return vec4<f32>(color, 1);
}
