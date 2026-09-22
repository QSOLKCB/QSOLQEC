use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::time::{SystemTime, UNIX_EPOCH};

use qsolqec_memorywall::{
    capture_process_memory_baseline, probe_host, run_experiment_with_context, source_revision,
    source_revision_url, ExperimentSpec, FlyBodyIdSource, HarnessError, MemoryWallReceipt,
    RepresentationKind, SweepChildFailure, SweepReceipt, SWEEP_SCHEMA,
};

const FROZEN_FLY_BODY_IDS_ENV: &str = "QSOLQEC_FROZEN_FLY_BODY_IDS";

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
    let host = probe_host();
    let baseline = capture_process_memory_baseline();
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

    let receipt = run_experiment_with_context(&spec, host, baseline)?;
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
    let fly_args = FrozenFlyChildArgs::from_sweep_args(args)?;
    if !fly_args.args.is_empty()
        && !representations.contains(&RepresentationKind::FlyPhi664Virtualized)
    {
        return Err("Fly-specific options require fly-phi664 in --representations".into());
    }

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
            let mut command = Command::new(&executable);
            command.args(&child_args);
            if *representation == RepresentationKind::FlyPhi664Virtualized {
                command.args(&fly_args.args);
                if let Some(path) = &fly_args.frozen_body_ids_path {
                    command.env(FROZEN_FLY_BODY_IDS_ENV, path.as_os_str());
                }
            }

            let output = command.output()?;
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
            format!("unknown representation {value:?}; use dense, stabilizer, or fly-phi664")
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
    let frozen_body_ids_path = std::env::var_os(FROZEN_FLY_BODY_IDS_ENV);
    let has_fly_option = frozen_body_ids_path.is_some()
        || FLY_VALUE_FLAGS
            .iter()
            .any(|flag| args.iter().any(|argument| argument == flag));

    if spec.representation != RepresentationKind::FlyPhi664Virtualized {
        if has_fly_option {
            return Err("Fly-specific options require --representation fly-phi664".into());
        }
        return Ok(());
    }

    if frozen_body_ids_path.is_some() && optional_value_once(args, "--fly-body-ids")?.is_some() {
        return Err("frozen Fly body-ID snapshot conflicts with --fly-body-ids".into());
    }

    if let Some(path) = frozen_body_ids_path {
        spec.fly.body_ids = parse_body_id_file(std::path::Path::new(&path))?;
        spec.fly.body_id_source = FlyBodyIdSource::ExternalCanonicalList;
    } else if let Some(path) = optional_value_once(args, "--fly-body-ids")? {
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

struct FrozenFlyChildArgs {
    args: Vec<String>,
    frozen_body_ids_path: Option<PathBuf>,
}

impl FrozenFlyChildArgs {
    fn from_sweep_args(args: &[String]) -> Result<Self, Box<dyn std::error::Error>> {
        let mut output = Vec::new();
        let mut frozen_body_ids_path = None;

        for flag in FLY_VALUE_FLAGS {
            let Some(value) = optional_value_once(args, flag)? else {
                continue;
            };

            if flag == "--fly-body-ids" {
                let mut body_ids = parse_body_id_file(std::path::Path::new(value))?;
                body_ids.sort_unstable();
                frozen_body_ids_path = Some(write_frozen_body_ids(&body_ids)?);
            } else {
                output.push(flag.to_owned());
                output.push(value.to_owned());
            }
        }

        Ok(Self {
            args: output,
            frozen_body_ids_path,
        })
    }
}

impl Drop for FrozenFlyChildArgs {
    fn drop(&mut self) {
        if let Some(path) = self.frozen_body_ids_path.take() {
            let _ = fs::remove_file(path);
        }
    }
}

fn write_frozen_body_ids(body_ids: &[u64]) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();

    for attempt in 0..100u32 {
        let path = std::env::temp_dir().join(format!(
            "qsolqec-memorywall-frozen-bodyids-{}-{stamp}-{attempt}.txt",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                for body_id in body_ids {
                    writeln!(file, "{body_id}")?;
                }
                file.sync_all()?;
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }

    Err("unable to create frozen Fly body-ID snapshot".into())
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

fn optional_u32(args: &[String], flag: &str) -> Result<Option<u32>, Box<dyn std::error::Error>> {
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
    fn fly_representation_aliases_parse() {
        for value in ["fly-phi664", "fly", "virtualized"] {
            assert_eq!(
                parse_representation(value).unwrap(),
                RepresentationKind::FlyPhi664Virtualized
            );
        }
    }

    #[test]
    fn fly_options_configure_only_the_fly_representation() {
        let args = vec![
            "--fly-page-span".to_owned(),
            "64".to_owned(),
            "--fly-tile-span".to_owned(),
            "32".to_owned(),
            "--fly-scratch-domains".to_owned(),
            "2".to_owned(),
        ];
        let mut fly = ExperimentSpec::new(RepresentationKind::FlyPhi664Virtualized, 2, 3, 1);
        configure_fly_spec(&mut fly, &args).unwrap();
        assert_eq!(fly.fly.page_span, 64);
        assert_eq!(fly.fly.tile_span, 32);
        assert_eq!(fly.fly.scratch_domains, 2);

        let mut dense = ExperimentSpec::new(RepresentationKind::Dense, 2, 3, 1);
        assert!(configure_fly_spec(&mut dense, &args).is_err());
    }

    #[test]
    fn sweep_freezes_external_body_ids_before_child_runs() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let original = std::env::temp_dir().join(format!(
            "qsolqec-memorywall-source-bodyids-{}-{stamp}.txt",
            std::process::id()
        ));
        fs::write(&original, "556329\n12781\n").unwrap();

        let args = vec![
            "--fly-body-ids".to_owned(),
            original.to_string_lossy().into_owned(),
            "--fly-page-span".to_owned(),
            "64".to_owned(),
        ];
        let frozen = FrozenFlyChildArgs::from_sweep_args(&args).unwrap();
        let frozen_path = frozen.frozen_body_ids_path.as_ref().unwrap().clone();

        fs::write(&original, "12781\n556329\n999999\n").unwrap();

        assert_ne!(frozen_path, original);
        assert_eq!(
            parse_body_id_file(&frozen_path).unwrap(),
            vec![12781, 556329]
        );
        assert!(!frozen.args.iter().any(|argument| argument == "--fly-body-ids"));
        assert!(frozen
            .args
            .windows(2)
            .any(|pair| pair[0] == "--fly-page-span" && pair[1] == "64"));

        fs::remove_file(original).unwrap();
    }

    #[test]
    fn frozen_snapshot_environment_preserves_non_utf8_path_bytes() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let raw = b"/tmp/qsolqec-\xff-bodyids.txt".to_vec();
        let path = PathBuf::from(std::ffi::OsString::from_vec(raw.clone()));
        let mut command = Command::new("true");
        command.env(FROZEN_FLY_BODY_IDS_ENV, path.as_os_str());

        let (_, value) = command
            .get_envs()
            .find(|(key, _)| *key == std::ffi::OsStr::new(FROZEN_FLY_BODY_IDS_ENV))
            .unwrap();
        assert_eq!(value.unwrap().as_bytes(), raw.as_slice());
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
