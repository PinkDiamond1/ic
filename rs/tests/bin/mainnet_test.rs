use std::time::Duration;

#[rustfmt::skip]

use anyhow::Result;

use ic_tests::driver::new::group::SystemTestGroup;
use ic_tests::mainnet_tests::mainnet_api::{mainnet_basic_test, mainnet_config};
use ic_tests::systest;

fn main() -> Result<()> {
    SystemTestGroup::new()
        .with_overall_timeout(Duration::from_secs(25 * 60))
        .without_farm()
        .with_setup(mainnet_config)
        .add_test(systest!(mainnet_basic_test))
        .execute_from_args()?;
    Ok(())
}
