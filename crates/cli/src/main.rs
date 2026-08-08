use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use xray_qbrute_core::backend::{
    expected_search_seconds, hit_probability, ProgressReporter, SearchConfig, SearchError,
};
use xray_qbrute_core::candidate::{self, CandidateConfig, DEFAULT_DIFFICULTY_BITS, MAX_UUID_INDEX};
use xray_qbrute_core::search::{self, BackendKind};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(value_name = "HEX8", help = "Node suffix (last 8 hex digits of the node ID)")]
    node_suffix: String,

    #[arg(long, default_value = "eb366895", value_name = "HEX8")]
    commit_last8: String,

    #[arg(long, value_enum, default_value_t = BackendKind::Auto)]
    backend: BackendKind,

    #[arg(long, default_value_t = 0, value_name = "COUNT")]
    start: u64,

    #[arg(long, default_value_t = MAX_UUID_INDEX, value_name = "COUNT")]
    max_index: u64,

    #[arg(long)]
    no_progress: bool,
}

fn parse_hex_u32(name: &str, value: &str) -> Result<u32, String> {
    if value.len() != 8 {
        return Err(format!(
            "{name} must contain exactly 8 hexadecimal characters"
        ));
    }
    u32::from_str_radix(value, 16)
        .map_err(|_| format!("{name} contains a non-hexadecimal character"))
}

fn make_progress_bar(max_index: u64, hidden: bool) -> ProgressBar {
    if hidden {
        return ProgressBar::hidden();
    }

    let progress = ProgressBar::new(max_index);
    progress.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({per_sec}) {msg}",
            )
            .expect("progress template is static and valid"),
    );
    progress
}

