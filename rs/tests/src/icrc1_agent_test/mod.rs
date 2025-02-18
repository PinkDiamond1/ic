use std::convert::TryFrom;

use candid::{Encode, Nat, Principal};
use canister_test::{Canister, PrincipalId};
use ic_icrc1::Account;
use ic_icrc1_agent::{CallMode, Icrc1Agent, TransferArg, Value};
use ic_icrc1_ledger::{InitArgs, LedgerArgument};
use ic_nns_test_utils::itest_helpers::install_rust_canister_from_path;
use ic_registry_subnet_type::SubnetType;
use icp_ledger::ArchiveOptions;

use crate::{
    driver::{
        ic::{InternetComputer, Subnet},
        pot_dsl::{par, pot_with_setup, sys_t, Pot},
        test_env::TestEnv,
        test_env_api::{
            HasDependencies, HasGroupSetup, HasPublicApiUrl, HasTopologySnapshot, IcNodeContainer,
        },
    },
    util::{assert_create_agent, block_on, runtime_from_url},
};

pub fn icrc1_agent_test_pot() -> Pot {
    pot_with_setup(
        "icrc1_agent_test_pot",
        config,
        par(vec![sys_t("icrc1_agent_test", test)]),
    )
}

pub fn config(env: TestEnv) {
    env.ensure_group_setup_created();
    InternetComputer::new()
        .add_subnet(Subnet::fast_single_node(SubnetType::Application))
        .setup_and_start(&env)
        .expect("failed to setup IC under test");
    env.topology_snapshot().subnets().for_each(|subnet| {
        subnet
            .nodes()
            .for_each(|node| node.await_status_is_healthy().unwrap())
    });
}

pub fn test(env: TestEnv) {
    let nns_node = env
        .topology_snapshot()
        .root_subnet()
        .nodes()
        .next()
        .unwrap();
    let nns_runtime = runtime_from_url(nns_node.get_public_url(), nns_node.effective_canister_id());
    let nns_agent = nns_node.with_default_agent(|agent| async move { agent });
    block_on(async move {
        let minting_user = PrincipalId::new_user_test_id(100);
        let user1 = PrincipalId::try_from(nns_agent.get_principal().unwrap().as_ref()).unwrap();
        let user2 = PrincipalId::new_user_test_id(102);
        let account1 = Account {
            owner: user1,
            subaccount: None,
        };
        let account2 = Account {
            owner: user2,
            subaccount: None,
        };
        let minting_account = Account {
            owner: minting_user,
            subaccount: None,
        };
        let mut ledger = nns_runtime
            .create_canister_max_cycles_with_retries()
            .await
            .expect("Unable to create canister");

        let init_args = InitArgs {
            minting_account,
            initial_balances: vec![(account1, 1_000_000_000)],
            transfer_fee: 1_000,
            token_name: "Example Token".to_string(),
            token_symbol: "XTK".to_string(),
            metadata: vec![],
            archive_options: ArchiveOptions {
                trigger_threshold: 1000,
                num_blocks_to_archive: 1000,
                node_max_memory_size_bytes: None,
                max_message_size_bytes: None,
                controller_id: minting_user,
                cycles_for_archive_creation: None,
                max_transactions_per_response: None,
            },
        };
        install_icrc1_ledger(&env, &mut ledger, &LedgerArgument::Init(init_args.clone())).await;

        /////////////
        // test

        let agent = Icrc1Agent {
            agent: assert_create_agent(nns_node.get_public_url().as_str()).await,
            ledger_canister_id: Principal::try_from_slice(ledger.canister_id().as_ref()).unwrap(),
        };

        // name
        assert_eq!(
            init_args.token_name,
            agent.name(CallMode::Query).await.unwrap()
        );
        assert_eq!(
            init_args.token_name,
            agent.name(CallMode::Update).await.unwrap()
        );

        // symbol
        assert_eq!(
            init_args.token_symbol,
            agent.symbol(CallMode::Query).await.unwrap()
        );
        assert_eq!(
            init_args.token_symbol,
            agent.symbol(CallMode::Update).await.unwrap()
        );

        // decimal
        assert_eq!(
            ic_ledger_core::tokens::DECIMAL_PLACES as u8,
            agent.decimals(CallMode::Query).await.unwrap()
        );
        assert_eq!(
            ic_ledger_core::tokens::DECIMAL_PLACES as u8,
            agent.decimals(CallMode::Update).await.unwrap()
        );

        // total_supply
        assert_eq!(
            Nat::from(1_000_000_000u64),
            agent.total_supply(CallMode::Query).await.unwrap()
        );
        assert_eq!(
            Nat::from(1_000_000_000u64),
            agent.total_supply(CallMode::Update).await.unwrap()
        );

        // fee
        assert_eq!(
            Nat::from(init_args.transfer_fee),
            agent.fee(CallMode::Query).await.unwrap()
        );
        assert_eq!(
            Nat::from(init_args.transfer_fee),
            agent.fee(CallMode::Update).await.unwrap()
        );

        // minting account
        assert_eq!(
            Some(&init_args.minting_account),
            agent
                .minting_account(CallMode::Query)
                .await
                .unwrap()
                .as_ref()
        );
        assert_eq!(
            Some(&init_args.minting_account),
            agent
                .minting_account(CallMode::Update)
                .await
                .unwrap()
                .as_ref()
        );

        // metadata
        let expected_metadata = vec![
            Value::entry(
                "icrc1:decimals",
                ic_ledger_core::tokens::DECIMAL_PLACES as u64,
            ),
            Value::entry("icrc1:name", init_args.token_name),
            Value::entry("icrc1:symbol", init_args.token_symbol),
            Value::entry("icrc1:fee", init_args.transfer_fee),
        ];
        assert_eq!(
            expected_metadata,
            agent.metadata(CallMode::Query).await.unwrap()
        );
        assert_eq!(
            expected_metadata,
            agent.metadata(CallMode::Update).await.unwrap()
        );

        // balance_of
        assert_eq!(
            Nat::from(1_000_000_000u64),
            agent.balance_of(account1, CallMode::Query).await.unwrap()
        );
        assert_eq!(
            Nat::from(1_000_000_000u64),
            agent.balance_of(account1, CallMode::Update).await.unwrap()
        );

        // transfer
        let amount = 10_000_000u64;
        let _block = agent
            .transfer(TransferArg {
                from_subaccount: None,
                to: Account {
                    owner: user2,
                    subaccount: None,
                },
                fee: None,
                created_at_time: None,
                amount: Nat::from(amount),
                memo: None,
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            Nat::from(1_000_000_000u64 - amount - init_args.transfer_fee),
            agent.balance_of(account1, CallMode::Query).await.unwrap()
        );
        assert_eq!(
            Nat::from(amount),
            agent.balance_of(account2, CallMode::Query).await.unwrap()
        );
    });
}

pub async fn install_icrc1_ledger<'a>(
    env: &TestEnv,
    canister: &mut Canister<'a>,
    args: &LedgerArgument,
) {
    install_rust_canister_from_path(
        canister,
        env.get_dependency_path("rs/rosetta-api/icrc1/ledger/ledger_canister.wasm"),
        Some(Encode!(&args).unwrap()),
    )
    .await
}
