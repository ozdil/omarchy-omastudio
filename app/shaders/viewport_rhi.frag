#version 440

layout(location = 0) in vec2 v_uv;
layout(location = 0) out vec4 fragColor;

layout(binding = 1) uniform sampler2D u_tex;

void main() {
    vec3 rgb = texture(u_tex, v_uv).rgb;
    fragColor = vec4(rgb, 1.0);
}