fn format_rate(rate_per_second: f64) -> String {
    const UNITS: [&str; 5] = ["H/s", "kH/s", "MH/s", "GH/s", "TH/s"];
    let mut value = rate_per_second;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

fn run(args: Args) -> Result<(), SearchError> {
    let commit =
        parse_hex_u32("--commit-last8", &args.commit_last8).map_err(SearchError::InvalidConfig)?;
    let node_suffix =
        parse_hex_u32("node-suffix", &args.node_suffix).map_err(SearchError::InvalidConfig)?;

    let candidate = CandidateConfig {
        commit,
        node_suffix,
    };
    let config = SearchConfig {
        candidate,
        start_index: args.start,
        max_index: args.max_index,
        leading_zero_bits: DEFAULT_DIFFICULTY_BITS,
    };

    println!("===== xray-qbrute =====");
    println!("Threads: {}", rayon::current_num_threads());
    println!("COMMIT_LAST8: {}", args.commit_last8);
    println!("NODE_SUFFIX: {}", args.node_suffix);
    println!("Backend request: {}", args.backend);
    println!(
        "Range: [{:#x}, {:#x})",
        config.start_index, config.max_index
    );

    let processed_total = Arc::new(AtomicU64::new(0));

    let prepared = search::prepare(&config, args.backend)?;
    let progress_bar = make_progress_bar(config.max_index - config.start_index, args.no_progress);
    let callback_bar = progress_bar.clone();
    let callback_total = Arc::clone(&processed_total);
    let progress: ProgressReporter = Arc::new(move |count| {
        callback_total.fetch_add(count, Ordering::Relaxed);
        callback_bar.inc(count);
    });

    let interrupt_total = Arc::clone(&processed_total);
    let interrupt_start = config.start_index;
    let interrupt_bar = progress_bar.clone();
    ctrlc::set_handler(move || {
        let next = (interrupt_start + interrupt_total.load(Ordering::Relaxed))
            & (MAX_UUID_INDEX - 1);
        interrupt_bar.suspend(|| {
            eprintln!(
                "\nInterrupted after {} candidates.\nResume from: --start {next}  (0x{next:x})",
                interrupt_total.load(Ordering::Relaxed)
            );
        });
        std::process::exit(130);
    })
    .map_err(|error| SearchError::Runtime(format!("failed to install Ctrl+C handler: {error}")))?;

    let start = Instant::now();
    let outcome = prepared.search(&config, &progress)?;
    let wall_elapsed = start.elapsed();
    let elapsed = outcome.search_elapsed.unwrap_or(wall_elapsed);

    for reason in &outcome.fallback_reasons {
        eprintln!("backend fallback: {reason}");
    }

    if let Some(device) = &outcome.device_name {
        println!("Device: {device}");
    }
    println!("Backend: {}", outcome.backend_name);
    if let Some(kernel_config) = &outcome.kernel_config {
        println!("Kernel: {kernel_config}");
    }
    if let Some(tuning_elapsed) = outcome.tuning_elapsed {
        println!("GPU tuning: {:.2}s", tuning_elapsed.as_secs_f64());
    }
    let speed = outcome.evaluated as f64 / elapsed.as_secs_f64().max(f64::EPSILON);

    let probability =
        hit_probability(outcome.evaluated, config.leading_zero_bits);
    let expected_seconds =
        expected_search_seconds(config.leading_zero_bits, speed);

    if let Some(hit) = outcome.hit {
        let bytes = candidate::candidate_bytes(hit.index, config.candidate);
        let hash = candidate::digest(hit.index, config.candidate);
        let first_word = candidate::digest_word(&hash);
        if !candidate::matches_leading_zero_bits(first_word, config.leading_zero_bits) {
            return Err(SearchError::Runtime(format!(
                "backend returned an invalid hit at index {}",
                hit.index
            )));
        }

        let uuid = candidate::bytes_to_uuid_string(&bytes);
        println!("\n===== FOUND =====");
        println!("UUID      : {uuid}");
        println!("answer    : /answer {uuid}");
        println!("hash[:10] : {}", hex::encode(&hash[..10]));
        println!("processed : {} candidates", outcome.processed);
        println!("evaluated : {} candidates", outcome.evaluated);
        println!("time      : {:.2}s", elapsed.as_secs_f64());
        println!("rate      : {}", format_rate(speed));
        println!(
            "P(hit in range)   : {:.4}%",
            probability * 100.0
        );
        println!(
            "expected (1 hit)  : ~{:.1}s at current rate ({})",
            expected_seconds,
            format_rate(speed)
        );
        progress_bar.abandon_with_message("Found!");
    } else {
        println!("\n===== NOT FOUND =====");
        println!("searched  : {} candidates", outcome.processed);
        println!("evaluated : {} candidates", outcome.evaluated);
        println!("time      : {:.2}s", elapsed.as_secs_f64());
        println!("rate      : {}", format_rate(speed));
        println!(
            "P(hit in range)   : {:.4}%",
            probability * 100.0
        );
        println!(
            "expected to hit   : ~{:.1}s at current rate ({})",
            expected_seconds,
            format_rate(speed)
        );
        let next = (config.start_index + outcome.evaluated) & (MAX_UUID_INDEX - 1);
        println!("resume            : --start {next}  (0x{next:x})");
        progress_bar.finish_with_message("Not found");
    }

    Ok(())
}

fn main() {
    let args = Args::parse();
    if let Err(error) = run(args) {
        eprintln!("error: {error}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod test {
    use super::format_rate;

    #[test]
    fn rate_below_1000_stays_in_hash_per_second() {
        assert_eq!(format_rate(0.0), "0.0 H/s");
        assert_eq!(format_rate(999.9), "999.9 H/s");
    }

    #[test]
    fn rate_scales_through_si_units() {
        assert_eq!(format_rate(1000.0), "1.0 kH/s");
        assert_eq!(format_rate(1_000_000.0), "1.0 MH/s");
        assert_eq!(format_rate(1_000_000_000.0), "1.0 GH/s");
        assert_eq!(format_rate(1_000_000_000_000.0), "1.0 TH/s");
    }

    #[test]
    fn rate_clamps_to_terahash_at_overflow() {
        assert_eq!(format_rate(1e15), "1000.0 TH/s");
    }
}
