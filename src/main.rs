#![allow(warnings)]

#[macro_use]
extern crate structopt;
#[macro_use]
extern crate serde_derive;

use failure::Error;
use std::path::{PathBuf, Path};
use std::result::Result as StdResult;
use std::time::Duration;
use structopt::StructOpt;

type Result<T> = StdResult<T, Error>;

struct StaticConfigItem {
    path: &'static str,
    env_var: &'static str,
    values: &'static [&'static str],
    default: &'static str,
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

#[derive(Serialize, Deserialize)]
struct ConfigItem {
    path: String,
    env_var: String,
    values: Vec<String>,
    default: String,
}

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

#[derive(StructOpt)]
struct Options {
    #[structopt(long = "manifest-path", default_value = "Cargo.toml")]
    manifest_path: PathBuf,
    #[structopt(long = "data-dir", default_value = "./", parse(from_os_str))]
    data_dir: PathBuf,
}

impl Options {
    fn state_path(&self) -> PathBuf {
        self.data_dir.join("bcp-state.json")
    }

    fn report_path(&self) -> PathBuf {
        self.data_dir.join("bcp-report.html")
    }
}

fn main() -> Result<()> {
    let opts = Options::from_args();

    let mut state = load_state(&opts.state_path())?;

    run_experiments(&opts, &mut state)?;

    report(&state)?;
    
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct State {
    plan: Vec<Experiment>,
    results: Vec<ExpResult>,
}

#[derive(Serialize, Deserialize)]
struct Experiment {
    configs: Vec<(ConfigItem, String)>,
}

#[derive(Serialize, Deserialize)]
struct ExpResult {
    build_time: Duration,
    run_time: Duration,
}

fn load_state(path: &Path) -> Result<State> {
    panic!()
}

fn run_experiments(opts: &Options, state: &mut State) -> Result<()> {
    panic!()
}

fn report(state: &State) -> Result<()> {
    panic!()
}
