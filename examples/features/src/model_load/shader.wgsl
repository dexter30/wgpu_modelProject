struct VertexOutput {
    @location(0) tex_coord: vec2<f32>,
    @builtin(position) position: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> transform: mat4x4<f32>;

@group(0) @binding(1)
var base_color_texture: texture_2d<f32>;

@group(0) @binding(2)
var base_color_sampler: sampler;

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) tex_coord: vec2<f32>,
) -> VertexOutput {
    var out: VertexOutput;
    out.position = transform * vec4<f32>(position, 1.0);
    out.tex_coord = tex_coord;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(base_color_texture, base_color_sampler, in.tex_coord);
}

@fragment
fn fs_wire(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 1.0, 0.0, 1.0); // Green wireframe color
}
