// Blinn-Phong shader

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
    @location(12) phong_param: vec4<f32>,
    @location(13) model_scale: vec3<f32>,
    @location(14) model_rot: vec4<f32>,
    @location(15) model_pos: vec3<f32>,
}

// Implementation

struct VertexOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) frag_pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) phong_param: vec4<f32>,
}

@vertex fn vs_main(in: VertexIn) -> VertexOut {
    let pos = apply_all(in.pos, in.model_scale, in.model_rot, in.model_pos);

    var out: VertexOut;
    out.pos = uni.view_m[#VIEW_INDEX_VAL#] * vec4(pos, 1);
    out.frag_pos = pos;
    out.normal = apply_rot(normalize(apply_scale(in.normal, 1 / in.model_scale)), in.model_rot);
    out.color = in.color;
    out.phong_param = in.phong_param;

    return out;
}

@fragment fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let phong_param = in.phong_param;
    let ambient = phong_param[0];
    let diffuse = phong_param[1];
    let specular = phong_param[2];
    let shininess = phong_param[3];
    let frag_pos = in.frag_pos;
    let normal = normalize(in.normal);

    let ambient_factor = ambient;

    let light_dir = normalize(uni.light_pos - frag_pos);
    let diffuse_factor = diffuse * max(dot(light_dir, normal), 0);

    let cam_dir = normalize(uni.cam_pos - frag_pos);
    let halfway_dir = normalize(light_dir + cam_dir);
    let specular_factor = select(0, specular * pow(max(dot(halfway_dir, normal), 0), shininess), shininess > 0); // TODO: Why saber-ray is rendered wrong if shininess = 0?

    let color = (ambient_factor + diffuse_factor + specular_factor) * in.color;

    return vec4(color, 1);
}
