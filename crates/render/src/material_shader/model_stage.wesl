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
    @location(0) @interpolate(perspective, centroid) centroid_0: vec4<f32>,
    @location(1) smooth_0: vec4<f32>,
    @location(2) smooth_1: vec4<f32>,
    @location(3) smooth_2: vec4<f32>,
    @location(4) @interpolate(flat, either) flat_0: vec4<u32>,
    @location(5) @interpolate(flat, either) flat_1: vec4<u32>,
    @location(6) @interpolate(flat, either) flat_2: vec4<u32>,
    @location(7) @interpolate(flat, either) flat_3: vec4<u32>,
}
@if(WEBGPU_COMPATIBILITY && !ENHANCED)
fn pack_stage(input: VertexOutput) -> StageOutput {
    var out: StageOutput;
    out.clip_position = input.clip_position;
    out.centroid_0 = vec4(input.uv.x, input.uv.y, 0.0, 0.0);
    out.smooth_0 = vec4(input.normal.x, input.normal.y, input.normal.z, input.lighting.x);
    out.smooth_1 = vec4(input.lighting.y, input.lighting.z, input.native_light_levels.x, input.native_light_levels.y);
    out.smooth_2 = vec4(input.native_ao_face, input.world_position.x, input.world_position.y, input.world_position.z);
    out.flat_0 = vec4(input.current_texture, input.material_flags, bitcast<u32>(input.local_position.x), bitcast<u32>(input.local_position.y));
    out.flat_1 = vec4(bitcast<u32>(input.local_position.z), input.biome_record, input.next_texture, bitcast<u32>(input.frame_blend));
    out.flat_2 = vec4(input.visibility.x, input.visibility.y, bitcast<u32>(input.world_origin.x), bitcast<u32>(input.world_origin.y));
    out.flat_3 = vec4(bitcast<u32>(input.world_origin.z), bitcast<u32>(input.tint_gamma.x), bitcast<u32>(input.tint_gamma.y), bitcast<u32>(input.tint_gamma.z));
    return out;
}
@if(WEBGPU_COMPATIBILITY && !ENHANCED)
fn unpack_stage(input: StageOutput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = input.clip_position;
    out.uv = vec2(input.centroid_0.x, input.centroid_0.y);
    out.current_texture = input.flat_0.x;
    out.normal = vec3(input.smooth_0.x, input.smooth_0.y, input.smooth_0.z);
    out.material_flags = input.flat_0.y;
    out.local_position = vec3(bitcast<f32>(input.flat_0.z), bitcast<f32>(input.flat_0.w), bitcast<f32>(input.flat_1.x));
    out.biome_record = input.flat_1.y;
    out.next_texture = input.flat_1.z;
    out.frame_blend = bitcast<f32>(input.flat_1.w);
    out.visibility = vec2(input.flat_2.x, input.flat_2.y);
    out.lighting = vec3(input.smooth_0.w, input.smooth_1.x, input.smooth_1.y);
    out.native_light_levels = vec2(input.smooth_1.z, input.smooth_1.w);
    out.native_ao_face = input.smooth_2.x;
    out.world_origin = vec3(bitcast<f32>(input.flat_2.z), bitcast<f32>(input.flat_2.w), bitcast<f32>(input.flat_3.x));
    out.world_position = vec3(input.smooth_2.y, input.smooth_2.z, input.smooth_2.w);
    out.tint_gamma = vec3(bitcast<f32>(input.flat_3.y), bitcast<f32>(input.flat_3.z), bitcast<f32>(input.flat_3.w));
    return out;
}

@if(WEBGPU_COMPATIBILITY && ENHANCED)
struct StageOutput {
    @builtin(position) @invariant clip_position: vec4<f32>,
    @location(0) @interpolate(perspective, centroid) centroid_0: vec4<f32>,
    @location(1) smooth_0: vec4<f32>,
    @location(2) smooth_1: vec4<f32>,
    @location(3) smooth_2: vec4<f32>,
    @location(4) @interpolate(flat, either) flat_0: vec4<u32>,
    @location(5) @interpolate(flat, either) flat_1: vec4<u32>,
    @location(6) @interpolate(flat, either) flat_2: vec4<u32>,
    @location(7) @interpolate(flat, either) flat_3: vec4<u32>,
}
@if(WEBGPU_COMPATIBILITY && ENHANCED)
fn pack_stage(input: VertexOutput) -> StageOutput {
    var out: StageOutput;
    out.clip_position = input.clip_position;
    out.centroid_0 = vec4(input.uv.x, input.uv.y, 0.0, 0.0);
    out.smooth_0 = vec4(input.normal.x, input.normal.y, input.normal.z, input.lighting.x);
    out.smooth_1 = vec4(input.lighting.y, input.lighting.z, input.sky_light, input.ambient_occlusion);
    out.smooth_2 = vec4(input.world_position.x, input.world_position.y, input.world_position.z, 0.0);
    out.flat_0 = vec4(input.current_texture, input.material_flags, bitcast<u32>(input.local_position.x), bitcast<u32>(input.local_position.y));
    out.flat_1 = vec4(bitcast<u32>(input.local_position.z), input.biome_record, input.next_texture, bitcast<u32>(input.frame_blend));
    out.flat_2 = vec4(input.visibility.x, input.visibility.y, bitcast<u32>(input.world_origin.x), bitcast<u32>(input.world_origin.y));
    out.flat_3 = vec4(bitcast<u32>(input.world_origin.z), input.surface_class, 0u, 0u);
    return out;
}
@if(WEBGPU_COMPATIBILITY && ENHANCED)
fn unpack_stage(input: StageOutput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = input.clip_position;
    out.uv = vec2(input.centroid_0.x, input.centroid_0.y);
    out.current_texture = input.flat_0.x;
    out.normal = vec3(input.smooth_0.x, input.smooth_0.y, input.smooth_0.z);
    out.material_flags = input.flat_0.y;
    out.local_position = vec3(bitcast<f32>(input.flat_0.z), bitcast<f32>(input.flat_0.w), bitcast<f32>(input.flat_1.x));
    out.biome_record = input.flat_1.y;
    out.next_texture = input.flat_1.z;
    out.frame_blend = bitcast<f32>(input.flat_1.w);
    out.visibility = vec2(input.flat_2.x, input.flat_2.y);
    out.lighting = vec3(input.smooth_0.w, input.smooth_1.x, input.smooth_1.y);
    out.sky_light = input.smooth_1.z;
    out.ambient_occlusion = input.smooth_1.w;
    out.world_origin = vec3(bitcast<f32>(input.flat_2.z), bitcast<f32>(input.flat_2.w), bitcast<f32>(input.flat_3.x));
    out.world_position = vec3(input.smooth_2.x, input.smooth_2.y, input.smooth_2.z);
    out.surface_class = input.flat_3.y;
    return out;
}
