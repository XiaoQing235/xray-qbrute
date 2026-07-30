use rayon::prelude::*;

use crate::candidate::{self, CandidateConfig};

use super::{
    BackendOutcome, ProgressReporter, SEARCH_CHUNK_SIZE, SearchConfig, SearchError, SearchHit,
};

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
const SHA512_IV0: u64 = 0x6a09e667f3bcc908;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
const SHA512_K: [u64; 80] = [
    0x428a2f98d728ae22,
    0x7137449123ef65cd,
    0xb5c0fbcfec4d3b2f,
    0xe9b5dba58189dbbc,
    0x3956c25bf348b538,
    0x59f111f1b605d019,
    0x923f82a4af194f9b,
    0xab1c5ed5da6d8118,
    0xd807aa98a3030242,
    0x12835b0145706fbe,
    0x243185be4ee4b28c,
    0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f,
    0x80deb1fe3b1696b1,
    0x9bdc06a725c71235,
    0xc19bf174cf692694,
    0xe49b69c19ef14ad2,
    0xefbe4786384f25e3,
    0x0fc19dc68b8cd5b5,
    0x240ca1cc77ac9c65,
    0x2de92c6f592b0275,
    0x4a7484aa6ea6e483,
    0x5cb0a9dcbd41fbd4,
    0x76f988da831153b5,
    0x983e5152ee66dfab,
    0xa831c66d2db43210,
    0xb00327c898fb213f,
    0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2,
    0xd5a79147930aa725,
    0x06ca6351e003826f,
    0x142929670a0e6e70,
    0x27b70a8546d22ffc,
    0x2e1b21385c26c926,
    0x4d2c6dfc5ac42aed,
    0x53380d139d95b3df,
    0x650a73548baf63de,
    0x766a0abb3c77b2a8,
    0x81c2c92e47edaee6,
    0x92722c851482353b,
    0xa2bfe8a14cf10364,
    0xa81a664bbc423001,
    0xc24b8b70d0f89791,
    0xc76c51a30654be30,
    0xd192e819d6ef5218,
    0xd69906245565a910,
    0xf40e35855771202a,
    0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8,
    0x1e376c085141ab53,
    0x2748774cdf8eeb99,
    0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63,
    0x4ed8aa4ae3418acb,
    0x5b9cca4f7763e373,
    0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc,
    0x78a5636f43172f60,
    0x84c87814a1f0ab72,
    0x8cc702081a6439ec,
    0x90befffa23631e28,
    0xa4506cebde82bde9,
    0xbef9a3f7b2c67915,
    0xc67178f2e372532b,
    0xca273eceea26619c,
    0xd186b8c721c0c207,
    0xeada7dd6cde0eb1e,
    0xf57d4f7fee6ed178,
    0x06f067aa72176fba,
    0x0a637dc5a2c898a6,
    0x113f9804bef90dae,
    0x1b710b35131c471b,
    0x28db77f523047d84,
    0x32caab7b40c72493,
    0x3c9ebe0a15c9bebc,
    0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6,
    0x597f299cfc657e2a,
    0x5fcb6fab3ad6faec,
    0x6c44198c4a475817,
];

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub fn is_available() -> bool {
    is_x86_feature_detected!("avx2")
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub fn is_available() -> bool {
    false
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub fn first_digest_words(indices: [u64; 4], config: CandidateConfig) -> [u64; 4] {
    assert!(is_available(), "AVX2 is not available on this CPU");
    unsafe { first_digest_words_avx2(indices, config) }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub fn first_digest_words(_: [u64; 4], _: CandidateConfig) -> [u64; 4] {
    unreachable!("AVX2 is not available on this architecture")
}

pub fn search(
    config: &SearchConfig,
    progress: &ProgressReporter,
) -> Result<BackendOutcome, SearchError> {
    if !is_available() {
        return Err(SearchError::Unavailable(
            "AVX2 is not supported by this CPU".to_owned(),
        ));
    }

    let chunk_count = config.max_index.div_ceil(SEARCH_CHUNK_SIZE);
    let mut evaluated = 0u64;

    for chunk in 0..chunk_count {
        let start = chunk * SEARCH_CHUNK_SIZE;
        let end = (start + SEARCH_CHUNK_SIZE).min(config.max_index);
        let vector_end = end - (end - start) % 4;
        let min_index = (0..(vector_end - start) / 4)
            .into_par_iter()
            .filter_map(|block| {
                let index = start + block * 4;
                let indices = [index, index + 1, index + 2, index + 3];
                let words = first_digest_words(indices, config.candidate);
                words
                    .iter()
                    .position(|&word| {
                        candidate::matches_leading_zero_bits(word, config.leading_zero_bits)
                    })
                    .map(|lane| indices[lane])
            })
            .min()
            .or_else(|| {
                (vector_end..end).find(|&index| {
                    let word = candidate::digest_word(&candidate::digest(index, config.candidate));
                    candidate::matches_leading_zero_bits(word, config.leading_zero_bits)
                })
            });
        evaluated += vector_end - start;
        if min_index.is_none() {
            evaluated += end - vector_end;
        }

        if let Some(index) = min_index {
            let processed = index + 1;
            progress(processed - start);
            return Ok(BackendOutcome {
                hit: Some(SearchHit { index }),
                processed,
                evaluated,
                backend_name: "avx2-4x".to_owned(),
                device_name: None,
                fallback_reasons: Vec::new(),
                search_elapsed: None,
                kernel_config: None,
                tuning_elapsed: None,
            });
        }

        progress(end - start);
    }

    Ok(BackendOutcome {
        hit: None,
        processed: config.max_index,
        evaluated,
        backend_name: "avx2-4x".to_owned(),
        device_name: None,
        fallback_reasons: Vec::new(),
        search_elapsed: None,
        kernel_config: None,
        tuning_elapsed: None,
    })
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn first_digest_words_avx2(indices: [u64; 4], config: CandidateConfig) -> [u64; 4] {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    macro_rules! rotr {
        ($value:expr, $right:literal) => {{
            let value = $value;
            _mm256_or_si256(
                _mm256_srli_epi64::<$right>(value),
                _mm256_slli_epi64::<{ 64 - $right }>(value),
            )
        }};
    }

    #[inline(always)]
    unsafe fn add3(a: __m256i, b: __m256i, c: __m256i) -> __m256i {
        unsafe { _mm256_add_epi64(_mm256_add_epi64(a, b), c) }
    }

    let candidate_words = indices.map(|index| candidate::candidate_words(index, config));
    let mut schedule = [_mm256_setzero_si256(); 16];
    schedule[0] = _mm256_set_epi64x(
        candidate_words[3][0] as i64,
        candidate_words[2][0] as i64,
        candidate_words[1][0] as i64,
        candidate_words[0][0] as i64,
    );
    schedule[1] = _mm256_set_epi64x(
        candidate_words[3][1] as i64,
        candidate_words[2][1] as i64,
        candidate_words[1][1] as i64,
        candidate_words[0][1] as i64,
    );
    schedule[2] = _mm256_set1_epi64x(0x8000000000000000u64 as i64);
    schedule[15] = _mm256_set1_epi64x(128);

    let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h) = (
        _mm256_set1_epi64x(0x6a09e667f3bcc908u64 as i64),
        _mm256_set1_epi64x(0xbb67ae8584caa73bu64 as i64),
        _mm256_set1_epi64x(0x3c6ef372fe94f82bu64 as i64),
        _mm256_set1_epi64x(0xa54ff53a5f1d36f1u64 as i64),
        _mm256_set1_epi64x(0x510e527fade682d1u64 as i64),
        _mm256_set1_epi64x(0x9b05688c2b3e6c1fu64 as i64),
        _mm256_set1_epi64x(0x1f83d9abfb41bd6bu64 as i64),
        _mm256_set1_epi64x(0x5be0cd19137e2179u64 as i64),
    );

    for (round, &constant) in SHA512_K.iter().enumerate() {
        let word = if round < 16 {
            schedule[round]
        } else {
            let x = schedule[(round + 1) & 15];
            let y = schedule[(round + 14) & 15];
            let (sigma0, sigma1) = (
                _mm256_xor_si256(
                    _mm256_xor_si256(rotr!(x, 1), rotr!(x, 8)),
                    _mm256_srli_epi64::<7>(x),
                ),
                _mm256_xor_si256(
                    _mm256_xor_si256(rotr!(y, 19), rotr!(y, 61)),
                    _mm256_srli_epi64::<6>(y),
                ),
            );
            let word = _mm256_add_epi64(
                _mm256_add_epi64(sigma1, schedule[(round + 9) & 15]),
                _mm256_add_epi64(sigma0, schedule[round & 15]),
            );
            schedule[round & 15] = word;
            word
        };

        let (sigma1, ch, constant) = (
            _mm256_xor_si256(_mm256_xor_si256(rotr!(e, 14), rotr!(e, 18)), rotr!(e, 41)),
            _mm256_xor_si256(_mm256_and_si256(e, f), _mm256_andnot_si256(e, g)),
            _mm256_set1_epi64x(constant as i64),
        );
        let temp1 = _mm256_add_epi64(
            unsafe { add3(h, constant, _mm256_add_epi64(sigma1, ch)) },
            word,
        );
        let (sigma0, majority) = (
            _mm256_xor_si256(_mm256_xor_si256(rotr!(a, 28), rotr!(a, 34)), rotr!(a, 39)),
            _mm256_xor_si256(
                _mm256_xor_si256(_mm256_and_si256(a, b), _mm256_and_si256(a, c)),
                _mm256_and_si256(b, c),
            ),
        );
        let temp2 = _mm256_add_epi64(sigma0, majority);

        h = g;
        g = f;
        f = e;
        e = _mm256_add_epi64(d, temp1);
        d = c;
        c = b;
        b = a;
        a = _mm256_add_epi64(temp1, temp2);
    }

    let digest_word = _mm256_add_epi64(a, _mm256_set1_epi64x(SHA512_IV0 as i64));
    let mut output = [0u64; 4];
    unsafe { _mm256_storeu_si256(output.as_mut_ptr().cast(), digest_word) };
    output
}

#[cfg(test)]
mod test;
