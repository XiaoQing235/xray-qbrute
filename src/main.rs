use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use sha2::{Digest, Sha512};
use std::fmt::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

const COMMIT_LAST8: &str = "d0acdaaf";
const FIXED_NODE_SUFFIX: &str = "0ee876cf";
const MAX_UUID_INDEX: u64 = 1u64 << 58;

fn make_uuid(i: u64) -> String {
    let mut buf = String::with_capacity(36);

    // time_mid      16 bit
    // time_high     12 bit
    // variant_head   2 bit
    // variant_tail  12 bit
    // node_prefix   16 bit
    // total         58 bit

    let node_prefix = i & 0xFFFF;
    let variant_tail = (i >> 16) & 0xFFF;
    let variant_head = (i >> 28) & 0x3;
    let time_high = (i >> 30) & 0xFFF;
    let time_mid = (i >> 42) & 0xFFFF;

    write!(
        &mut buf,
        "{}-{:04x}-4{:03x}-{:x}{:03x}-{:04x}{}",
        COMMIT_LAST8,
        time_mid,
        time_high,
        8 + variant_head,
        variant_tail,
        node_prefix,
        FIXED_NODE_SUFFIX
    )
    .unwrap();

    buf
}

fn main() {
    println!("===== xray-qbrute =====");
    println!("Threads: {}", rayon::current_num_threads());

    let found = AtomicBool::new(false);
    let attempts = AtomicU64::new(0);
    let start = Instant::now();

    let pb = ProgressBar::new(MAX_UUID_INDEX);
    pb.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({per_sec}) {msg}")
        .unwrap());

    let result = (0u64..MAX_UUID_INDEX).into_par_iter().find_any(|&i| {
        if found.load(Ordering::Relaxed) {
            return false;
        }

        let count = attempts.fetch_add(1, Ordering::Relaxed) + 1;

        if count & 0xFFFFF == 0 {
            // 1m attempts
            pb.set_position(count);
        }

        let uuid = make_uuid(i);
        let hash = Sha512::digest(uuid.as_bytes());

        if hash[0] == 0 && hash[1] == 0 && hash[2] == 0 && hash[3] == 0 && (hash[4] & 0x80) == 0 {
            found.store(true, Ordering::Relaxed);

            let elapsed = start.elapsed();
            let speed = count as f64 / elapsed.as_secs_f64() / 1e6;

            println!("\n===== FOUND =====");
            println!("UUID      : {}", uuid);
            println!("answer    : /answer {}", uuid);
            println!("hash[:10] : {}", hex::encode(&hash[..10]));
            println!("attempts  : {}", count);
            println!("time      : {:.2}s", elapsed.as_secs_f64());
            println!("rate      : {:.1} M/s", speed);

            pb.finish_with_message("Found!");
            return true;
        }
        false
    });

    if result.is_none() {
        pb.finish_with_message("Not found in 2^58 range");
    }
}
