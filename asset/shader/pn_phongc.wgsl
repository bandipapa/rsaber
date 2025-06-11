// Blinn-Phong shader

// Input

#UNI#

struct VertexIn {
    #VIEW_INDEX_DEF#
    // Per-vertex
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // Per-instance
    @location(10) color: vec3<f32>,
    @location(11) phong_param: vec4<f32>,
    @location(12) model_m0: vec4<f32>,
    @location(13) model_m1: vec4<f32>,
    @location(14) model_m2: vec4<f32>,
    @location(15) model_m3: vec4<f32>,
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
    let model_m = mat4x4<f32>(in.model_m0, in.model_m1, in.model_m2, in.model_m3);
    let normal_m = mat3x3<f32>(normalize(model_m[0].xyz), normalize(model_m[1].xyz), normalize(model_m[2].xyz)); // TODO: Or do inverse+transpose?
    let pos = model_m * vec4<f32>(in.pos, 1);

    var out: VertexOut;
    out.pos = uni.view_m[#VIEW_INDEX_VAL#] * pos;
    out.frag_pos = pos.xyz;
    out.normal = normal_m * in.normal;
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

    return vec4<f32>(color, 1);
}
