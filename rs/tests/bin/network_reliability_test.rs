use std::time::Duration;

#[rustfmt::skip]

use anyhow::Result;

use ic_tests::driver::new::group::SystemTestGroup;
use ic_tests::networking::network_reliability::{setup, test, Config};
use ic_tests::systest;

// Test parameters
const CONFIG: Config = Config {
    nodes_system_subnet: 4,
    nodes_app_subnet: 4,
    runtime: Duration::from_secs(180),
    rps: 100,
    max_failures_ratio: 0.05,
};
// Timeout parameters
const TASK_TIMEOUT: Duration = Duration::from_secs(320 * 60);
const OVERALL_TIMEOUT: Duration = Duration::from_secs(350 * 60);

fn main() -> Result<()> {
    let setup = |env| setup(env, CONFIG);
    let test = |env| test(env, CONFIG);
    SystemTestGroup::new()
        .with_setup(setup)
        .add_test(systest!(test))
        .with_timeout_per_test(TASK_TIMEOUT) // each task (including the setup function) may take up to `per_task_timeout`.
        .with_overall_timeout(OVERALL_TIMEOUT) // the entire group may take up to `overall_timeout`.
        .execute_from_args()?;
    Ok(())
}
