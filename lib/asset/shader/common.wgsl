fn apply_scale(v: vec3<f32>, scale: vec3<f32>) -> vec3<f32> {
    return v * scale;
}

fn apply_rot(v: vec3<f32>, rot: vec4<f32>) -> vec3<f32> {
    // Optimized quaternion-vector rotation.

    let rot_v = rot.xyz;
    let t = 2 * cross(rot_v, v);
    return v + rot.w * t + cross(rot_v, t);
}

fn apply_pos(v: vec3<f32>, pos: vec3<f32>) -> vec3<f32> {
    return v + pos;
}

fn apply_all(v: vec3<f32>, scale: vec3<f32>, rot: vec4<f32>, pos: vec3<f32>) -> vec3<f32> {
    return apply_pos(apply_rot(apply_scale(v, scale), rot), pos);
}
