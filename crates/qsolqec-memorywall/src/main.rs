use std::fs;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use qsolqec_memorywall::{
    probe_host, run_experiment, source_revision, source_revision_url, ExperimentSpec, HarnessError,
    MemoryWallReceipt, RepresentationKind, SweepChildFailure, SweepReceipt, SWEEP_SCHEMA,
};

fn main() -> ExitCode {
    match real_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("qsolqec-memorywall: {error}");
            ExitCode::FAILURE
        }
    }
}

fn real_main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        print_usage();
        return Ok(());
    };

    match command {
        "probe" => {
            let output = option_value(&args[1..], "--output").map(PathBuf::from);
            emit_json(&probe_host(), output.as_deref())?;
        }
        "run" => run_command(&args[1..])?,
        "sweep" => sweep_command(&args[1..])?,
        "-h" | "--help" | "help" => print_usage(),
        other => return Err(format!("unknown command {other:?}").into()),
    }

    Ok(())
}

fn run_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let representation = parse_representation(required(args, "--representation")?)?;
    let dimension = parse_usize(required(args, "--dimension")?, "--dimension")?;
    let subsystems = parse_usize(required(args, "--subsystems")?, "--subsystems")?;
    let rounds = parse_usize(required(args, "--rounds")?, "--rounds")?;

    let mut spec = ExperimentSpec::new(representation, dimension, subsystems, rounds);
    spec.max_logical_bytes = optional_mib(args, "--max-logical-mib")?;
    if let Some(bytes) = optional_mib(args, "--oracle-limit-mib")? {
        spec.oracle_logical_limit_bytes = bytes;
    }

    let receipt = run_experiment(&spec)?;
    let output = option_value(args, "--output").map(PathBuf::from);
    emit_json(&receipt, output.as_deref())?;
    Ok(())
}

fn sweep_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let representations = required(args, "--representations")?
        .split(',')
        .map(parse_representation)
        .collect::<Result<Vec<_>, _>>()?;

    let dimension = parse_usize(required(args, "--dimension")?, "--dimension")?;
    let start_n = parse_usize(required(args, "--start-n")?, "--start-n")?;
    let end_n = parse_usize(required(args, "--end-n")?, "--end-n")?;
    let step = parse_usize(required(args, "--step")?, "--step")?;
    let rounds = parse_usize(required(args, "--rounds")?, "--rounds")?;
    if step == 0 {
        return Err("--step must be at least 1".into());
    }
    if start_n > end_n {
        return Err("--start-n must not exceed --end-n".into());
    }

    let max_logical_mib =
        optional_mib(args, "--max-logical-mib")?.map(|bytes| (bytes / (1024 * 1024)).to_string());
    let oracle_limit_mib =
        optional_mib(args, "--oracle-limit-mib")?.map(|bytes| (bytes / (1024 * 1024)).to_string());
    let executable = std::env::current_exe()?;

    let mut points = Vec::new();
    let mut child_failures = Vec::new();

    for representation in &representations {
        let mut n = start_n;
        while n <= end_n {
            let mut child_args = vec![
                "run".to_owned(),
                "--representation".to_owned(),
                representation.to_string(),
                "--dimension".to_owned(),
                dimension.to_string(),
                "--subsystems".to_owned(),
                n.to_string(),
                "--rounds".to_owned(),
                rounds.to_string(),
            ];

            if let Some(value) = &max_logical_mib {
                child_args.push("--max-logical-mib".into());
                child_args.push(value.clone());
            }
            if let Some(value) = &oracle_limit_mib {
                child_args.push("--oracle-limit-mib".into());
                child_args.push(value.clone());
            }

            let output = Command::new(&executable).args(&child_args).output()?;
            if !output.status.success() {
                child_failures.push(child_failure_record(
                    *representation,
                    dimension,
                    n,
                    rounds,
                    &output,
                ));
                break;
            }

            let receipt: MemoryWallReceipt = serde_json::from_slice(&output.stdout)
                .map_err(|error| HarnessError::Serialization(error.to_string()))?;
            let stop = !receipt.body.outcome.is_success();
            points.push(receipt);

            if stop {
                break;
            }

            let Some(next) = n.checked_add(step) else {
                break;
            };
            n = next;
        }
    }

    let sweep = SweepReceipt {
        schema: SWEEP_SCHEMA.into(),
        source_revision: source_revision(),
        source_revision_url: source_revision_url(),
        dimension,
        start_n,
        end_n,
        step,
        rounds,
        representations,
        points,
        child_failures,
    };

    let output = option_value(args, "--output").map(PathBuf::from);
    emit_json(&sweep, output.as_deref())?;
    Ok(())
}

