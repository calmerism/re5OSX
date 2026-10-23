// tiled_lighting's DepthDownsampleCS (_Default 16x16 and _B 8x8, as
// translated) with its reduction made of subgroup operations: the
// translation keeps the tile's minimum, maxima and count in workgroup
// atomics reduced over 8 (6) rounds, each with a barrier, which tilers run
// slowly (4-5 ms a frame on a Mali). Min, max and add of integers in any
// order: the same results, bit for bit. @TILE@ and @THREADS@ are the tile's
// side and threads (one dimension: subgroup_id needs it).
diagnostic(off, derivative_uniformity);

struct cb18_struct {
  tint_symbol : array<vec4<f32>, 18u>,
}

@group(0u) @binding(11u) var<uniform> cb11_11 : cb18_struct;

struct cb4_struct {
  tint_symbol_1 : array<vec4<f32>, 4u>,
}

@group(0u) @binding(10u) var<uniform> cb10_10 : cb4_struct;

@group(0u) @binding(39u) var t7 : texture_2d<f32>;

@group(0u) @binding(40u) var t8 : texture_2d<f32>;

@group(0u) @binding(161u) var u1 : texture_storage_2d<rgba32float, write>;

var<workgroup> g0 : array<u32, 1536u>;

struct tint_symbol_3 {
  tint_symbol_2 : array<vec4<u32>, 6u>,
}

@group(0u) @binding(15u) var<uniform> v : tint_symbol_3;

