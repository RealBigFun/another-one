use std::process::ExitCode;

const DEFAULT_TARGET_BYTES: usize = 50_000_000;

fn main() -> ExitCode {
    let target_bytes = std::env::args()
        .nth(1)
        .map(|value| value.parse::<usize>())
        .transpose()
        .unwrap_or_else(|error| {
            eprintln!("invalid byte target: {error}");
            None
        })
        .unwrap_or(DEFAULT_TARGET_BYTES);

    let report = slint_poc::run_terminal_render_probe(target_bytes);
    for line in report.to_lines() {
        println!("{line}");
    }

    if report.applied_bytes >= report.target_bytes {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
