// Stage packing keeps centroid UVs separate and transports flat float bits as integers.
@if(!WEBGPU_COMPATIBILITY)
alias StageOutput = VertexOutput;
@if(!WEBGPU_COMPATIBILITY)
fn pack_stage(input: VertexOutput) -> StageOutput { return input; }
@if(!WEBGPU_COMPATIBILITY)
fn unpack_stage(input: StageOutput) -> VertexOutput { return input; }

@if(WEBGPU_COMPATIBILITY && !ACTOR_MOTION && !ALPHA_TO_COVERAGE)
struct StageOutput {
    @builtin(position) @invariant position: vec4<f32>,
    @location(0) @interpolate(perspective, centroid) centroid_0: vec4<f32>,
    @location(1) @interpolate(perspective, centroid) centroid_1: vec4<f32>,
    @location(2) smooth_0: vec4<f32>,
    @location(3) smooth_1: vec4<f32>,
    @location(4) smooth_2: vec4<f32>,
    @location(5) @interpolate(flat, either) flat_0: vec4<u32>,
    @location(6) @interpolate(flat, either) flat_1: vec4<u32>,
    @location(7) @interpolate(flat, either) flat_2: vec4<u32>,
}
@if(WEBGPU_COMPATIBILITY && !ACTOR_MOTION && !ALPHA_TO_COVERAGE)
fn pack_stage(input: VertexOutput) -> StageOutput {
    var out: StageOutput;
    out.position = input.position;
    out.centroid_0 = vec4(input.uv.x, input.uv.y, input.uv.z, input.uv.w);
    out.centroid_1 = vec4(input.back_uv.x, input.back_uv.y, 0.0, 0.0);
    out.smooth_0 = vec4(input.world_normal.x, input.world_normal.y, input.world_normal.z, input.world_position.x);
    out.smooth_1 = vec4(input.world_position.y, input.world_position.z, input.native_lighting.x, input.native_lighting.y);
    out.smooth_2 = vec4(input.native_lighting.z, input.back_native_lighting.x, input.back_native_lighting.y, input.back_native_lighting.z);
    out.flat_0 = vec4(input.skin_info.x, input.skin_info.y, input.skin_info.z, input.skin_info.w);
    out.flat_1 = vec4(input.tint, input.overlay_bits, input.light, input.multitexture_layers.x);
    out.flat_2 = vec4(input.multitexture_layers.y, input.material, input.material_parameters.x, input.material_parameters.y);
    return out;
}
@if(WEBGPU_COMPATIBILITY && !ACTOR_MOTION && !ALPHA_TO_COVERAGE)
fn unpack_stage(input: StageOutput) -> VertexOutput {
    var out: VertexOutput;
    out.position = input.position;
    out.uv = vec4(input.centroid_0.x, input.centroid_0.y, input.centroid_0.z, input.centroid_0.w);
    out.skin_info = vec4(input.flat_0.x, input.flat_0.y, input.flat_0.z, input.flat_0.w);
    out.world_normal = vec3(input.smooth_0.x, input.smooth_0.y, input.smooth_0.z);
    out.back_uv = vec2(input.centroid_1.x, input.centroid_1.y);
    out.tint = input.flat_1.x;
    out.overlay = unpack4x8unorm(input.flat_1.y);
    out.overlay_bits = input.flat_1.y;
    out.light = input.flat_1.z;
    out.world_position = vec3(input.smooth_0.w, input.smooth_1.x, input.smooth_1.y);
    out.multitexture_layers = vec2(input.flat_1.w, input.flat_2.x);
    out.native_lighting = vec3(input.smooth_1.z, input.smooth_1.w, input.smooth_2.x);
    out.material = input.flat_2.y;
    out.material_parameters = vec2(input.flat_2.z, input.flat_2.w);
    out.back_native_lighting = vec3(input.smooth_2.y, input.smooth_2.z, input.smooth_2.w);
    return out;
}