fn child_failure_record(
    representation: RepresentationKind,
    dimension: usize,
    subsystems: usize,
    rounds: usize,
    output: &std::process::Output,
) -> SweepChildFailure {
    SweepChildFailure {
        representation,
        dimension,
        subsystems,
        rounds,
        exit_code: output.status.code(),
        signal: exit_signal(&output.status),
        stderr: String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(4096)
            .collect(),
    }
}

fn exit_signal(status: &std::process::ExitStatus) -> Option<i32> {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status.signal()
    }

    #[cfg(not(unix))]
    {
        let _ = status;
        None
    }
}

fn required<'a>(args: &'a [String], flag: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    option_value(args, flag).ok_or_else(|| format!("missing required option {flag}").into())
}

fn option_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

fn parse_usize(value: &str, flag: &str) -> Result<usize, Box<dyn std::error::Error>> {
    value
        .parse()
        .map_err(|_| format!("{flag} expects a non-negative integer, got {value:?}").into())
}

fn parse_representation(value: &str) -> Result<RepresentationKind, Box<dyn std::error::Error>> {
    match value {
        "dense" => Ok(RepresentationKind::Dense),
        "stabilizer" | "prime-stabilizer" => Ok(RepresentationKind::PrimeStabilizer),
        _ => Err(format!("unknown representation {value:?}; use dense or stabilizer").into()),
    }
}

fn optional_mib(args: &[String], flag: &str) -> Result<Option<u64>, Box<dyn std::error::Error>> {
    let positions: Vec<usize> = args
        .iter()
        .enumerate()
        .filter_map(|(index, argument)| (argument == flag).then_some(index))
        .collect();

    if positions.len() > 1 {
        return Err(format!("{flag} may be supplied at most once").into());
    }

    let Some(index) = positions.first().copied() else {
        return Ok(None);
    };

    let value = args
        .get(index + 1)
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| format!("{flag} requires an integer MiB value"))?;

    let mib: u64 = value
        .parse()
        .map_err(|_| format!("{flag} expects an integer MiB value, got {value:?}"))?;

    mib.checked_mul(1024 * 1024)
        .map(Some)
        .ok_or_else(|| format!("{flag} byte conversion overflow").into())
}

fn emit_json<T: serde::Serialize>(
    value: &T,
    output: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(value)?;
    if let Some(path) = output {
        fs::write(path, format!("{json}\n"))?;
    }
    println!("{json}");
    Ok(())
}

fn print_usage() {
    println!(
        "QSOLQEC memory-wall runtime

USAGE:
  qsolqec-memorywall probe [--output FILE]

  qsolqec-memorywall run \\
    --representation dense|stabilizer \\
    --dimension D --subsystems N --rounds R \\
    [--max-logical-mib MIB] [--oracle-limit-mib MIB] [--output FILE]

  qsolqec-memorywall sweep \\
    --representations dense,stabilizer \\
    --dimension D --start-n N --end-n N --step S --rounds R \\
    [--max-logical-mib MIB] [--oracle-limit-mib MIB] [--output FILE]

Each sweep point executes as a fresh child process so per-point Linux VmHWM
measurements are not contaminated by earlier representations."
    );
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    #[test]
    fn child_failure_preserves_requested_point_and_signal() {
        let output = std::process::Output {
            status: std::process::ExitStatus::from_raw(9),
            stdout: Vec::new(),
            stderr: b"killed".to_vec(),
        };

        let failure = child_failure_record(RepresentationKind::Dense, 2, 24, 8, &output);

        assert_eq!(failure.representation, RepresentationKind::Dense);
        assert_eq!(failure.dimension, 2);
        assert_eq!(failure.subsystems, 24);
        assert_eq!(failure.rounds, 8);
        assert_eq!(failure.exit_code, None);
        assert_eq!(failure.signal, Some(9));
        assert_eq!(failure.stderr, "killed");
    }
}
