use ic_btc_adapter_client::{setup_bitcoin_adapter_clients, BitcoinAdapterClients};
use ic_btc_consensus::BitcoinPayloadBuilder;
use ic_config::{artifact_pool::ArtifactPoolConfig, subnet_config::SubnetConfig, Config};
use ic_consensus::certification::VerifierImpl;
use ic_crypto::CryptoComponent;
use ic_cycles_account_manager::CyclesAccountManager;
use ic_execution_environment::ExecutionServices;
use ic_interfaces::{
    consensus_pool::ConsensusPoolCache,
    execution_environment::{
        AnonymousQueryService, IngressFilterService, QueryExecutionService, QueryHandler,
    },
};
use ic_interfaces_certified_stream_store::CertifiedStreamStore;
use ic_interfaces_p2p::IngressIngestionService;
use ic_interfaces_registry::{LocalStoreCertifiedTimeReader, RegistryClient};
use ic_logger::{info, ReplicaLogger};
use ic_messaging::MessageRoutingImpl;
use ic_p2p::P2PThreadJoiner;
use ic_registry_subnet_type::SubnetType;
use ic_replica_setup_ic_network::{
    create_networking_stack, init_artifact_pools, P2PStateSyncClient,
};
use ic_replicated_state::ReplicatedState;
use ic_state_manager::{state_sync::StateSync, StateManagerImpl};
use ic_types::{consensus::catchup::CUPWithOriginalProtobuf, NodeId, SubnetId};
use ic_xnet_endpoint::{XNetEndpoint, XNetEndpointConfig};
use ic_xnet_payload_builder::XNetPayloadBuilderImpl;

