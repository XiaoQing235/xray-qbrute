use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

mod sha512_constants {
    include!("src/sha512_constants.rs");
}

const NATIVE_TEMPLATE: &str = "src/shaders/sha512_search.wgsl";
const WEB_TEMPLATE: &str = "src/shaders/sha512_web.wgsl";
const CUDA_TEMPLATE: &str = "src/shaders/sha512_search.cu";
const ROUND_MARKER: &str = "    // @SHA512_UNROLLED_ROUNDS@";

fn main() {
    for path in [NATIVE_TEMPLATE, WEB_TEMPLATE, CUDA_TEMPLATE, "src/sha512_constants.rs"] {
        println!("cargo:rerun-if-changed={path}");
    }
    println!("cargo:rerun-if-changed=build.rs");

    let output_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set"));
    generate_shader(
        NATIVE_TEMPLATE,
        output_dir.join("sha512_search.wgsl"),
        native_rounds(),
    );
    generate_shader(
        WEB_TEMPLATE,
        output_dir.join("sha512_web.wgsl"),
        web_rounds(),
    );
    generate_shader(
        CUDA_TEMPLATE,
        output_dir.join("sha512_search.cu"),
        cuda_rounds(),
    );
}

fn generate_shader(template_path: &str, output_path: PathBuf, rounds: String) {
    let template = fs::read_to_string(template_path).expect("read shader template");
    assert_eq!(template.matches(ROUND_MARKER).count(), 1);
    fs::write(output_path, template.replace(ROUND_MARKER, &rounds)).expect("write generated shader");
}

fn native_rounds() -> String {
    let mut output = String::new();
    let mut state = ["a", "b", "c", "d", "e", "f", "g", "h"];

    for (round, constant) in sha512_constants::ROUND_CONSTANTS.iter().enumerate() {
        let slot = round & 15;
        if round >= 16 {
            writeln!(
                output,
                "    w{slot} = small_sigma1(w{}) + w{} + small_sigma0(w{}) + w{slot};",
                (round + 14) & 15,
                (round + 9) & 15,
                (round + 1) & 15
            )
            .expect("write to String");
        }
        let [a, b, c, d, e, f, g, h] = state;
        writeln!(output, "    let temp1_{round} = {h} + big_sigma1({e}) + (({e} & {f}) ^ ((~{e}) & {g})) + 0x{constant:016x}lu + w{slot};").expect("write to String");
        writeln!(
            output,
            "    let temp2_{round} = big_sigma0({a}) + (({a} & {b}) ^ ({a} & {c}) ^ ({b} & {c}));"
        )
        .expect("write to String");
        writeln!(output, "    {d} = {d} + temp1_{round};").expect("write to String");
        writeln!(output, "    {h} = temp1_{round} + temp2_{round};").expect("write to String");
        state = [h, a, b, c, d, e, f, g];
    }

    writeln!(
        output,
        "    return {} + 0x{:016x}lu;",
        state[0],
        sha512_constants::IV[0]
    )
    .expect("write to String");
    output
}

fn web_rounds() -> String {
    let mut output = String::new();
    let mut state = ["a", "b", "c", "d", "e", "f", "g", "h"];

    for (round, constant) in sha512_constants::ROUND_CONSTANTS.iter().enumerate() {
        let slot = round & 15;
        if round >= 16 {
            writeln!(output, "    w{slot} = add64(add64(small_sigma1(w{}), w{}), add64(small_sigma0(w{}), w{slot}));", (round + 14) & 15, (round + 9) & 15, (round + 1) & 15).expect("write to String");
        }
        let [a, b, c, d, e, f, g, h] = state;
        let high = constant >> 32;
        let low = constant & 0xffff_ffff;
        writeln!(output, "    let temp1_{round} = add64(add64(add64({h}, big_sigma1({e})), choice({e}, {f}, {g})), add64(vec2<u32>(0x{high:08x}u, 0x{low:08x}u), w{slot}));").expect("write to String");
        writeln!(
            output,
            "    let temp2_{round} = add64(big_sigma0({a}), majority({a}, {b}, {c}));"
        )
        .expect("write to String");
        writeln!(output, "    {d} = add64({d}, temp1_{round});").expect("write to String");
        writeln!(output, "    {h} = add64(temp1_{round}, temp2_{round});")
            .expect("write to String");
        state = [h, a, b, c, d, e, f, g];
    }

    let high = sha512_constants::IV[0] >> 32;
    let low = sha512_constants::IV[0] & 0xffff_ffff;
    writeln!(
        output,
        "    return add64({}, vec2<u32>(0x{high:08x}u, 0x{low:08x}u));",
        state[0]
    )
    .expect("write to String");
    output
}

fn cuda_rounds() -> String {
    let mut output = String::new();
    let mut state = ["a", "b", "c", "d", "e", "f", "g", "h"];

    for (round, constant) in sha512_constants::ROUND_CONSTANTS.iter().enumerate() {
        let slot = round & 15;
        if round >= 16 {
            writeln!(
                output,
                "    w{slot} = small_sigma1(w{}) + w{} + small_sigma0(w{}) + w{slot};",
                (round + 14) & 15,
                (round + 9) & 15,
                (round + 1) & 15
            )
            .expect("write to String");
        }
        let [a, b, c, d, e, f, g, h] = state;
        writeln!(output, "    unsigned long long temp1_{round} = {h} + big_sigma1({e}) + (({e} & {f}) ^ ((~{e}) & {g})) + 0x{constant:016x}ull + w{slot};").expect("write to String");
        writeln!(
            output,
            "    unsigned long long temp2_{round} = big_sigma0({a}) + (({a} & {b}) ^ ({a} & {c}) ^ ({b} & {c}));"
        )
        .expect("write to String");
        writeln!(output, "    {d} = {d} + temp1_{round};").expect("write to String");
        writeln!(output, "    {h} = temp1_{round} + temp2_{round};").expect("write to String");
        state = [h, a, b, c, d, e, f, g];
    }

    writeln!(
        output,
        "    return {} + 0x{:016x}ull;",
        state[0],
        sha512_constants::IV[0]
    )
    .expect("write to String");
    output
}
