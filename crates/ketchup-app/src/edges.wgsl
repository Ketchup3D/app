struct EdgeVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) lateral: f32,
}

fn edge_vertex(position: vec3<f32>, color: vec4<f32>, lateral: f32) -> EdgeVertex {
    var output: EdgeVertex;
    output.position = vec4<f32>(position, 1.0);
    output.color = color;
    output.lateral = lateral;
    return output;
}

@vertex
fn vertex_main(@location(0) position: vec3<f32>, @location(1) color: vec4<f32>, @location(2) lateral: f32) -> EdgeVertex {
    return edge_vertex(position, color, lateral);
}

@vertex
fn vertex_main_srgb(@location(0) position: vec3<f32>, @location(1) color: vec4<f32>, @location(2) lateral: f32) -> EdgeVertex {
    // Unorm8x4 preserves the original Color32 bytes. Convert on the GPU for
    // sRGB targets instead of copying and converting the entire mesh on CPU.
    let linear = select(color.rgb / 12.92, pow((color.rgb + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)), color.rgb > vec3<f32>(0.04045));
    return edge_vertex(position, vec4<f32>(linear, color.a), lateral);
}

@fragment
fn fragment_main(input: EdgeVertex) -> @location(0) vec4<f32> {
    // The CPU expands the strip by this pixel footprint, including diagonals.
    // Limit subpixel strokes to their true coverage rather than darkening them.
    let gradient = max(fwidth(input.lateral), 0.0001);
    let peak = min(1.0, 2.0 / gradient - 1.0);
    let coverage = clamp((1.0 - abs(input.lateral)) / gradient, 0.0, peak);
    if coverage < 0.001 { discard; }
    return input.color * coverage;
}
