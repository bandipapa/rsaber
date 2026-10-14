// Obstacle shader

#COMMON#

// Input

#UNI#

struct VertexIn {
    #VIEW_INDEX_DEF#
    // Per-vertex
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // Per-instance
    @location(11) color: vec3<f32>,
    @location(12) outline_width: f32,
    @location(13) model_scale: vec3<f32>,
    @location(14) model_rot: vec4<f32>,
    @location(15) model_pos: vec3<f32>,
}

// Implementation

struct VertexOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) frag_pos: vec2<f32>,
    @location(1) scale: vec2<f32>,
    @location(2) color: vec3<f32>,
    @location(3) outline_width: f32,
}

@vertex fn vs_main(in: VertexIn) -> VertexOut {
    let pos_scale = apply_scale(in.pos, in.model_scale);

    var out: VertexOut;
    out.pos = uni.view_m[#VIEW_INDEX_VAL#] * vec4(apply_pos(apply_rot(pos_scale, in.model_rot), in.model_pos), 1);

    let normal = abs(in.normal);
    var frag_pos: vec2<f32>;
    var scale: vec2<f32>;

    if normal.x > 0 {
        frag_pos = pos_scale.yz;
        scale = in.model_scale.yz;
    } else if normal.y > 0 {
        frag_pos = pos_scale.xz;
        scale = in.model_scale.xz;
    } else { // normal.z > 0
        frag_pos = pos_scale.xy;
        scale = in.model_scale.xy;
    }

    out.frag_pos = frag_pos;
    out.scale = scale / 2;
    out.color = in.color;
    out.outline_width = in.outline_width;
    
    return out;
}

@fragment fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let d = in.scale - abs(in.frag_pos); // Distance to edge.
    let m = min(d.x, d.y); // Distance to closest edge.
    let alpha = max(1.0 - smoothstep(0, in.outline_width, m), 0.01);

    return vec4(in.color, alpha);
}
