override COMMIT: u32 = 0u;
override NODE_SUFFIX: u32 = 0u;
override LEADING_ZERO_BITS: u32 = 64u;
override WORKGROUP_SIZE: u32 = 256u;
override CANDIDATES_PER_INVOCATION: u32 = 32u;

struct Params {
    base_lo: u32,
    base_hi: u32,
    candidate_count: u32,
    reserved: u32,
};

struct SearchResult {
    claimed: atomic<u32>,
    index_lo: u32,
    index_hi: u32,
    processed: atomic<u32>,
};

@group(0) @binding(0)
var<storage, read> params: Params;

@group(0) @binding(1)
var<storage, read_write> search_result: SearchResult;

@group(0) @binding(2)
var<storage, read_write> hash_results: array<u64>;

var<workgroup> search_workgroup_active: u32;

fn rotr(value: u64, amount: u32) -> u64 {
    return (value >> amount) | (value << (64u - amount));
}

fn big_sigma0(value: u64) -> u64 {
    return rotr(value, 28u) ^ rotr(value, 34u) ^ rotr(value, 39u);
}

fn big_sigma1(value: u64) -> u64 {
    return rotr(value, 14u) ^ rotr(value, 18u) ^ rotr(value, 41u);
}

fn small_sigma0(value: u64) -> u64 {
    return rotr(value, 1u) ^ rotr(value, 8u) ^ (value >> 7u);
}

fn small_sigma1(value: u64) -> u64 {
    return rotr(value, 19u) ^ rotr(value, 61u) ^ (value >> 6u);
}

fn candidate_words(index: u64) -> vec2<u64> {
    let node_prefix = index & 0xfffflu;
    let variant_tail = (index >> 16u) & 0xffflu;
    let variant_head = (index >> 28u) & 0x3lu;
    let time_high = (index >> 30u) & 0xffflu;
    let time_mid = (index >> 42u) & 0xfffflu;

    let word0 = (u64(COMMIT) << 32u) | (time_mid << 16u) | 0x4000lu | time_high;
    let variant = ((8lu + variant_head) << 12u) | variant_tail;
    let word1 = (variant << 48u) | (node_prefix << 32u) | u64(NODE_SUFFIX);
    return vec2<u64>(word0, word1);
}

fn sha512_first_word(index: u64) -> u64 {
    let candidate = candidate_words(index);
    var w0 = candidate.x;
    var w1 = candidate.y;
    var w2 = 0x8000000000000000lu;
    var w3 = 0lu;
    var w4 = 0lu;
    var w5 = 0lu;
    var w6 = 0lu;
    var w7 = 0lu;
    var w8 = 0lu;
    var w9 = 0lu;
    var w10 = 0lu;
    var w11 = 0lu;
    var w12 = 0lu;
    var w13 = 0lu;
    var w14 = 0lu;
    var w15 = 128lu;

    var a = 0x6a09e667f3bcc908lu;
    var b = 0xbb67ae8584caa73blu;
    var c = 0x3c6ef372fe94f82blu;
    var d = 0xa54ff53a5f1d36f1lu;
    var e = 0x510e527fade682d1lu;
    var f = 0x9b05688c2b3e6c1flu;
    var g = 0x1f83d9abfb41bd6blu;
    var h = 0x5be0cd19137e2179lu;

    // @SHA512_UNROLLED_ROUNDS@
}

fn matches_leading_zero_bits(word: u64) -> bool {
    if (LEADING_ZERO_BITS == 0u) {
        return true;
    }
    if (LEADING_ZERO_BITS > 64u) {
        return false;
    }
    return (word >> (64u - LEADING_ZERO_BITS)) == 0lu;
}

@compute @workgroup_size(WORKGROUP_SIZE)
fn search_main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_index) local_id: u32,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
) {
    // Admit the whole workgroup together so queued groups can stop after a hit.
    if (local_id == 0u) {
        let candidates_per_workgroup = WORKGROUP_SIZE * CANDIDATES_PER_INVOCATION;
        let group_start = workgroup_id.x * candidates_per_workgroup;
        let group_count = min(candidates_per_workgroup, params.candidate_count - group_start);
        search_workgroup_active = select(0u, 1u, atomicLoad(&search_result.claimed) == 0u);
        if (search_workgroup_active != 0u) {
            atomicAdd(&search_result.processed, group_count);
        }
    }
    workgroupBarrier();
    if (search_workgroup_active == 0u) {
        return;
    }

    let base = (u64(params.base_hi) << 32u) | u64(params.base_lo);
    let invocation_start = base + u64(gid.x) * u64(CANDIDATES_PER_INVOCATION);

    for (var offset = 0u; offset < CANDIDATES_PER_INVOCATION; offset = offset + 1u) {
        let local_index = gid.x * CANDIDATES_PER_INVOCATION + offset;
        if (local_index >= params.candidate_count) {
            break;
        }

        let index = invocation_start + u64(offset);
        if (matches_leading_zero_bits(sha512_first_word(index))) {
            var claim = atomicCompareExchangeWeak(&search_result.claimed, 0u, 1u);
            while (!claim.exchanged && claim.old_value == 0u) {
                claim = atomicCompareExchangeWeak(&search_result.claimed, 0u, 1u);
            }
            if (claim.exchanged) {
                search_result.index_lo = u32(index & 0xfffffffflu);
                search_result.index_hi = u32(index >> 32u);
            }
            break;
        }
    }
}

@compute @workgroup_size(256)
fn hash_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= params.candidate_count) {
        return;
    }
    let base = (u64(params.base_hi) << 32u) | u64(params.base_lo);
    hash_results[gid.x] = sha512_first_word(base + u64(gid.x));
}
