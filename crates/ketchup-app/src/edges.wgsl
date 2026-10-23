struct EdgeVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) lateral: f32,
}

@vertex
fn vertex_main(@location(0) position: vec2<f32>, @location(1) color: vec4<f32>, @location(2) lateral: f32) -> EdgeVertex {
    var output: EdgeVertex;
    output.position = vec4<f32>(position, 0.0, 1.0);
    output.color = color;
    output.lateral = lateral;
    return output;
}

@fragment
fn fragment_main(input: EdgeVertex) -> @location(0) vec4<f32> {
    let coverage = clamp((1.0 - abs(input.lateral)) / max(fwidth(input.lateral), 0.0001), 0.0, 1.0);
    return input.color * coverage;
}
