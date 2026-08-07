override COMMIT: u32 = 0u;
override NODE_SUFFIX: u32 = 0u;
override LEADING_ZERO_BITS: u32 = 64u;
override WORKGROUP_SIZE: u32 = 128u;
override CANDIDATES_PER_INVOCATION: u32 = 8u;

struct Params {
    base_lo: u32,
    base_hi: u32,
    candidate_count: u32,
    reserved: u32,
};

struct SearchResult {
    min_offset: atomic<u32>,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var<storage, read> params: Params;
@group(0) @binding(1) var<storage, read_write> search_result: SearchResult;

fn add64(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
    let low = a.y + b.y;
    return vec2<u32>(a.x + b.x + select(0u, 1u, low < a.y), low);
}

fn xor64(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
    return a ^ b;
}

fn shr64(value: vec2<u32>, amount: u32) -> vec2<u32> {
    if (amount == 0u) { return value; }
    if (amount < 32u) {
        return vec2<u32>(value.x >> amount, (value.y >> amount) | (value.x << (32u - amount)));
    }
    if (amount == 32u) { return vec2<u32>(0u, value.x); }
    return vec2<u32>(0u, value.x >> (amount - 32u));
}

fn rotr64(value: vec2<u32>, amount: u32) -> vec2<u32> {
    let shift = amount & 31u;
    if (shift == 0u) {
        return select(value, value.yx, amount == 32u);
    }
    if (amount < 32u) {
        return vec2<u32>((value.x >> shift) | (value.y << (32u - shift)), (value.y >> shift) | (value.x << (32u - shift)));
    }
    return vec2<u32>((value.y >> shift) | (value.x << (32u - shift)), (value.x >> shift) | (value.y << (32u - shift)));
}

fn big_sigma0(value: vec2<u32>) -> vec2<u32> { return xor64(xor64(rotr64(value, 28u), rotr64(value, 34u)), rotr64(value, 39u)); }
fn big_sigma1(value: vec2<u32>) -> vec2<u32> { return xor64(xor64(rotr64(value, 14u), rotr64(value, 18u)), rotr64(value, 41u)); }
fn small_sigma0(value: vec2<u32>) -> vec2<u32> { return xor64(xor64(rotr64(value, 1u), rotr64(value, 8u)), shr64(value, 7u)); }
fn small_sigma1(value: vec2<u32>) -> vec2<u32> { return xor64(xor64(rotr64(value, 19u), rotr64(value, 61u)), shr64(value, 6u)); }
fn choice(e: vec2<u32>, f: vec2<u32>, g: vec2<u32>) -> vec2<u32> { return (e & f) ^ ((~e) & g); }
fn majority(a: vec2<u32>, b: vec2<u32>, c: vec2<u32>) -> vec2<u32> { return (a & b) ^ (a & c) ^ (b & c); }

fn add_index(base: vec2<u32>, offset: u32) -> vec2<u32> {
    let low = base.y + offset;
    return vec2<u32>(base.x + select(0u, 1u, low < base.y), low);
}

fn candidate_words(index: vec2<u32>) -> array<vec2<u32>, 2> {
    let variant_tail = (index.y >> 16u) & 0xfffu;
    let variant_head = (index.y >> 28u) & 0x3u;
    let time_high = ((index.x << 2u) | (index.y >> 30u)) & 0xfffu;
    let time_mid = (index.x >> 10u) & 0xffffu;
    let word0 = vec2<u32>(COMMIT, (time_mid << 16u) | 0x4000u | time_high);
    let variant = ((8u + variant_head) << 12u) | variant_tail;
    let word1 = vec2<u32>((variant << 16u) | (index.y & 0xffffu), NODE_SUFFIX);
    return array<vec2<u32>, 2>(word0, word1);
}

fn sha512_first_word(index: vec2<u32>) -> vec2<u32> {
    let candidate = candidate_words(index);
    var w0 = candidate[0]; var w1 = candidate[1];
    var w2 = vec2<u32>(0x80000000u, 0u);
    var w3 = vec2<u32>(0u); var w4 = vec2<u32>(0u); var w5 = vec2<u32>(0u);
    var w6 = vec2<u32>(0u); var w7 = vec2<u32>(0u); var w8 = vec2<u32>(0u);
    var w9 = vec2<u32>(0u); var w10 = vec2<u32>(0u); var w11 = vec2<u32>(0u);
    var w12 = vec2<u32>(0u); var w13 = vec2<u32>(0u); var w14 = vec2<u32>(0u);
    var w15 = vec2<u32>(0u, 128u);
    var a = vec2<u32>(0x6a09e667u, 0xf3bcc908u); var b = vec2<u32>(0xbb67ae85u, 0x84caa73bu);
    var c = vec2<u32>(0x3c6ef372u, 0xfe94f82bu); var d = vec2<u32>(0xa54ff53au, 0x5f1d36f1u);
    var e = vec2<u32>(0x510e527fu, 0xade682d1u); var f = vec2<u32>(0x9b05688cu, 0x2b3e6c1fu);
    var g = vec2<u32>(0x1f83d9abu, 0xfb41bd6bu); var h = vec2<u32>(0x5be0cd19u, 0x137e2179u);
    // @SHA512_UNROLLED_ROUNDS@
}

fn matches_leading_zero_bits(word: vec2<u32>) -> bool {
    if (LEADING_ZERO_BITS == 0u) { return true; }
    if (LEADING_ZERO_BITS <= 32u) { return (word.x >> ((32u - LEADING_ZERO_BITS) & 31u)) == 0u; }
    return word.x == 0u && (word.y >> ((64u - LEADING_ZERO_BITS) & 31u)) == 0u;
}

@compute @workgroup_size(WORKGROUP_SIZE)
fn search_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let invocation_offset = gid.x * CANDIDATES_PER_INVOCATION;
    var best = 0xffffffffu;
    for (var offset = 0u; offset < CANDIDATES_PER_INVOCATION; offset += 1u) {
        let local = invocation_offset + offset;
        if (local >= params.candidate_count) { break; }
        if (matches_leading_zero_bits(sha512_first_word(add_index(vec2<u32>(params.base_hi, params.base_lo), local)))) {
            best = local;
            break;
        }
    }
    if (best != 0xffffffffu) { atomicMin(&search_result.min_offset, best); }
}
