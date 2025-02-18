#[rustfmt::skip]

use anyhow::Result;
use std::time::Duration;

use ic_prep_lib::subnet_configuration::constants::{NNS_SUBNET_SIZE, SMALL_APP_SUBNET_MAX_SIZE};
use ic_tests::driver::new::group::SystemTestGroup;
use ic_tests::networking::subnet_update_workload::{config, test};
use ic_tests::systest;

// Test parameters
const RPS: usize = 5;
const PAYLOAD_SIZE_BYTES: usize = 100_000;
const MIN_SUCCESS_RATIO: f64 = 0.95;
const USE_BOUNDARY_NODE: bool = false;
const WORKLOAD_RUNTIME: Duration = Duration::from_secs(30 * 60);
// Timeout parameters
const TASK_TIMEOUT_DELTA: Duration = Duration::from_secs(10 * 60);
const OVERALL_TIMEOUT_DELTA: Duration = Duration::from_secs(5 * 60);

fn main() -> Result<()> {
    let per_task_timeout: Duration = WORKLOAD_RUNTIME + TASK_TIMEOUT_DELTA; // This should be a bit larger than the workload execution time.
    let overall_timeout: Duration = per_task_timeout + OVERALL_TIMEOUT_DELTA; // This should be a bit larger than the per_task_timeout.
    let config = |env| {
        config(
            env,
            NNS_SUBNET_SIZE,
            SMALL_APP_SUBNET_MAX_SIZE,
            USE_BOUNDARY_NODE,
            None,
        )
    };
    let test = |env| {
        test(
            env,
            RPS,
            PAYLOAD_SIZE_BYTES,
            WORKLOAD_RUNTIME,
            USE_BOUNDARY_NODE,
            MIN_SUCCESS_RATIO,
        )
    };
    SystemTestGroup::new()
        .with_setup(config)
        .add_test(systest!(test))
        .with_timeout_per_test(per_task_timeout) // each task (including the setup function) may take up to `per_task_timeout`.
        .with_overall_timeout(overall_timeout) // the entire group may take up to `overall_timeout`.
        .execute_from_args()?;
    Ok(())
}
