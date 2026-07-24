use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use sha2::{Digest, Sha512};
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long, default_value = "ebac62b9")]
    node_suffix: String,

    #[arg(long, default_value = "eb366895")]
    commit_last8: String,
}

const MAX_UUID_INDEX: u64 = 1u64 << 58;

#[inline(always)]
fn make_uuid_bytes(i: u64, bytes: &mut [u8; 16], commit: u32, node_suffix: u32) {
    let node_prefix = (i & 0xFFFF) as u16;
    let variant_tail = ((i >> 16) & 0xFFF) as u16;
    let variant_head = ((i >> 28) & 0x3) as u8;
    let time_high = ((i >> 30) & 0xFFF) as u16;
    let time_mid = ((i >> 42) & 0xFFFF) as u16;

    let commit_bytes = commit.to_be_bytes();
    let suffix_bytes = node_suffix.to_be_bytes();

    bytes[0] = commit_bytes[0];
    bytes[1] = commit_bytes[1];
    bytes[2] = commit_bytes[2];
    bytes[3] = commit_bytes[3];
    bytes[4] = (time_mid >> 8) as u8;
    bytes[5] = time_mid as u8;
    bytes[6] = 0x40 | ((time_high >> 8) as u8);
    bytes[7] = time_high as u8;
    bytes[8] = ((8 + variant_head) << 4) | ((variant_tail >> 8) as u8);
    bytes[9] = variant_tail as u8;
    bytes[10] = (node_prefix >> 8) as u8;
    bytes[11] = node_prefix as u8;
    bytes[12] = suffix_bytes[0];
    bytes[13] = suffix_bytes[1];
    bytes[14] = suffix_bytes[2];
    bytes[15] = suffix_bytes[3];
}

fn bytes_to_uuid_string(bytes: &[u8; 16]) -> String {
    let mut s = String::with_capacity(36);
    use std::fmt::Write;
    write!(
        &mut s,
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    ).unwrap();
    s
}

fn main() {
    let args = Args::parse();

    let commit = u32::from_str_radix(&args.commit_last8, 16).unwrap();
    let node_suffix = u32::from_str_radix(&args.node_suffix, 16).unwrap();

    println!("===== xray-qbrute =====");
    println!("Threads: {}", rayon::current_num_threads());
    println!("COMMIT_LAST8: {}", args.commit_last8);
    println!("NODE_SUFFIX: {}", args.node_suffix);

    let attempts = AtomicU64::new(0);
    let start = Instant::now();

    let pb = ProgressBar::new(MAX_UUID_INDEX);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({per_sec}) {msg}",
            )
            .unwrap(),
    );

    let result = (0u64..MAX_UUID_INDEX)
        .into_par_iter()
        .map(|i| {
            let count = attempts.fetch_add(1, Ordering::Relaxed) + 1;

            if count & 0xFFFFF == 0 {
                pb.set_position(count);
            }

            let mut buf = [0u8; 16];
            make_uuid_bytes(i, &mut buf, commit, node_suffix);
            let hash = Sha512::digest(buf);

            (buf, hash, count)
        })
        .find_any(|(_, hash, _)| {
            hash[0] == 0 && hash[1] == 0 && hash[2] == 0 && hash[3] == 0 && (hash[4] & 0x80) == 0
        });

    if let Some((bytes, hash, count)) = result {
        let elapsed = start.elapsed();
        let speed = count as f64 / elapsed.as_secs_f64() / 1e6;
        let uuid = bytes_to_uuid_string(&bytes);

        println!("\n===== FOUND =====");
        println!("UUID      : {}", uuid);
        println!("answer    : /answer {}", uuid);
        println!("hash[:10] : {}", hex::encode(&hash[..10]));
        println!("attempts  : {}", count);
        println!("time      : {:.2}s", elapsed.as_secs_f64());
        println!("rate      : {:.1} M/s", speed);

        pb.finish_with_message("Found!");
    } else {
        pb.finish_with_message("Not found in 2^58 range");
    }
}