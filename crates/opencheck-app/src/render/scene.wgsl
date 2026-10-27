// Shading for federated IFC models.
//
// Every vertex carries the slot of its element. The per-slot display mode
// lives in a storage buffer, so hiding, ghosting or selecting elements only
// rewrites that buffer, never the geometry.

struct Camera {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    section_point: vec4<f32>,
    section_normal: vec4<f32>,
    // x: section box enabled (0/1), y: ghost alpha
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<storage, read> element_modes: array<u32>;

const MODE_HIDDEN: u32 = 0u;
const MODE_NORMAL: u32 = 1u;
const MODE_GHOST: u32 = 2u;
const MODE_SELECTED: u32 = 3u;
const MODE_HOVER: u32 = 4u;
const MODE_FOCUS_A: u32 = 5u;
const MODE_FOCUS_B: u32 = 6u;
// Flag: drawn in the sRGB colour in the low 24 bits (from a visual check).
const MODE_PAINTED: u32 = 0x80000000u;

// Selection green, as in Solibri.
const SELECTED_COLOR: vec3<f32> = vec3<f32>(0.0, 0.80, 0.0);
const GHOST_COLOR: vec3<f32> = vec3<f32>(0.62, 0.66, 0.72);
const FOCUS_A_COLOR: vec3<f32> = vec3<f32>(0.910, 0.396, 0.059);
const FOCUS_B_COLOR: vec3<f32> = vec3<f32>(0.122, 0.373, 0.820);

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec4<f32>,
    @location(2) color: vec4<f32>,
    @location(3) slot: u32,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

fn styled_color(input: VertexIn) -> vec4<f32> {
    let mode = element_modes[input.slot];
    if (mode & MODE_PAINTED) != 0u {
        let rgb = vec3<f32>(
            f32((mode >> 16u) & 0xffu),
            f32((mode >> 8u) & 0xffu),
            f32(mode & 0xffu),
        ) / 255.0;
        return vec4<f32>(rgb, 1.0);
    }
    switch mode {
        case MODE_GHOST: {
            return vec4<f32>(GHOST_COLOR, camera.params.y);
        }
        case MODE_SELECTED: {
            return vec4<f32>(SELECTED_COLOR, 1.0);
        }
        case MODE_FOCUS_A: {
            return vec4<f32>(FOCUS_A_COLOR, 1.0);
        }
        case MODE_FOCUS_B: {
            return vec4<f32>(FOCUS_B_COLOR, 1.0);
        }
        case MODE_HOVER: {
            return vec4<f32>(mix(input.color.rgb, SELECTED_COLOR, 0.35), input.color.a);
        }
        default: {
            return input.color;
        }
    }
}

fn shade_vertex(input: VertexIn, color: vec4<f32>) -> VertexOut {
    var out: VertexOut;
    out.clip = camera.view_proj * vec4<f32>(input.position, 1.0);
    out.world = input.position;
    out.normal = input.normal.xyz;
    out.color = color;
    return out;
}

// A triangle whose vertices all land on the same point outside the clip
// volume is dropped by the rasteriser before any fragment work.
fn culled() -> VertexOut {
    var out: VertexOut;
    out.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    out.world = vec3<f32>(0.0);
    out.normal = vec3<f32>(0.0, 0.0, 1.0);
    out.color = vec4<f32>(0.0);
    return out;
}

@vertex
fn vs_opaque(input: VertexIn) -> VertexOut {
    if element_modes[input.slot] == MODE_HIDDEN {
        return culled();
    }
    let color = styled_color(input);
    if color.a < 0.99 {
        return culled();
    }
    return shade_vertex(input, color);
}

@vertex
fn vs_transparent(input: VertexIn) -> VertexOut {
    if element_modes[input.slot] == MODE_HIDDEN {
        return culled();
    }
    let color = styled_color(input);
    if color.a >= 0.99 {
        return culled();
    }
    return shade_vertex(input, color);
}

fn lit(input: VertexOut) -> vec3<f32> {
    if camera.params.x > 0.5 {
        if dot(input.world - camera.section_point.xyz, camera.section_normal.xyz) > 0.0 {
            discard;
        }
    }
    // IFC winding is not reliable, so light both sides of every face.
    var n = normalize(input.normal);
    let to_eye = normalize(camera.eye.xyz - input.world);
    if dot(n, to_eye) < 0.0 {
        n = -n;
    }
    let sun = normalize(vec3<f32>(0.35, 0.55, 0.76));
    let key = max(dot(n, sun), 0.0);
    let head = max(dot(n, to_eye), 0.0);
    // Sky above, ground bounce below.
    let ambient = mix(0.42, 0.58, n.z * 0.5 + 0.5);
    let shade = ambient + 0.32 * key + 0.18 * head;
    return input.color.rgb * shade;
}

@fragment
fn fs_main(input: VertexOut) -> @location(0) vec4<f32> {
    let rgb = lit(input);
    // The egui framebuffer is not sRGB: colours are written gamma encoded,
    // and IFC surface colours are already given in that space.
    return vec4<f32>(rgb * input.color.a, input.color.a);
}
