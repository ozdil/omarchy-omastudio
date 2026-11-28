#version 440

layout(location = 0) out vec2 v_uv;

layout(std140, binding = 0) uniform Params {
    vec2 scale;
    float y_flip;
    float _pad;
} u_params;

void main() {
    // Fullscreen triangle strip (4 vertices): (-1,-1), (1,-1), (-1,1), (1,1)
    vec2 pos = vec2(
        (gl_VertexIndex & 1) != 0 ? 1.0 : -1.0,
        (gl_VertexIndex & 2) != 0 ? 1.0 : -1.0
    );
    vec2 uv = pos * 0.5 + 0.5;
    v_uv = vec2(uv.x, u_params.y_flip > 0.5 ? (1.0 - uv.y) : uv.y);
    gl_Position = vec4(pos * u_params.scale, 0.0, 1.0);
}