@compute @workgroup_size(@THREADS@u, 1u, 1u)
fn main(@builtin(workgroup_id) vThreadGroupID : vec3<u32>, @builtin(local_invocation_index) g5_li : u32, @builtin(subgroup_invocation_id) g5_lane : u32, @builtin(subgroup_id) g5_sg : u32, @builtin(num_subgroups) g5_nsg : u32) {
  let vThreadIDInGroup = vec3<u32>(g5_li % @TILE@u, g5_li / @TILE@u, 0u);
  let v_1 = vec3<i32>(vThreadIDInGroup);
  let v_2 = vec3<i32>(vThreadGroupID);
  var v_3 : f32 = 0.0f;
  var v_4 : f32 = 0.0f;
  var v_5 : f32 = 0.0f;
  var v_6 : f32 = 0.0f;
  var v_7 : f32 = 0.0f;
  var v_8 : f32 = 0.0f;
  var v_9 : f32 = 0.0f;
  var v_10 : f32 = 0.0f;
  var v_11 : f32 = 0.0f;
  var v_12 : f32 = 0.0f;
  var v_13 : f32 = 0.0f;
  var v_14 : f32 = 0.0f;
  v_3 = bitcast<f32>(((v_1.y * @TILE@i) + v_1.x));
  let v_15 = bitcast<vec2<f32>>(((v_2.xy * vec2<i32>(@TILE@i)) + v_1.xy));
  let v_16 = vec4<f32>(v_15.xy, vec4<f32>(v_7, v_8, v_9, v_10).zw);
  v_7 = v_16.x;
  v_8 = v_16.y;
  v_9 = v_16.z;
  v_10 = v_16.w;
  let v_17 = bitcast<vec2<f32>>(select(vec2<u32>(), vec2<u32>(4294967295u), (bitcast<vec2<u32>>(vec4<f32>(v_7, v_8, v_9, v_10).xy) < bitcast<vec2<u32>>(cb10_10.tint_symbol_1[3u].xy))));
  let v_18 = vec4<f32>(v_17.xy, vec4<f32>(v_11, v_12, v_13, v_14).zw);
  v_11 = v_18.x;
  v_12 = v_18.y;
  v_13 = v_18.z;
  v_14 = v_18.w;
  v_11 = bitcast<f32>((bitcast<u32>(v_12) & bitcast<u32>(v_11)));
  if ((bitcast<u32>(v_11) != 0u)) {
    let v_19 = vec4<f32>(vec4<f32>(v_7, v_8, v_9, v_10).xy, vec2<f32>());
    v_7 = v_19.x;
    v_8 = v_19.y;
    v_9 = v_19.z;
    v_10 = v_19.w;
    v_11 = textureLoad(t7, bitcast<vec2<i32>>(vec4<f32>(v_7, v_8, v_9, v_10).xy), bitcast<i32>(v_10)).x;
    v_11 = ((-(vec4<f32>(v_11, v_12, v_13, v_14).xxxx)).x + 1.0f);
    let v_20 = textureLoad(t8, bitcast<vec2<i32>>(vec4<f32>(v_7, v_8, v_9, v_10).xy), bitcast<i32>(v_10)).xw;
    let v_21 = vec4<f32>(v_20.xy, vec4<f32>(v_7, v_8, v_9, v_10).zw);
    v_7 = v_21.x;
    v_8 = v_21.y;
    v_9 = v_21.z;
    v_10 = v_21.w;
    v_9 = bitcast<f32>(select(0u, 4294967295u, (v_11 < 1.0f)));
    v_10 = bitcast<f32>((bitcast<u32>(v_9) & 1u));
    let v_22 = bitcast<vec2<f32>>((bitcast<vec2<i32>>(vec4<f32>(v_7, v_8, v_9, v_10).xy) * bitcast<vec2<i32>>(vec4<f32>(v_7, v_8, v_9, v_10).ww)));
    let v_23 = vec4<f32>(v_11, v_12, v_13, v_14);
    let v_24 = vec4<f32>(v_23.x, v_22.xy, v_23.w);
    v_11 = v_24.x;
    v_12 = v_24.y;
    v_13 = v_24.z;
    v_14 = v_24.w;
    let v_25 = (bitcast<u32>(v_9) != 0u);
    v_14 = select(bitcast<f32>(v.tint_symbol_2[0i].x), 0.0f, v_25);
    let v_26 = vec4<f32>(v_11, v_12, v_13, v_14);
    let v_27 = vec4<f32>(v_26.zw, vec4<f32>(v_7, v_8, v_9, v_10).zw);
    v_7 = v_27.x;
    v_8 = v_27.y;
    v_9 = v_27.z;
    v_10 = v_27.w;
    let v_28 = vec4<f32>(v_11, v_12, v_13, v_14).xxyz;
    v_11 = v_28.x;
    v_12 = v_28.y;
    v_13 = v_28.z;
    v_14 = v_28.w;
  } else {
    let v_29 = vec4<f32>(vec2<f32>(1073741824.0f, 0.0f), vec4<f32>(v_7, v_8, v_9, v_10).zw);
    v_7 = v_29.x;
    v_8 = v_29.y;
    v_9 = v_29.z;
    v_10 = v_29.w;
    v_11 = 1073741824.0f;
    v_12 = 0.0f;
    v_13 = 0.0f;
    v_14 = 0.0f;
  }
  let g5_tid = bitcast<u32>(v_3);
  let g5_a = bitcast<vec4<u32>>(vec4<f32>(v_11, v_12, v_13, v_14));
  let g5_b = bitcast<u32>(v_8);
  let g5_r = vec4<u32>(subgroupMin(g5_a.x), subgroupMax(g5_a.y), subgroupMax(g5_a.z), subgroupMax(g5_a.w));
  let g5_n = subgroupAdd(g5_b);
  if (g5_lane == 0u) {
    g0[g5_sg * 6u + 0u] = g5_r.x;
    g0[g5_sg * 6u + 1u] = g5_r.y;
    g0[g5_sg * 6u + 2u] = g5_r.z;
    g0[g5_sg * 6u + 3u] = g5_r.w;
    g0[g5_sg * 6u + 5u] = g5_n;
  }
  workgroupBarrier();
  if (g5_tid == 0u) {
    for (var i = 1u; i < g5_nsg; i++) {
      g0[0u] = min(g0[0u], g0[i * 6u + 0u]);
      g0[1u] = max(g0[1u], g0[i * 6u + 1u]);
      g0[2u] = max(g0[2u], g0[i * 6u + 2u]);
      g0[3u] = max(g0[3u], g0[i * 6u + 3u]);
      g0[5u] = g0[5u] + g0[i * 6u + 5u];
    }
  }
  v_7 = v_3;
  if ((bitcast<u32>(v_7) != 0u)) {
  } else {
    let v_39 = bitcast<vec4<f32>>(vec4<u32>(g0[0u], g0[1u], g0[2u], g0[3u]));
    v_3 = v_39.x;
    v_4 = v_39.y;
    v_5 = v_39.z;
    v_6 = v_39.w;
    let v_40 = (vec4<f32>(v_3, v_4, v_5, v_6).xy + cb11_11.tint_symbol[17u].ww);
    let v_41 = vec4<f32>(v_40.xy, vec4<f32>(v_3, v_4, v_5, v_6).zw);
    v_3 = v_41.x;
    v_4 = v_41.y;
    v_5 = v_41.z;
    v_6 = v_41.w;
    let v_42 = (cb11_11.tint_symbol[17u].zz / vec4<f32>(v_3, v_4, v_5, v_6).xy);
    let v_43 = vec4<f32>(v_42.xy, vec4<f32>(v_7, v_8, v_9, v_10).zw);
    v_7 = v_43.x;
    v_8 = v_43.y;
    v_9 = v_43.z;
    v_10 = v_43.w;
    v_3 = bitcast<f32>(g0[5u]);
    v_3 = bitcast<f32>(select(0u, 4294967295u, (bitcast<i32>(v_3) == @THREADS@i)));
    let v_44 = bitcast<vec2<u32>>(vec4<f32>(v_3, v_4, v_5, v_6).xx);
    let v_45 = select(vec4<f32>(v_3, v_4, v_5, v_6).zw, vec2<f32>(-1.0f, 0.0f), (v_44 != vec2<u32>()));
    let v_46 = vec4<f32>(vec4<f32>(v_7, v_8, v_9, v_10).xy, v_45.xy);
    v_7 = v_46.x;
    v_8 = v_46.y;
    v_9 = v_46.z;
    v_10 = v_46.w;
    textureStore(u1, v_2.xy, vec4<f32>(v_7, v_8, v_9, v_10));
  }
}
