use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

const SHADER_TEMPLATE: &str = "src/shaders/sha512_search.wgsl";
const ROUND_MARKER: &str = "    // @SHA512_UNROLLED_ROUNDS@";

const ROUND_CONSTANTS: [&str; 80] = [
    "0x428a2f98d728ae22lu",
    "0x7137449123ef65cdlu",
    "0xb5c0fbcfec4d3b2flu",
    "0xe9b5dba58189dbbclu",
    "0x3956c25bf348b538lu",
    "0x59f111f1b605d019lu",
    "0x923f82a4af194f9blu",
    "0xab1c5ed5da6d8118lu",
    "0xd807aa98a3030242lu",
    "0x12835b0145706fbelu",
    "0x243185be4ee4b28clu",
    "0x550c7dc3d5ffb4e2lu",
    "0x72be5d74f27b896flu",
    "0x80deb1fe3b1696b1lu",
    "0x9bdc06a725c71235lu",
    "0xc19bf174cf692694lu",
    "0xe49b69c19ef14ad2lu",
    "0xefbe4786384f25e3lu",
    "0x0fc19dc68b8cd5b5lu",
    "0x240ca1cc77ac9c65lu",
    "0x2de92c6f592b0275lu",
    "0x4a7484aa6ea6e483lu",
    "0x5cb0a9dcbd41fbd4lu",
    "0x76f988da831153b5lu",
    "0x983e5152ee66dfablu",
    "0xa831c66d2db43210lu",
    "0xb00327c898fb213flu",
    "0xbf597fc7beef0ee4lu",
    "0xc6e00bf33da88fc2lu",
    "0xd5a79147930aa725lu",
    "0x06ca6351e003826flu",
    "0x142929670a0e6e70lu",
    "0x27b70a8546d22ffclu",
    "0x2e1b21385c26c926lu",
    "0x4d2c6dfc5ac42aedlu",
    "0x53380d139d95b3dflu",
    "0x650a73548baf63delu",
    "0x766a0abb3c77b2a8lu",
    "0x81c2c92e47edaee6lu",
    "0x92722c851482353blu",
    "0xa2bfe8a14cf10364lu",
    "0xa81a664bbc423001lu",
    "0xc24b8b70d0f89791lu",
    "0xc76c51a30654be30lu",
    "0xd192e819d6ef5218lu",
    "0xd69906245565a910lu",
    "0xf40e35855771202alu",
    "0x106aa07032bbd1b8lu",
    "0x19a4c116b8d2d0c8lu",
    "0x1e376c085141ab53lu",
    "0x2748774cdf8eeb99lu",
    "0x34b0bcb5e19b48a8lu",
    "0x391c0cb3c5c95a63lu",
    "0x4ed8aa4ae3418acblu",
    "0x5b9cca4f7763e373lu",
    "0x682e6ff3d6b2b8a3lu",
    "0x748f82ee5defb2fclu",
    "0x78a5636f43172f60lu",
    "0x84c87814a1f0ab72lu",
    "0x8cc702081a6439eclu",
    "0x90befffa23631e28lu",
    "0xa4506cebde82bde9lu",
    "0xbef9a3f7b2c67915lu",
    "0xc67178f2e372532blu",
    "0xca273eceea26619clu",
    "0xd186b8c721c0c207lu",
    "0xeada7dd6cde0eb1elu",
    "0xf57d4f7fee6ed178lu",
    "0x06f067aa72176fbalu",
    "0x0a637dc5a2c898a6lu",
    "0x113f9804bef90daelu",
    "0x1b710b35131c471blu",
    "0x28db77f523047d84lu",
    "0x32caab7b40c72493lu",
    "0x3c9ebe0a15c9bebclu",
    "0x431d67c49c100d4clu",
    "0x4cc5d4becb3e42b6lu",
    "0x597f299cfc657e2alu",
    "0x5fcb6fab3ad6faeclu",
    "0x6c44198c4a475817lu",
];

fn main() {
    println!("cargo:rerun-if-changed={SHADER_TEMPLATE}");
    println!("cargo:rerun-if-changed=build.rs");

    let template = fs::read_to_string(SHADER_TEMPLATE).expect("read WGSL template");
    assert_eq!(
        template.matches(ROUND_MARKER).count(),
        1,
        "WGSL template must contain one SHA-512 round marker"
    );

    let generated = template.replace(ROUND_MARKER, &generate_unrolled_rounds());
    let output = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set"))
        .join("sha512_search.wgsl");
    fs::write(output, generated).expect("write generated WGSL");
}

fn generate_unrolled_rounds() -> String {
    let mut output = String::new();
    let mut state = ["a", "b", "c", "d", "e", "f", "g", "h"];

    for (round, constant) in ROUND_CONSTANTS.iter().enumerate() {
        let slot = round & 15;
        if round >= 16 {
            writeln!(
                output,
                "    w{slot} = small_sigma1(w{}) + w{} + small_sigma0(w{}) + w{slot};",
                (round + 14) & 15,
                (round + 9) & 15,
                (round + 1) & 15,
            )
            .expect("write to String");
        }

        let [a, b, c, d, e, f, g, h] = state;
        writeln!(
            output,
            "    let temp1_{round} = {h} + big_sigma1({e}) + (({e} & {f}) ^ ((~{e}) & {g})) + {constant} + w{slot};"
        )
        .expect("write to String");
        writeln!(
            output,
            "    let temp2_{round} = big_sigma0({a}) + (({a} & {b}) ^ ({a} & {c}) ^ ({b} & {c}));"
        )
        .expect("write to String");
        writeln!(output, "    {d} = {d} + temp1_{round};").expect("write to String");
        writeln!(output, "    {h} = temp1_{round} + temp2_{round};").expect("write to String");

        state = [h, a, b, c, d, e, f, g];
    }

    writeln!(output, "    return {} + 0x6a09e667f3bcc908lu;", state[0]).expect("write to String");
    output
}
