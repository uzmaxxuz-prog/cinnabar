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
    out.centroid_0 = vec4(input.model_uv.x, input.model_uv.y, 0.0, 0.0);
    out.smooth_0 = vec4(input.world_position.x, input.world_position.y, input.world_position.z, input.native_light_ao.x);
    out.smooth_1 = vec4(input.native_light_ao.y, input.native_light_ao.z, input.liquid_uv.x, input.liquid_uv.y);
    out.smooth_2 = vec4(input.water_tint.x, input.water_tint.y, input.water_tint.z, input.water_tint.w);
    out.flat_0 = vec4(input.routing.x, input.routing.y, input.routing.z, input.routing.w);
    out.flat_1 = vec4(input.textures.x, input.textures.y, input.material_biome.x, input.material_biome.y);
    out.flat_2 = vec4(bitcast<u32>(input.frame_face.x), bitcast<u32>(input.frame_face.y), bitcast<u32>(input.tint_gamma.x), bitcast<u32>(input.tint_gamma.y));
    out.flat_3 = vec4(bitcast<u32>(input.tint_gamma.z), 0u, 0u, 0u);
    return out;
}
@if(WEBGPU_COMPATIBILITY && !ENHANCED)
fn unpack_stage(input: StageOutput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = input.clip_position;
    out.routing = vec4(input.flat_0.x, input.flat_0.y, input.flat_0.z, input.flat_0.w);
    out.textures = vec2(input.flat_1.x, input.flat_1.y);
    out.material_biome = vec2(input.flat_1.z, input.flat_1.w);
    out.frame_face = vec2(bitcast<f32>(input.flat_2.x), bitcast<f32>(input.flat_2.y));
    out.world_position = vec3(input.smooth_0.x, input.smooth_0.y, input.smooth_0.z);
    out.native_light_ao = vec3(input.smooth_0.w, input.smooth_1.x, input.smooth_1.y);
    out.tint_gamma = vec3(bitcast<f32>(input.flat_2.z), bitcast<f32>(input.flat_2.w), bitcast<f32>(input.flat_3.x));
    out.liquid_uv = vec2(input.smooth_1.z, input.smooth_1.w);
    out.water_tint = vec4(input.smooth_2.x, input.smooth_2.y, input.smooth_2.z, input.smooth_2.w);
    out.model_uv = vec2(input.centroid_0.x, input.centroid_0.y);
    return out;
}

@if(WEBGPU_COMPATIBILITY && ENHANCED)
struct StageOutput {
    @builtin(position) @invariant clip_position: vec4<f32>,
    @location(0) @interpolate(perspective, centroid) centroid_0: vec4<f32>,
    @location(1) smooth_0: vec4<f32>,
    @location(2) smooth_1: vec4<f32>,
    @location(3) smooth_2: vec4<f32>,
    @location(4) smooth_3: vec4<f32>,
    @location(5) smooth_4: vec4<f32>,
    @location(6) @interpolate(flat, either) flat_0: vec4<u32>,
    @location(7) @interpolate(flat, either) flat_1: vec4<u32>,
    @location(8) @interpolate(flat, either) flat_2: vec4<u32>,
    @location(9) @interpolate(flat, either) flat_3: vec4<u32>,
    @location(10) @interpolate(flat, either) flat_4: vec4<u32>,
}
@if(WEBGPU_COMPATIBILITY && ENHANCED)
fn pack_stage(input: VertexOutput) -> StageOutput {
    var out: StageOutput;
    out.clip_position = input.clip_position;
    out.centroid_0 = vec4(input.model_uv.x, input.model_uv.y, 0.0, 0.0);
    out.smooth_0 = vec4(input.world_position.x, input.world_position.y, input.world_position.z, input.lighting.x);
    out.smooth_1 = vec4(input.lighting.y, input.lighting.z, input.normal.x, input.normal.y);
    out.smooth_2 = vec4(input.normal.z, input.sky_ao.x, input.sky_ao.y, input.liquid_uv.x);
    out.smooth_3 = vec4(input.liquid_uv.y, input.water_tint.x, input.water_tint.y, input.water_tint.z);
    out.smooth_4 = vec4(input.water_tint.w, 0.0, 0.0, 0.0);
    out.flat_0 = vec4(input.routing.x, input.routing.y, input.routing.z, input.routing.w);
    out.flat_1 = vec4(input.textures.x, input.textures.y, input.material_biome.x, input.material_biome.y);
    out.flat_2 = vec4(bitcast<u32>(input.frame_face.x), bitcast<u32>(input.frame_face.y), bitcast<u32>(input.local_position.x), bitcast<u32>(input.local_position.y));
    out.flat_3 = vec4(bitcast<u32>(input.local_position.z), bitcast<u32>(input.world_origin.x), bitcast<u32>(input.world_origin.y), bitcast<u32>(input.world_origin.z));
    out.flat_4 = vec4(input.surface_class, 0u, 0u, 0u);
    return out;
}
@if(WEBGPU_COMPATIBILITY && ENHANCED)
fn unpack_stage(input: StageOutput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = input.clip_position;
    out.routing = vec4(input.flat_0.x, input.flat_0.y, input.flat_0.z, input.flat_0.w);
    out.textures = vec2(input.flat_1.x, input.flat_1.y);
    out.material_biome = vec2(input.flat_1.z, input.flat_1.w);
    out.frame_face = vec2(bitcast<f32>(input.flat_2.x), bitcast<f32>(input.flat_2.y));
    out.local_position = vec3(bitcast<f32>(input.flat_2.z), bitcast<f32>(input.flat_2.w), bitcast<f32>(input.flat_3.x));
    out.world_origin = vec3(bitcast<f32>(input.flat_3.y), bitcast<f32>(input.flat_3.z), bitcast<f32>(input.flat_3.w));
    out.world_position = vec3(input.smooth_0.x, input.smooth_0.y, input.smooth_0.z);
    out.lighting = vec3(input.smooth_0.w, input.smooth_1.x, input.smooth_1.y);
    out.normal = vec3(input.smooth_1.z, input.smooth_1.w, input.smooth_2.x);
    out.sky_ao = vec2(input.smooth_2.y, input.smooth_2.z);
    out.surface_class = input.flat_4.x;
    out.liquid_uv = vec2(input.smooth_2.w, input.smooth_3.x);
    out.water_tint = vec4(input.smooth_3.y, input.smooth_3.z, input.smooth_3.w, input.smooth_4.x);
    out.model_uv = vec2(input.centroid_0.x, input.centroid_0.y);
    return out;
}