@if(WEBGPU_COMPATIBILITY && ACTOR_MOTION && !ALPHA_TO_COVERAGE)
struct StageOutput {
    @builtin(position) @invariant position: vec4<f32>,
    @location(0) @interpolate(perspective, centroid) centroid_0: vec4<f32>,
    @location(1) @interpolate(perspective, centroid) centroid_1: vec4<f32>,
    @location(2) smooth_0: vec4<f32>,
    @location(3) smooth_1: vec4<f32>,
    @location(4) smooth_2: vec4<f32>,
    @location(5) smooth_3: vec4<f32>,
    @location(6) smooth_4: vec4<f32>,
    @location(7) @interpolate(flat, either) flat_0: vec4<u32>,
    @location(8) @interpolate(flat, either) flat_1: vec4<u32>,
    @location(9) @interpolate(flat, either) flat_2: vec4<u32>,
}
@if(WEBGPU_COMPATIBILITY && ACTOR_MOTION && !ALPHA_TO_COVERAGE)
fn pack_stage(input: VertexOutput) -> StageOutput {
    var out: StageOutput;
    out.position = input.position;
    out.centroid_0 = vec4(input.uv.x, input.uv.y, input.uv.z, input.uv.w);
    out.centroid_1 = vec4(input.back_uv.x, input.back_uv.y, 0.0, 0.0);
    out.smooth_0 = vec4(input.current_clip.x, input.current_clip.y, input.current_clip.z, input.current_clip.w);
    out.smooth_1 = vec4(input.world_position.x, input.world_position.y, input.world_position.z, input.native_lighting.x);
    out.smooth_2 = vec4(input.native_lighting.y, input.native_lighting.z, input.back_native_lighting.x, input.back_native_lighting.y);
    out.smooth_3 = vec4(input.back_native_lighting.z, input.previous_clip.x, input.previous_clip.y, input.previous_clip.z);
    out.smooth_4 = vec4(input.previous_clip.w, 0.0, 0.0, 0.0);
    out.flat_0 = vec4(input.skin_info.x, input.skin_info.y, input.skin_info.z, input.skin_info.w);
    out.flat_1 = vec4(input.tint, input.overlay_bits, input.light, input.multitexture_layers.x);
    out.flat_2 = vec4(input.multitexture_layers.y, input.material, input.material_parameters.x, input.material_parameters.y);
    return out;
}
@if(WEBGPU_COMPATIBILITY && ACTOR_MOTION && !ALPHA_TO_COVERAGE)
fn unpack_stage(input: StageOutput) -> VertexOutput {
    var out: VertexOutput;
    out.position = input.position;
    out.uv = vec4(input.centroid_0.x, input.centroid_0.y, input.centroid_0.z, input.centroid_0.w);
    out.skin_info = vec4(input.flat_0.x, input.flat_0.y, input.flat_0.z, input.flat_0.w);
    out.current_clip = vec4(input.smooth_0.x, input.smooth_0.y, input.smooth_0.z, input.smooth_0.w);
    out.back_uv = vec2(input.centroid_1.x, input.centroid_1.y);
    out.tint = input.flat_1.x;
    out.overlay = unpack4x8unorm(input.flat_1.y);
    out.overlay_bits = input.flat_1.y;
    out.light = input.flat_1.z;
    out.world_position = vec3(input.smooth_1.x, input.smooth_1.y, input.smooth_1.z);
    out.multitexture_layers = vec2(input.flat_1.w, input.flat_2.x);
    out.native_lighting = vec3(input.smooth_1.w, input.smooth_2.x, input.smooth_2.y);
    out.material = input.flat_2.y;
    out.material_parameters = vec2(input.flat_2.z, input.flat_2.w);
    out.back_native_lighting = vec3(input.smooth_2.z, input.smooth_2.w, input.smooth_3.x);
    out.previous_clip = vec4(input.smooth_3.y, input.smooth_3.z, input.smooth_3.w, input.smooth_4.x);
    return out;
}

