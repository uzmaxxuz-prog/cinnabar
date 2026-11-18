// Flat float payloads retain their bits; positional values keep perspective interpolation.
@if(!WEBGPU_COMPATIBILITY)
alias StageOutput = VertexOutput;
@if(!WEBGPU_COMPATIBILITY)
fn pack_stage(input: VertexOutput) -> StageOutput { return input; }
@if(!WEBGPU_COMPATIBILITY)
fn unpack_stage(input: StageOutput) -> VertexOutput { return input; }

@if(WEBGPU_COMPATIBILITY)
struct StageOutput {
    @builtin(position) @invariant clip_position: vec4<f32>,
    @location(0) smooth_0: vec4<f32>,
    @location(1) smooth_1: vec4<f32>,
    @location(2) smooth_2: vec4<f32>,
    @location(3) smooth_3: vec4<f32>,
    @location(4) @interpolate(flat, either) flat_0: vec4<u32>,
    @location(5) @interpolate(flat, either) flat_1: vec4<u32>,
    @if(!ENHANCED)
    @location(6) @interpolate(flat, either) flat_2: vec4<u32>,
}
@if(WEBGPU_COMPATIBILITY)
fn pack_stage(input: VertexOutput) -> StageOutput {
    var out: StageOutput;
    out.clip_position = input.clip_position;
    out.smooth_0 = vec4(input.uv, input.local_position.xy);
    out.smooth_1 = vec4(input.local_position.z, input.world_position);
    out.flat_0 = vec4(input.current_texture, input.quad_shape, input.material_flags, input.biome_record);
    out.flat_1 = vec4(input.next_texture, bitcast<u32>(input.frame_blend), 0u, 0u);
    @if(ENHANCED) {
        out.smooth_2 = vec4(input.lighting, input.sky_light);
        out.smooth_3 = vec4(input.ambient_occlusion, 0.0, 0.0, 0.0);
        out.flat_1.z = input.surface_class;
    }
    @if(!ENHANCED) {
        out.smooth_2 = vec4(input.native_light_levels, input.native_ao_face, 0.0);
        out.smooth_3 = vec4(0.0);
        out.flat_1.z = bitcast<u32>(input.uniform_tint_gamma.x);
        out.flat_1.w = bitcast<u32>(input.uniform_tint_gamma.y);
        out.flat_2 = vec4(bitcast<vec2<u32>>(input.uniform_tint_gamma.zw), 0u, 0u);
    }
    return out;
}
@if(WEBGPU_COMPATIBILITY)
fn unpack_stage(input: StageOutput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = input.clip_position;
    out.uv = input.smooth_0.xy;
    out.local_position = vec3(input.smooth_0.zw, input.smooth_1.x);
    out.world_position = input.smooth_1.yzw;
    out.current_texture = input.flat_0.x;
    out.quad_shape = input.flat_0.y;
    out.material_flags = input.flat_0.z;
    out.biome_record = input.flat_0.w;
    out.next_texture = input.flat_1.x;
    out.frame_blend = bitcast<f32>(input.flat_1.y);
    @if(ENHANCED) {
        out.lighting = input.smooth_2.xyz;
        out.sky_light = input.smooth_2.w;
        out.ambient_occlusion = input.smooth_3.x;
        out.surface_class = input.flat_1.z;
    }
    @if(!ENHANCED) {
        out.native_light_levels = input.smooth_2.xy;
        out.native_ao_face = input.smooth_2.z;
        out.uniform_tint_gamma = bitcast<vec4<f32>>(vec4(input.flat_1.zw, input.flat_2.xy));
    }
    return out;
}
