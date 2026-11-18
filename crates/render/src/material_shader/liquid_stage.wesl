// Stage packing keeps centroid UVs separate and transports flat float bits as integers.
@if(!WEBGPU_COMPATIBILITY)
alias StageOutput = VertexOutput;
@if(!WEBGPU_COMPATIBILITY)
fn pack_stage(input: VertexOutput) -> StageOutput { return input; }
@if(!WEBGPU_COMPATIBILITY)
fn unpack_stage(input: StageOutput) -> VertexOutput { return input; }

@if(WEBGPU_COMPATIBILITY && !ENHANCED)
struct StageOutput {
    @builtin(position) @invariant clip_position: vec4<f32>,
    @location(0) smooth_0: vec4<f32>,
    @location(1) smooth_1: vec4<f32>,
    @location(2) smooth_2: vec4<f32>,
    @location(3) smooth_3: vec4<f32>,
    @location(4) @interpolate(flat, either) flat_0: vec4<u32>,
    @location(5) @interpolate(flat, either) flat_1: vec4<u32>,
}
@if(WEBGPU_COMPATIBILITY && !ENHANCED)
fn pack_stage(input: VertexOutput) -> StageOutput {
    var out: StageOutput;
    out.clip_position = input.clip_position;
    out.smooth_0 = vec4(input.uv.x, input.uv.y, input.water_tint.x, input.water_tint.y);
    out.smooth_1 = vec4(input.water_tint.z, input.water_tint.w, input.lighting.x, input.lighting.y);
    out.smooth_2 = vec4(input.lighting.z, input.world_position.x, input.world_position.y, input.world_position.z);
    out.smooth_3 = vec4(input.native_light_levels.x, input.native_light_levels.y, 0.0, 0.0);
    out.flat_0 = vec4(input.current_texture, input.next_texture, bitcast<u32>(input.frame_blend), input.depth_write_route);
    out.flat_1 = vec4(bitcast<u32>(input.native_face_shade), input.two_sided, 0u, 0u);
    return out;
}
@if(WEBGPU_COMPATIBILITY && !ENHANCED)
fn unpack_stage(input: StageOutput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = input.clip_position;
    out.uv = vec2(input.smooth_0.x, input.smooth_0.y);
    out.current_texture = input.flat_0.x;
    out.next_texture = input.flat_0.y;
    out.frame_blend = bitcast<f32>(input.flat_0.z);
    out.water_tint = vec4(input.smooth_0.z, input.smooth_0.w, input.smooth_1.x, input.smooth_1.y);
    out.lighting = vec3(input.smooth_1.z, input.smooth_1.w, input.smooth_2.x);
    out.depth_write_route = input.flat_0.w;
    out.world_position = vec3(input.smooth_2.y, input.smooth_2.z, input.smooth_2.w);
    out.native_light_levels = vec2(input.smooth_3.x, input.smooth_3.y);
    out.native_face_shade = bitcast<f32>(input.flat_1.x);
    out.two_sided = input.flat_1.y;
    return out;
}

@if(WEBGPU_COMPATIBILITY && ENHANCED)
struct StageOutput {
    @builtin(position) @invariant clip_position: vec4<f32>,
    @location(0) smooth_0: vec4<f32>,
    @location(1) smooth_1: vec4<f32>,
    @location(2) smooth_2: vec4<f32>,
    @location(3) smooth_3: vec4<f32>,
    @location(4) smooth_4: vec4<f32>,
    @location(5) @interpolate(flat, either) flat_0: vec4<u32>,
    @location(6) @interpolate(flat, either) flat_1: vec4<u32>,
}
@if(WEBGPU_COMPATIBILITY && ENHANCED)
fn pack_stage(input: VertexOutput) -> StageOutput {
    var out: StageOutput;
    out.clip_position = input.clip_position;
    out.smooth_0 = vec4(input.uv.x, input.uv.y, input.water_tint.x, input.water_tint.y);
    out.smooth_1 = vec4(input.water_tint.z, input.water_tint.w, input.lighting.x, input.lighting.y);
    out.smooth_2 = vec4(input.lighting.z, input.sky_light, input.ambient_occlusion, input.world_position.x);
    out.smooth_3 = vec4(input.world_position.y, input.world_position.z, input.native_light_levels.x, input.native_light_levels.y);
    out.smooth_4 = vec4(input.normal.x, input.normal.y, input.normal.z, 0.0);
    out.flat_0 = vec4(input.current_texture, input.next_texture, bitcast<u32>(input.frame_blend), input.depth_write_route);
    out.flat_1 = vec4(bitcast<u32>(input.native_face_shade), input.two_sided, input.surface_class, 0u);
    return out;
}
@if(WEBGPU_COMPATIBILITY && ENHANCED)
fn unpack_stage(input: StageOutput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = input.clip_position;
    out.uv = vec2(input.smooth_0.x, input.smooth_0.y);
    out.current_texture = input.flat_0.x;
    out.next_texture = input.flat_0.y;
    out.frame_blend = bitcast<f32>(input.flat_0.z);
    out.water_tint = vec4(input.smooth_0.z, input.smooth_0.w, input.smooth_1.x, input.smooth_1.y);
    out.lighting = vec3(input.smooth_1.z, input.smooth_1.w, input.smooth_2.x);
    out.sky_light = input.smooth_2.y;
    out.ambient_occlusion = input.smooth_2.z;
    out.depth_write_route = input.flat_0.w;
    out.world_position = vec3(input.smooth_2.w, input.smooth_3.x, input.smooth_3.y);
    out.native_light_levels = vec2(input.smooth_3.z, input.smooth_3.w);
    out.native_face_shade = bitcast<f32>(input.flat_1.x);
    out.two_sided = input.flat_1.y;
    out.normal = vec3(input.smooth_4.x, input.smooth_4.y, input.smooth_4.z);
    out.surface_class = input.flat_1.z;
    return out;
}