use std::sync::Arc;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn construct_ic_stack(
    replica_logger: ReplicaLogger,
    rt_handle: tokio::runtime::Handle,
    rt_handle_xnet: tokio::runtime::Handle,
    config: Config,
    subnet_config: SubnetConfig,
    node_id: NodeId,
    subnet_id: SubnetId,
    subnet_type: SubnetType,
    registry: Arc<dyn RegistryClient + Send + Sync>,
    crypto: Arc<CryptoComponent>,
    metrics_registry: ic_metrics::MetricsRegistry,
    catch_up_package: Option<CUPWithOriginalProtobuf>,
    local_store_time_reader: Option<Arc<dyn LocalStoreCertifiedTimeReader>>,
) -> std::io::Result<(
    // TODO(SCL-213): When Rust traits support it, simplify and pass a single
    // trait.
    Arc<CryptoComponent>,
    Arc<StateManagerImpl>,
    Arc<dyn QueryHandler<State = ReplicatedState>>,
    QueryExecutionService,
    AnonymousQueryService,
    P2PThreadJoiner,
    IngressIngestionService,
    Arc<dyn ConsensusPoolCache>,
    IngressFilterService,
    XNetEndpoint,
)> {
    let artifact_pool_config = ArtifactPoolConfig::from(config.artifact_pool);

    // Determine the correct catch-up package.
    let catch_up_package = {
        use ic_types::consensus::HasHeight;
        let make_registry_cup = || {
            CUPWithOriginalProtobuf::from_cup(
                ic_consensus::dkg::make_registry_cup(&*registry, subnet_id, None)
                    .expect("Couldn't create a registry CUP"),
            )
        };
        match catch_up_package {
            // The replica was started on a CUP persisted by the orchestrator.
            Some(cup_from_orc) => {
                // The CUP passed by the orchestrator can be signed or unsigned. If it's signed, it
                // was created and signed by the subnet. An unsigned CUP was created by the orchestrator
                // from the registry CUP contents and can only happen during a subnet recovery or subnet genesis.
                let signed = !cup_from_orc
                    .cup
                    .signature
                    .signature
                    .clone()
                    .get()
                    .0
                    .is_empty();
                info!(
                    &replica_logger,
                    "Using the {} CUP with height {}",
                    if signed { "signed" } else { "unsigned" },
                    cup_from_orc.cup.height()
                );
                cup_from_orc
            }
            // This case is only possible if the replica is started without an orchestrator which
            // is currently only possible in the local development mode with `dfx`.
            None => {
                let registry_cup = make_registry_cup();
                info!(
                    &replica_logger,
                    "Using the CUP with height {} generated from the registry",
                    registry_cup.cup.height()
                );
                registry_cup
            }
        }
    };

    let artifact_pools = init_artifact_pools(
        subnet_id,
        artifact_pool_config,
        metrics_registry.clone(),
        replica_logger.clone(),
        catch_up_package,
    );

    let cycles_account_manager = Arc::new(CyclesAccountManager::new(
        subnet_config.scheduler_config.max_instructions_per_message,
        subnet_type,
        subnet_id,
        subnet_config.cycles_account_manager_config,
    ));
    let verifier = VerifierImpl::new(crypto.clone());

    let state_manager = Arc::new(StateManagerImpl::new(
        Arc::new(verifier),
        subnet_id,
        subnet_type,
        replica_logger.clone(),
        &metrics_registry,
        &config.state_manager,
        Some(artifact_pools.consensus_pool_cache.starting_height()),
        config.malicious_behaviour.malicious_flags.clone(),
    ));
    // Get the file descriptor factory object and pass it down the line to the hypervisor
    let fd_factory = state_manager.get_fd_factory();
    let execution_services = ExecutionServices::setup_execution(
        replica_logger.clone(),
        &metrics_registry,
        subnet_id,
        subnet_type,
        subnet_config.scheduler_config,
        config.hypervisor.clone(),
        Arc::clone(&cycles_account_manager),
        Arc::clone(&state_manager) as Arc<_>,
        Arc::clone(&fd_factory),
    );

    let certified_stream_store: Arc<dyn CertifiedStreamStore> =
        Arc::clone(&state_manager) as Arc<_>;

    let message_router = if config
        .malicious_behaviour
        .malicious_flags
        .maliciously_disable_execution
    {
        MessageRoutingImpl::new_fake(
            subnet_id,
            Arc::clone(&state_manager) as Arc<_>,
            execution_services.ingress_history_writer,
            &metrics_registry,
            replica_logger.clone(),
        )
    } else {
        MessageRoutingImpl::new(
            Arc::clone(&state_manager) as Arc<_>,
            Arc::clone(&certified_stream_store) as Arc<_>,
            execution_services.ingress_history_writer,
            execution_services.scheduler,
            config.hypervisor,
            Arc::clone(&cycles_account_manager),
            subnet_id,
            &metrics_registry,
            replica_logger.clone(),
            Arc::clone(&registry) as Arc<_>,
            config.malicious_behaviour.malicious_flags.clone(),
        )
    };
    let message_router = Arc::new(message_router);

    let xnet_config =
        XNetEndpointConfig::from(Arc::clone(&registry) as Arc<_>, node_id, &replica_logger);

    let xnet_endpoint = XNetEndpoint::new(
        rt_handle_xnet,
        Arc::clone(&certified_stream_store),
        Arc::clone(&crypto) as Arc<_>,
        Arc::clone(&registry),
        xnet_config,
        &metrics_registry,
        replica_logger.clone(),
    );

    // Use default runtime to spawn xnet client threads.
    let xnet_payload_builder = XNetPayloadBuilderImpl::new(
        Arc::clone(&state_manager) as Arc<_>,
        Arc::clone(&certified_stream_store) as Arc<_>,
        Arc::clone(&crypto) as Arc<_>,
        Arc::clone(&registry) as Arc<_>,
        rt_handle.clone(),
        node_id,
        subnet_id,
        &metrics_registry,
        replica_logger.clone(),
    );
    let xnet_payload_builder = Arc::new(xnet_payload_builder);

    let BitcoinAdapterClients {
        btc_testnet_client,
        btc_mainnet_client,
    } = setup_bitcoin_adapter_clients(
        replica_logger.clone(),
        &metrics_registry,
        rt_handle.clone(),
        config.adapters_config.clone(),
    );
    let self_validating_payload_builder = BitcoinPayloadBuilder::new(
        state_manager.clone(),
        &metrics_registry,
        btc_mainnet_client,
        btc_testnet_client,
        subnet_id,
        Arc::clone(&registry),
        replica_logger.clone(),
    );
    let self_validating_payload_builder = Arc::new(self_validating_payload_builder);

    let canister_http_adapter_client = ic_https_outcalls_adapter_client::setup_canister_http_client(
        rt_handle.clone(),
        &metrics_registry,
        config.adapters_config,
        execution_services.anonymous_query_handler.clone(),
        replica_logger.clone(),
        subnet_type,
    );

    let state_sync = StateSync::new(state_manager.clone(), replica_logger.clone());

    let (ingress_ingestion_service, p2p_runner) = create_networking_stack(
        metrics_registry,
        replica_logger,
        rt_handle,
        config.transport,
        config.consensus,
        config.malicious_behaviour.malicious_flags,
        node_id,
        subnet_id,
        None,
        Arc::clone(&crypto) as Arc<_>,
        Arc::clone(&state_manager) as Arc<_>,
        Arc::clone(&state_manager) as Arc<_>,
        P2PStateSyncClient::Client(state_sync),
        xnet_payload_builder as Arc<_>,
        self_validating_payload_builder as Arc<_>,
        message_router as Arc<_>,
        // TODO(SCL-213)
        Arc::clone(&crypto) as Arc<_>,
        Arc::clone(&crypto) as Arc<_>,
        Arc::clone(&crypto) as Arc<_>,
        Arc::clone(&crypto) as Arc<_>,
        registry,
        execution_services.ingress_history_reader,
        &artifact_pools,
        cycles_account_manager,
        local_store_time_reader,
        canister_http_adapter_client,
        config.nns_registry_replicator.poll_delay_duration_ms,
    );
    Ok((
        crypto,
        state_manager,
        execution_services.sync_query_handler,
        execution_services.async_query_handler,
        execution_services.anonymous_query_handler,
        p2p_runner,
        ingress_ingestion_service,
        artifact_pools.consensus_pool_cache,
        execution_services.ingress_filter,
        xnet_endpoint,
    ))
}
