// Grid shader

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
    @location(0) orig_pos: vec2<f32>,
    @location(1) color: vec3<f32>,
}

@vertex fn vs_main(in: VertexIn) -> VertexOut {
    let pos = in.pos;

    var out: VertexOut;
    out.pos = uni.view_m[#VIEW_INDEX_VAL#] * vec4(apply_all(pos, in.model_scale, in.model_rot, in.model_pos), 1);
    out.orig_pos = pos.xy;
    out.color = in.color;

    return out;
}

@fragment fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    // Compute anti-aliased grid lines. This excellent shader is
    // taken from https://madebyevan.com/shaders/grid/ .

    let orig_pos = in.orig_pos;

    let grid = abs(fract(orig_pos - 0.5) - 0.5) / fwidth(orig_pos);
    let line = min(grid.x, grid.y);
    let factor = 1 - min(line, 1);
    let color = factor * in.color;

    return vec4(color, 1);
}
