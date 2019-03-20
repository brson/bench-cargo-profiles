//! B-C-P! The Rust compile-time / run-time performance analyzer.
//!
//! ## TODO
//!
//! - better project name
//! - better name than 'state'
//! - better name than 'item' (cfg?)
//! - better error reporting
//! - use logging instead of println

#![allow(warnings)] // TODO tmp

#[macro_use]
extern crate structopt;
#[macro_use]
extern crate serde_derive;

use failure::{ResultExt, Error, err_msg};
use std::fs;
use std::io;
use std::path::{PathBuf, Path};
use std::process::Command;
use std::result::Result as StdResult;
use std::time::{Duration, Instant};
use structopt::StructOpt;

// A shorthand for all the `Result` types returned in this crate
type Result<T> = StdResult<T, Error>;

/// Options to the CLI
#[derive(StructOpt)]
struct Options {
    /// The path to the Cargo.toml file to explore
    #[structopt(long = "manifest-path", default_value = "Cargo.toml")]
    manifest_path: PathBuf,
    /// The directory in which we store the experiment data and HTML report
    #[structopt(long = "data-dir", default_value = "./", parse(from_os_str))]
    data_dir: PathBuf,
}

impl Options {
    /// The path to the experiment snapshot
    fn state_path(&self) -> PathBuf {
        self.data_dir.join("bcp-state.json")
    }

    /// The path to the HTML report
    fn report_path(&self) -> PathBuf {
        self.data_dir.join("bcp-report.html")
    }
}

fn main() -> Result<()> {
    let opts = Options::from_args();

    let mut state = load_state(&opts.state_path())?;

    run_experiments(&opts, &mut state)?;

    let report = gen_report(&opts, &mut state)?;

    log_report(&report)?;
    render_html(&report)?;

    Ok(())
}

static CONFIG_ITEMS: &[StaticConfigItem] = &[
    StaticConfigItem {
        path: "profile.release.opt-level",
        env_var: "CARGO_PROFILE_RELEASSE_OPT_LEVEL",
        values: &["0", "1", "2", "3"],
        default: "3",
    },
    StaticConfigItem {
        path: "profile.release.debug",
        env_var: "CARGO_PROFILE_RELEASE_DEBUG",
        values: &["false", "1", "true"],
        default: "false",
    },
    StaticConfigItem {
        path: "profile.release.rpath",
        env_var: "CARGO_PROFILE_RELEASE_RPATH",
        values: &[],
        default: "false",
    },
    StaticConfigItem {
        path: "profile.release.lto",
        env_var: "CARGO_PROFILE_RELEASE_LTO",
        values: &["false", "thin", "true"],
        default: "true",
    },
    StaticConfigItem {
        path: "profile.release.debug-assertions",
        env_var: "CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS",
        values: &["false", "true"],
        default: "false",
    },
    StaticConfigItem {
        path: "profile.release.codegen-units",
        env_var: "CARGO_PROFILE_RELEASE_CODEGEN_UNITS",
        values: &["1", "4", "16"],
        default: "1",
    },
    StaticConfigItem {
        path: "profile.release.panic",
        env_var: "CARGO_PROFILE_RELEASE_PANIC",
        values: &[],
        default: "unwind",
    },
    StaticConfigItem {
        path: "profile.release.incremental",
        env_var: "CARGO_PROFILE_RELEASE_INCREMENTAL",
        values: &["false", "true"],
        default: "false",
    },
    StaticConfigItem {
        path: "profile.release.overflow-checks",
        env_var: "CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS",
        values: &["false", "true"],
        default: "false",
    },
];

struct StaticConfigItem {
    path: &'static str,
    env_var: &'static str,
    values: &'static [&'static str],
    default: &'static str,
}


#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Debug)]
struct ConfigItem {
    path: String,
    env_var: String,
    values: Vec<String>,
    default: String,
}

type DefinedItem = (ConfigItem, String);

