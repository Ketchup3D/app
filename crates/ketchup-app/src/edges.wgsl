struct EdgeVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) lateral: f32,
    @location(2) shadow_color: vec4<f32>,
    @location(3) uv: vec2<f32>,
}

fn edge_vertex(position: vec3<f32>, color: vec4<f32>, lateral: f32, shadow_color: vec4<f32>) -> EdgeVertex {
    var output: EdgeVertex;
    output.position = vec4<f32>(position, 1.0);
    output.color = color;
    output.lateral = lateral;
    output.shadow_color = shadow_color;
    output.uv = position.xy * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    return output;
}

@vertex
fn vertex_main(@location(0) position: vec3<f32>, @location(1) color: vec4<f32>, @location(2) lateral: f32, @location(3) shadow_color: vec4<f32>) -> EdgeVertex {
    return edge_vertex(position, color, lateral, shadow_color);
}

@vertex
fn vertex_main_srgb(@location(0) position: vec3<f32>, @location(1) color: vec4<f32>, @location(2) lateral: f32, @location(3) shadow_color: vec4<f32>) -> EdgeVertex {
    // Unorm8x4 preserves the original Color32 bytes. Convert on the GPU for
    // sRGB targets instead of copying and converting the entire mesh on CPU.
    let linear = select(color.rgb / 12.92, pow((color.rgb + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)), color.rgb > vec3<f32>(0.04045));
    let shadow_linear = select(shadow_color.rgb / 12.92, pow((shadow_color.rgb + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)), shadow_color.rgb > vec3<f32>(0.04045));
    return edge_vertex(position, vec4<f32>(linear, color.a), lateral, vec4<f32>(shadow_linear, shadow_color.a));
}

fn stroke_coverage(input: EdgeVertex) -> f32 {
    // The CPU expands the strip by this pixel footprint, including diagonals.
    // Limit subpixel strokes to their true coverage rather than darkening them.
    let gradient = max(fwidth(input.lateral), 0.0001);
    let peak = min(1.0, 2.0 / gradient - 1.0);
    let coverage = clamp((1.0 - abs(input.lateral)) / gradient, 0.0, peak);
    return coverage;
}

@fragment
fn fragment_edge_depth(input: EdgeVertex) -> @location(0) vec4<f32> {
    // Resolve the entire visible footprint, including the antialias fringe.
    // Otherwise rear selection strokes leak through the fringe of nearer edges.
    // Screen overlays and translucent strokes do not occlude world geometry.
    let coverage = stroke_coverage(input);
    if input.position.z <= 0.0 || input.position.z >= 1.0 ||
       input.color.a < 1.0 || coverage < 0.001 { discard; }
    return vec4<f32>(0.0);
}

@fragment
fn fragment_main(input: EdgeVertex) -> @location(0) vec4<f32> {
    let coverage = stroke_coverage(input);
    if coverage < 0.001 { discard; }
    return input.color * coverage;
}

// Multisample a scalar shadow mask, then shade the original single-sample
// receiving faces. Neither face silhouettes nor viewport strokes are resolved.
@group(0) @binding(0) var shadow_mask: texture_2d<f32>;
@group(0) @binding(1) var shadow_sampler: sampler;

@fragment
fn fragment_mask_base() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}

@fragment
fn fragment_mask_shadow() -> @location(0) vec4<f32> {
    return vec4<f32>(1.0);
}

@fragment
fn fragment_shadow_faces(input: EdgeVertex) -> @location(0) vec4<f32> {
    let coverage = textureSample(shadow_mask, shadow_sampler, input.uv).r;
    return mix(input.color, input.shadow_color, coverage * select(0.0, 1.0, input.shadow_color.a > 0.0));
}
