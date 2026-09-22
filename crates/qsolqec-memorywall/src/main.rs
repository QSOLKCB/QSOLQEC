use std::fs;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use qsolqec_memorywall::{
    probe_host, run_experiment, source_revision, source_revision_url, ExperimentSpec, HarnessError,
    FlyBodyIdSource, MemoryWallReceipt, RepresentationKind, SweepChildFailure, SweepReceipt,
    SWEEP_SCHEMA,
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
    configure_fly_spec(&mut spec, args)?;

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
    let fly_args = normalized_fly_child_args(args)?;

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
            child_args.extend(fly_args.iter().cloned());

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
        "fly-phi664" | "fly" | "virtualized" => Ok(RepresentationKind::FlyPhi664Virtualized),
        _ => Err(
            format!(
                "unknown representation {value:?}; use dense, stabilizer, or fly-phi664"
            )
            .into(),
        ),
    }
}

const FLY_VALUE_FLAGS: [&str; 9] = [
    "--fly-body-ids",
    "--fly-page-span",
    "--fly-tile-span",
    "--fly-sparse-max",
    "--fly-bitmap-max",
    "--fly-scratch-domains",
    "--fly-owner-count",
    "--fly-cache-states",
    "--fly-max-in-flight",
];

fn configure_fly_spec(
    spec: &mut ExperimentSpec,
    args: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let has_fly_option = FLY_VALUE_FLAGS
        .iter()
        .any(|flag| args.iter().any(|argument| argument == flag));

    if spec.representation != RepresentationKind::FlyPhi664Virtualized {
        if has_fly_option {
            return Err("Fly-specific options require --representation fly-phi664".into());
        }
        return Ok(());
    }

    if let Some(path) = optional_value_once(args, "--fly-body-ids")? {
        spec.fly.body_ids = parse_body_id_file(std::path::Path::new(path))?;
        spec.fly.body_id_source = FlyBodyIdSource::ExternalCanonicalList;
    }
    if let Some(value) = optional_usize(args, "--fly-page-span")? {
        spec.fly.page_span = value;
    }
    if let Some(value) = optional_usize(args, "--fly-tile-span")? {
        spec.fly.tile_span = value;
    }
    if let Some(value) = optional_usize(args, "--fly-sparse-max")? {
        spec.fly.sparse_max_occupancy = value;
    }
    if let Some(value) = optional_usize(args, "--fly-bitmap-max")? {
        spec.fly.bitmap_max_occupancy = value;
    }
    if let Some(value) = optional_usize(args, "--fly-scratch-domains")? {
        spec.fly.scratch_domains = value;
    }
    if let Some(value) = optional_u32(args, "--fly-owner-count")? {
        spec.fly.owner_count = value;
    }
    if let Some(value) = optional_usize(args, "--fly-cache-states")? {
        spec.fly.max_cached_states = value;
    }
    if let Some(value) = optional_usize(args, "--fly-max-in-flight")? {
        spec.fly.max_in_flight_generations = value;
    }
    Ok(())
}

fn normalized_fly_child_args(
    args: &[String],
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut output = Vec::new();
    for flag in FLY_VALUE_FLAGS {
        if let Some(value) = optional_value_once(args, flag)? {
            output.push(flag.to_owned());
            output.push(value.to_owned());
        }
    }
    Ok(output)
}

fn optional_value_once<'a>(
    args: &'a [String],
    flag: &str,
) -> Result<Option<&'a str>, Box<dyn std::error::Error>> {
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
        .ok_or_else(|| format!("{flag} requires a value"))?;
    Ok(Some(value))
}

fn optional_usize(
    args: &[String],
    flag: &str,
) -> Result<Option<usize>, Box<dyn std::error::Error>> {
    optional_value_once(args, flag)?
        .map(|value| parse_usize(value, flag))
        .transpose()
}

fn optional_u32(
    args: &[String],
    flag: &str,
) -> Result<Option<u32>, Box<dyn std::error::Error>> {
    optional_value_once(args, flag)?
        .map(|value| {
            value
                .parse()
                .map_err(|_| format!("{flag} expects a non-negative u32, got {value:?}").into())
        })
        .transpose()
}

fn parse_body_id_file(path: &std::path::Path) -> Result<Vec<u64>, Box<dyn std::error::Error>> {
    let text = fs::read_to_string(path)?;
    let mut body_ids = Vec::new();
    for (line_number, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let body_id = trimmed.parse::<u64>().map_err(|_| {
            format!(
                "{}:{} is not a valid unsigned bodyId: {trimmed:?}",
                path.display(),
                line_number + 1
            )
        })?;
        body_ids.push(body_id);
    }
    if body_ids.is_empty() {
        return Err(format!("{} contains no bodyId values", path.display()).into());
    }
    Ok(body_ids)
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
    --representation dense|stabilizer|fly-phi664 \\
    --dimension D --subsystems N --rounds R \\
    [--max-logical-mib MIB] [--oracle-limit-mib MIB] [--output FILE]
    [--fly-body-ids FILE]
    [--fly-page-span N] [--fly-tile-span N]
    [--fly-sparse-max N] [--fly-bitmap-max N]
    [--fly-scratch-domains N] [--fly-owner-count N]
    [--fly-cache-states N] [--fly-max-in-flight N]

  qsolqec-memorywall sweep \\
    --representations dense,stabilizer,fly-phi664 \\
    --dimension D --start-n N --end-n N --step S --rounds R \\
    [--max-logical-mib MIB] [--oracle-limit-mib MIB] [--output FILE]
    [--fly-body-ids FILE]
    [--fly-page-span N] [--fly-tile-span N]
    [--fly-sparse-max N] [--fly-bitmap-max N]
    [--fly-scratch-domains N] [--fly-owner-count N]
    [--fly-cache-states N] [--fly-max-in-flight N]

Each sweep point executes as a fresh child process so per-point Linux VmHWM
measurements are not contaminated by earlier representations."
    );
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    #[test]
    fn optional_memory_flags_require_values() {
        for flag in ["--max-logical-mib", "--oracle-limit-mib"] {
            let args = vec![flag.to_owned()];
            let error = optional_mib(&args, flag).unwrap_err().to_string();
            assert!(error.contains("requires an integer MiB value"));
        }
    }

    #[test]
    fn optional_memory_flags_reject_another_flag_as_value() {
        let args = vec![
            "--max-logical-mib".to_owned(),
            "--oracle-limit-mib".to_owned(),
            "8".to_owned(),
        ];
        let error = optional_mib(&args, "--max-logical-mib")
            .unwrap_err()
            .to_string();
        assert!(error.contains("requires an integer MiB value"));
    }

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
