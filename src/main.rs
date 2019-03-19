//! B-C-P! The Rust compile-time / run-time performance analyzer.
//!
//! ## TODO
//!
//! - better project name
//! - better name than 'state'
//! - better name than 'item' (cfg?)
//! - better error reporting
//! - logging

#![allow(warnings)] // TODO tmp

#[macro_use]
extern crate structopt;
#[macro_use]
extern crate serde_derive;

use failure::{ResultExt, Error};
use std::path::{PathBuf, Path};
use std::result::Result as StdResult;
use std::time::Duration;
use std::fs;
use std::io;
use std::process::Command;
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
        default: "true",
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
        default: "true",
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
        default: "true",
    },
];

struct StaticConfigItem {
    path: &'static str,
    env_var: &'static str,
    values: &'static [&'static str],
    default: &'static str,
}


#[derive(Serialize, Deserialize, Clone, Eq, PartialEq)]
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

#[derive(Serialize, Deserialize)]
struct Experiment {
    configs: Vec<DefinedItem>,
}

impl Experiment {
    fn display(&self) -> String {
        if self.configs.is_empty() {
            return "(empty)".to_string();
        }
        
        let mut buf = String::new();

        for &(ref cfg, ref val) in &self.configs {
            buf.push_str(&format!("{}={},", cfg.path, val));
        }

        buf   
    }
}

#[derive(Serialize, Deserialize)]
struct ExpResult {
    build_time: Duration,
    run_time: Duration,
}

struct Report { }

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
    let tmp_ext = format!("{:?}.{}", ext, ".tmp");
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

    for c in CONFIG_ITEMS {

        baseline.push((c.into(), c.default.into()));

        let ignore_case = c.values.is_empty();
        if ignore_case { continue }

        for vals in c.values {
            cases.push(Experiment {
                configs: vec![(c.into(), (*vals).into())],
            });
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

    let baseline = &state.plan.baseline;
    for (idx, case) in state.plan.cases.iter().enumerate() {

        assert!(state.plan.cases.len() >= state.results.len());

        let previously_run = state.results.len() >= idx + 1;

        if !previously_run {
            println!("running experiment {}: {}", idx, case.display());
            state.results.push(run_experiment(baseline, case)?);
            save_state(&opts.state_path(), state);
        } else {
            println!("skipping previously-run experiment {}, {}", idx, case.display());
        }
    }

    Ok(())
}

fn run_experiment(baseline: &[DefinedItem], case: &Experiment) -> Result<ExpResult> {
    let items = merge_items(baseline, &case.configs);
    let envs = items_to_envs(&items);

    let cmd_res = run_cargo(envs.clone(), &["clean"]);

    panic!()
}

fn run_cargo<'a>(envs: Vec<(String, String)>, args: &[&str]) -> Result<()> {
    let clean_cmd = cargo_command(envs.clone(), &["clean"]);

    run_command(clean_cmd)
}

fn cargo_command<'a>(envs: Vec<(String, String)>, args: &[&str]) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.envs(envs);
    cmd.args(args);
    cmd
}

fn run_command(cmd: Command) -> Result<()> {
    panic!()
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

fn gen_report(opts: &Options, state: &State) -> Result<Report> {
    panic!()
}

fn render_html(report: &Report) -> Result<()> {
    panic!()
}

