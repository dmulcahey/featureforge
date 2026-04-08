use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchConfig {
    pub benchmark: &'static str,
    pub iterations: u32,
    pub warmup_iterations: u32,
    pub output_path: Option<PathBuf>,
    pub run_requested: bool,
}

pub fn parse_args(benchmark: &'static str) -> BenchConfig {
    parse_args_from(benchmark, std::env::args_os().skip(1))
}

pub fn parse_args_from<I, S>(benchmark: &'static str, args: I) -> BenchConfig
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut iterations = 50_u32;
    let mut warmup_iterations = 5_u32;
    let mut output_path = None;
    let mut run_requested = false;

    let mut args = args
        .into_iter()
        .map(|arg| os_string_to_string(arg.into()))
        .collect::<Vec<_>>()
        .into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--run-benchmark" => {
                run_requested = true;
            }
            "--iterations" => {
                let value = args
                    .next()
                    .unwrap_or_else(|| panic!("--iterations requires a numeric value"));
                iterations = value
                    .parse::<u32>()
                    .unwrap_or_else(|_| panic!("invalid --iterations value: {value}"));
            }
            "--warmup" => {
                let value = args
                    .next()
                    .unwrap_or_else(|| panic!("--warmup requires a numeric value"));
                warmup_iterations = value
                    .parse::<u32>()
                    .unwrap_or_else(|_| panic!("invalid --warmup value: {value}"));
            }
            "--output" => {
                let value = args
                    .next()
                    .unwrap_or_else(|| panic!("--output requires a file path"));
                output_path = Some(PathBuf::from(value));
            }
            _ => {}
        }
    }

    BenchConfig {
        benchmark,
        iterations,
        warmup_iterations,
        output_path,
        run_requested,
    }
}

pub fn render_run_gate_message(config: &BenchConfig) -> Option<String> {
    (!config.run_requested).then(|| {
        format!(
            "Skipping {} benchmark; pass --run-benchmark to execute.",
            config.benchmark
        )
    })
}

fn os_string_to_string(value: OsString) -> String {
    value
        .into_string()
        .unwrap_or_else(|value| value.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::{BenchConfig, parse_args_from, render_run_gate_message};
    use std::path::PathBuf;

    #[test]
    fn benchmark_args_require_explicit_run_flag() {
        assert_eq!(
            parse_args_from("workflow_status", std::iter::empty::<&str>()),
            BenchConfig {
                benchmark: "workflow_status",
                iterations: 50,
                warmup_iterations: 5,
                output_path: None,
                run_requested: false,
            }
        );
    }

    #[test]
    fn benchmark_args_preserve_explicit_run_configuration() {
        assert_eq!(
            parse_args_from(
                "execution_status",
                [
                    "--run-benchmark",
                    "--iterations",
                    "7",
                    "--warmup",
                    "2",
                    "--output",
                    "tmp/report.json",
                ]
            ),
            BenchConfig {
                benchmark: "execution_status",
                iterations: 7,
                warmup_iterations: 2,
                output_path: Some(PathBuf::from("tmp/report.json")),
                run_requested: true,
            }
        );
    }

    #[test]
    fn benchmark_run_gate_message_requires_run_flag() {
        let config = parse_args_from("workflow_status", std::iter::empty::<&str>());
        assert_eq!(
            render_run_gate_message(&config),
            Some(String::from(
                "Skipping workflow_status benchmark; pass --run-benchmark to execute.",
            ))
        );
    }

    #[test]
    fn benchmark_run_gate_message_is_absent_when_run_requested() {
        let config = parse_args_from("workflow_status", ["--run-benchmark"]);
        assert_eq!(render_run_gate_message(&config), None);
    }
}