@if(WEBGPU_COMPATIBILITY && !ACTOR_MOTION && ALPHA_TO_COVERAGE)
struct StageOutput {
    @builtin(position) @invariant position: vec4<f32>,
    @location(0) @interpolate(perspective, centroid) centroid_0: vec4<f32>,
    @location(1) @interpolate(perspective, centroid) centroid_1: vec4<f32>,
    @location(2) smooth_0: vec4<f32>,
    @location(3) smooth_1: vec4<f32>,
    @location(4) smooth_2: vec4<f32>,
    @location(5) @interpolate(flat, either) flat_0: vec4<u32>,
    @location(6) @interpolate(flat, either) flat_1: vec4<u32>,
    @location(7) @interpolate(flat, either) flat_2: vec4<u32>,
    @location(8) @interpolate(flat, either) flat_3: vec4<u32>,
}
@if(WEBGPU_COMPATIBILITY && !ACTOR_MOTION && ALPHA_TO_COVERAGE)
fn pack_stage(input: VertexOutput) -> StageOutput {
    var out: StageOutput;
    out.position = input.position;
    out.centroid_0 = vec4(input.uv.x, input.uv.y, input.uv.z, input.uv.w);
    out.centroid_1 = vec4(input.back_uv.x, input.back_uv.y, 0.0, 0.0);
    out.smooth_0 = vec4(input.world_normal.x, input.world_normal.y, input.world_normal.z, input.world_position.x);
    out.smooth_1 = vec4(input.world_position.y, input.world_position.z, input.native_lighting.x, input.native_lighting.y);
    out.smooth_2 = vec4(input.native_lighting.z, input.back_native_lighting.x, input.back_native_lighting.y, input.back_native_lighting.z);
    out.flat_0 = vec4(input.skin_info.x, input.skin_info.y, input.skin_info.z, input.skin_info.w);
    out.flat_1 = vec4(input.tint, input.overlay_bits, input.light, input.multitexture_layers.x);
    out.flat_2 = vec4(input.multitexture_layers.y, input.material, input.material_parameters.x, input.material_parameters.y);
    out.flat_3 = vec4(input.chart_bounds.x, input.chart_bounds.y, input.chart_bounds.z, input.chart_bounds.w);
    return out;
}
@if(WEBGPU_COMPATIBILITY && !ACTOR_MOTION && ALPHA_TO_COVERAGE)
fn unpack_stage(input: StageOutput) -> VertexOutput {
    var out: VertexOutput;
    out.position = input.position;
    out.uv = vec4(input.centroid_0.x, input.centroid_0.y, input.centroid_0.z, input.centroid_0.w);
    out.skin_info = vec4(input.flat_0.x, input.flat_0.y, input.flat_0.z, input.flat_0.w);
    out.world_normal = vec3(input.smooth_0.x, input.smooth_0.y, input.smooth_0.z);
    out.back_uv = vec2(input.centroid_1.x, input.centroid_1.y);
    out.tint = input.flat_1.x;
    out.overlay = unpack4x8unorm(input.flat_1.y);
    out.overlay_bits = input.flat_1.y;
    out.light = input.flat_1.z;
    out.world_position = vec3(input.smooth_0.w, input.smooth_1.x, input.smooth_1.y);
    out.multitexture_layers = vec2(input.flat_1.w, input.flat_2.x);
    out.native_lighting = vec3(input.smooth_1.z, input.smooth_1.w, input.smooth_2.x);
    out.material = input.flat_2.y;
    out.material_parameters = vec2(input.flat_2.z, input.flat_2.w);
    out.back_native_lighting = vec3(input.smooth_2.y, input.smooth_2.z, input.smooth_2.w);
    out.chart_bounds = vec4(input.flat_3.x, input.flat_3.y, input.flat_3.z, input.flat_3.w);
    return out;
}

