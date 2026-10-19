// The Swingshot's rope (crate::moby_attach `rope_draw`, the game's `0x2dba30`): StandardMaterial's own fragment,
// this vertex stage only. Bevy's mesh.wgsl vertex with the position read from a storage buffer.
//
// Per vertex (persistent mesh, `rope_mesh`): position.x = quad << 2 | corner, UV the corner's (0,0), (1,0), (0,1),
// (1,1). Per quad (storage buffer, rewritten while the rope shows): its four corners in Bevy world, in the strip
// order v0 e0 v1 e1. Quads past `count` collapse onto one point outside the clip volume (no fragments).

#import bevy_pbr::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
}

struct RopeQuads {
    count: vec4<u32>,
    corners: array<vec4<f32>>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<storage, read> rope: RopeQuads;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let tag = u32(vertex.position.x);
    if ((tag >> 2u) >= rope.count.x) {
        out.position = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        return out;
    }
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(rope.corners[tag].xyz, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(vertex.instance_index, world_from_local[3]);
#endif
    return out;
}