impl<'a> From<&'a StaticConfigItem> for ConfigItem {
    fn from(other: &'a StaticConfigItem) -> ConfigItem {
        ConfigItem {
            path: other.path.into(),
            env_var: other.env_var.into(),
            values: other.values.iter().cloned().map(|s| s.into()).collect(),
            default: other.default.into(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct State {
    plan: Plan,
    results: Vec<ExpResult>,
}

#[derive(Serialize, Deserialize)]
struct Plan {
    baseline: Vec<DefinedItem>,
    cases: Vec<Experiment>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Debug)]
struct Experiment {
    configs: Vec<DefinedItem>,
}

#[derive(Serialize, Deserialize, Clone)]
struct ExpResult {
    build_time: Duration,
    run_time: Duration,
}

type ExpAndResult = (Experiment, ExpResult);

struct Report {
    results_by_total_time: Vec<ExpAndResult>,
}

impl State {
    fn validate(&self) {
        assert!(self.plan.cases.len() >= self.results.len());
    }
}

impl Experiment {
    fn display(&self) -> String {
        if self.configs.is_empty() {
            return "(baseline)".to_string();
        }
        
        let mut buf = String::new();

        for &(ref cfg, ref val) in &self.configs {
            buf.push_str(&format!("{}={},", cfg.path, val));
        }

        buf   
    }
}

fn load_state(path: &Path) -> Result<State> {
    let state_str = fs::read_to_string(path);

    match state_str {
        Err(ref e) if e.kind() == io::ErrorKind::NotFound => {
            println!("beginning fresh run");
            Ok(new_state())
        }
        Ok(s) => {
            println!("resuming from state file {}", path.display());
            parse_state(&s)
        },
        Err(e) => {
            Err(e)
                .context("error loading from state file")
                .map_err(Error::from)
        }
    }
}

fn save_state(path: &Path, state: &State) -> Result<()> {
    let s = serde_json::to_string_pretty(state)?;

    let ext = path.extension().expect("state path has no extension");
    let tmp_ext = format!("{}.{}", ext.to_string_lossy(), "tmp");
    let tmp_path = path.with_extension(&tmp_ext);

    println!("tmp_path: {}", tmp_path.display());

    fs::write(&tmp_path, s)
        .context("failed to write state to disk")?;

    fs::rename(&tmp_path, path)
        .context("failed to overwrite previous state")?;

    Ok(())
}

fn new_state() -> State {
    State {
        plan: make_plan(),
        results: vec![],
    }
}

fn make_plan() -> Plan {

    let mut cases = vec![];
    let mut baseline = vec![];

    // Gather the set of cfgs that makeup the baseline profile
    for c in CONFIG_ITEMS {
        let baseline_cfg = (c.into(), c.default.into());
        baseline.push(baseline_cfg.clone());
    }

    // Test the baseline case. An empty config vec here means that this
    // experiment is run without adding any cfgs to the baseline profile.
    cases.push(Experiment {
        configs: vec![],
    });

    for c in CONFIG_ITEMS {
        for vals in c.values {
            let ex = Experiment {
                configs: vec![(c.into(), (*vals).into())],
            };

            let all_cfgs_in_baseline =
                ex.configs.iter().all(|i| baseline.contains(i));

            if all_cfgs_in_baseline {
                println!("skipped redundant baseline case {}", ex.display());
                continue;
            }

            cases.push(ex);
        }
    }

    Plan {
        baseline,
        cases,
    }
}

fn parse_state(s: &str) -> Result<State> {
    Ok(serde_json::from_str(s)?)
}

fn run_experiments(opts: &Options, state: &mut State) -> Result<()> {

    state.validate();

    for (idx, case) in state.plan.cases.iter().enumerate() {
        println!("case {}: {}", idx, case.display());
    }

    let baseline = &state.plan.baseline;
    for (idx, case) in state.plan.cases.iter().enumerate() {

        let previously_run = state.results.len() >= idx + 1;

        if !previously_run {
            println!("running experiment {}: {}", idx, case.display());
            state.results.push(run_experiment(opts, baseline, case)?);
            state.validate();
            save_state(&opts.state_path(), state);
        } else {
            println!("skipping previously-run experiment {}, {}", idx, case.display());
        }
    }

    Ok(())
}

fn run_experiment(opts: &Options,
                  baseline: &[DefinedItem],
                  case: &Experiment) -> Result<ExpResult> {

    let items = merge_items(baseline, &case.configs);
    let envs = items_to_envs(&items);

    run_cargo(opts, "clean", &[], envs.clone())?;

    let build_time = time(&|| {
        run_cargo(opts, "bench", &["--no-run"], envs.clone())
    })?;

    let run_time = time(&|| {
        run_cargo(opts, "bench", &[], envs.clone())
    })?;

    Ok(ExpResult { build_time, run_time })
}

fn run_cargo<'a>(opts: &Options, subcmd: &str, args: &[&str],
                 envs: Vec<(String, String)>) -> Result<()> {
    let clean_cmd = cargo_command(opts, subcmd, args, envs.clone());

    let cmd_result = run_command(clean_cmd)
        .context("error running cargo")?;

    if let Err(e) = cmd_result {
        let msg = "cargo command failed";
        println!("msg");
        Err(e).context(msg)?;
    }

    Ok(())
}

fn cargo_command(opts: &Options,
                 subcmd: &str,
                 args: &[&str],
                 envs: Vec<(String, String)>) -> Command {

    // FIXME: Show the common args and envs as well
    println!("running cargo with args {:?}", args);
    println!("running cargo with envs {:?}", envs);

    let common_args = &["-vv", "-Zunstable-options", "-Zconfig-profile"];
    // FIXME: why do I need a Vec here instead of like common_args?
    let common_envs = vec![("RUSTC_BOOTSTRAP", "1")];
    let manifest_arg = &format!("--manifest-path={}", opts.manifest_path.display());

    let mut cmd = Command::new("cargo");
    cmd.arg(subcmd);
    cmd.arg(manifest_arg);
    cmd.args(common_args);
    cmd.args(args);
    cmd.envs(common_envs);
    cmd.envs(envs);
    cmd
}

fn run_command(mut cmd: Command) -> Result<Result<()>> {
    let status = cmd.status()?;

    Ok(if status.success() {
        Ok(())
    } else {
        Err(err_msg("command failed"))
    })
}

fn merge_items(baseline: &[DefinedItem], items: &[DefinedItem]) -> Vec<DefinedItem> {

    // Create the baseline, which should be a full set of config options
    let mut new = baseline.to_vec();

    // Merge the test case items into the baseline
    for ref item in items {
        let mut found = false;

        // Look for the config item in the baseline
        for base in new.iter_mut() {
            if base.0 == base.0 {
                base.1 = item.1.clone();

                found = true;
                break;
            }
        }

        assert!(found);
    }

    new
}

fn items_to_envs(items: &[DefinedItem]) -> Vec<(String, String)> {
    items.iter().map(|i| {
        (i.0.env_var.clone(), i.1.clone())
    }).collect()
}

fn time(f: &Fn() -> Result<()>) -> Result<Duration> {
    let start = Instant::now();
    f()?;
    let end = Instant::now();

    Ok(end.duration_since(start))
}

fn gen_report(opts: &Options, state: &State) -> Result<Report> {

    state.validate();

    assert!(state.results.len() == state.plan.cases.len());

    let mut results =
        state.plan.cases.iter().cloned()
        .zip(state.results.iter().cloned())
        .collect::<Vec<_>>();

    //let (baseline, results) = split_results(results, &state.plan.baseline);

    results.sort_by_key(|x| x.1.build_time + x.1.run_time);

    Ok(Report {
        results_by_total_time: results
    })
}

/*fn split_results(mut res: Vec<ExpAndResult>, baseline: &[DefinedItem])
                 -> (ExpAndResult, Vec<ExpAndResult>) {

    let idx = res.iter().position(|e| e.0.configs == baseline);
    let idx = idx.expect("no baseline in results?");

    let baseline = res.remove(idx);

    (baseline, res)
}*/

fn log_report(report: &Report) -> Result<()> {
    println!("results:");
    for r in &report.results_by_total_time {
        // FIXME: Duration's Debug ignores the width format specifier
        println!("{:7.2?} ({:7.2?} build / {:7.2?} run)- {}",
                 r.1.build_time + r.1.run_time,
                 r.1.build_time, r.1.run_time,
                 r.0.display());
    }

    Ok(())
}

fn render_html(report: &Report) -> Result<()> {
    Ok(())
}