@if(WEBGPU_COMPATIBILITY && ACTOR_MOTION && ALPHA_TO_COVERAGE)
struct StageOutput {
    @builtin(position) @invariant position: vec4<f32>,
    @location(0) @interpolate(perspective, centroid) centroid_0: vec4<f32>,
    @location(1) @interpolate(perspective, centroid) centroid_1: vec4<f32>,
    @location(2) smooth_0: vec4<f32>,
    @location(3) smooth_1: vec4<f32>,
    @location(4) smooth_2: vec4<f32>,
    @location(5) smooth_3: vec4<f32>,
    @location(6) smooth_4: vec4<f32>,
    @location(7) @interpolate(flat, either) flat_0: vec4<u32>,
    @location(8) @interpolate(flat, either) flat_1: vec4<u32>,
    @location(9) @interpolate(flat, either) flat_2: vec4<u32>,
    @location(10) @interpolate(flat, either) flat_3: vec4<u32>,
}
@if(WEBGPU_COMPATIBILITY && ACTOR_MOTION && ALPHA_TO_COVERAGE)
fn pack_stage(input: VertexOutput) -> StageOutput {
    var out: StageOutput;
    out.position = input.position;
    out.centroid_0 = vec4(input.uv.x, input.uv.y, input.uv.z, input.uv.w);
    out.centroid_1 = vec4(input.back_uv.x, input.back_uv.y, 0.0, 0.0);
    out.smooth_0 = vec4(input.current_clip.x, input.current_clip.y, input.current_clip.z, input.current_clip.w);
    out.smooth_1 = vec4(input.world_position.x, input.world_position.y, input.world_position.z, input.native_lighting.x);
    out.smooth_2 = vec4(input.native_lighting.y, input.native_lighting.z, input.back_native_lighting.x, input.back_native_lighting.y);
    out.smooth_3 = vec4(input.back_native_lighting.z, input.previous_clip.x, input.previous_clip.y, input.previous_clip.z);
    out.smooth_4 = vec4(input.previous_clip.w, 0.0, 0.0, 0.0);
    out.flat_0 = vec4(input.skin_info.x, input.skin_info.y, input.skin_info.z, input.skin_info.w);
    out.flat_1 = vec4(input.tint, input.overlay_bits, input.light, input.multitexture_layers.x);
    out.flat_2 = vec4(input.multitexture_layers.y, input.material, input.material_parameters.x, input.material_parameters.y);
    out.flat_3 = vec4(input.chart_bounds.x, input.chart_bounds.y, input.chart_bounds.z, input.chart_bounds.w);
    return out;
}
@if(WEBGPU_COMPATIBILITY && ACTOR_MOTION && ALPHA_TO_COVERAGE)
fn unpack_stage(input: StageOutput) -> VertexOutput {
    var out: VertexOutput;
    out.position = input.position;
    out.uv = vec4(input.centroid_0.x, input.centroid_0.y, input.centroid_0.z, input.centroid_0.w);
    out.skin_info = vec4(input.flat_0.x, input.flat_0.y, input.flat_0.z, input.flat_0.w);
    out.current_clip = vec4(input.smooth_0.x, input.smooth_0.y, input.smooth_0.z, input.smooth_0.w);
    out.back_uv = vec2(input.centroid_1.x, input.centroid_1.y);
    out.tint = input.flat_1.x;
    out.overlay = unpack4x8unorm(input.flat_1.y);
    out.overlay_bits = input.flat_1.y;
    out.light = input.flat_1.z;
    out.world_position = vec3(input.smooth_1.x, input.smooth_1.y, input.smooth_1.z);
    out.multitexture_layers = vec2(input.flat_1.w, input.flat_2.x);
    out.native_lighting = vec3(input.smooth_1.w, input.smooth_2.x, input.smooth_2.y);
    out.material = input.flat_2.y;
    out.material_parameters = vec2(input.flat_2.z, input.flat_2.w);
    out.back_native_lighting = vec3(input.smooth_2.z, input.smooth_2.w, input.smooth_3.x);
    out.chart_bounds = vec4(input.flat_3.x, input.flat_3.y, input.flat_3.z, input.flat_3.w);
    out.previous_clip = vec4(input.smooth_3.y, input.smooth_3.z, input.smooth_3.w, input.smooth_4.x);
    return out;
}
