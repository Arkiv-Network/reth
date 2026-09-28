//! Engine orchestrator launch helper.
//!
//! Provides [`EngineOrchestratorBuilder`](crate::launch::EngineOrchestratorBuilder) which wires
//! together all engine components and builds a
//! [`ChainOrchestrator`](crate::chain::ChainOrchestrator) ready to be polled as a `Stream`.

use crate::{
    backfill::PipelineSync,
    chain::ChainOrchestrator,
    download::BasicBlockDownloader,
    engine::{EngineApiKind, EngineApiRequest, EngineApiRequestHandler, EngineHandler},
    persistence::PersistenceHandle,
    tree::{EngineApiTreeHandler, EngineValidator, TreeConfig, WaitForCaches},
};
use futures::Stream;
use reth_chain_state::CanonicalStateProvider;
use reth_consensus::FullConsensus;
use reth_engine_primitives::BeaconEngineMessage;
use reth_evm::ConfigureEvm;
use reth_network_p2p::{BlockAccessListsClient, BlockClient};
use reth_payload_builder::PayloadBuilderHandle;
use reth_primitives_traits::NodePrimitives;
use reth_provider::{
    providers::ProviderNodeTypes, BalProvider, BlockReader, ChangeSetReader,
    DatabaseProviderFactory, ProviderFactory, PruneCheckpointReader, StageCheckpointReader,
    StateProviderFactory, StateReader, StorageChangeSetReader, StorageSettingsCache,
};
use reth_prune::PrunerWithFactory;
use reth_stages_api::{MetricEventsSender, Pipeline};
use reth_storage_overlay::OverlayManager;
use reth_tasks::Runtime;
use std::sync::Arc;

/// The [`ChainOrchestrator`] built by [`EngineOrchestratorBuilder`].
pub type EngineOrchestrator<T, N, Client, S, B> = ChainOrchestrator<
    EngineHandler<
        EngineApiRequestHandler<EngineApiRequest<T, N>, N>,
        S,
        BasicBlockDownloader<Client, <N as NodePrimitives>::Block>,
    >,
    B,
>;

/// How the engine obtains its [`PersistenceHandle`].
///
/// [`Default`](Self::Default) spawns reth's own service, which writes through a
/// [`ProviderFactory`]. [`External`](Self::External) takes a handle to a service the
/// caller spawned (see [`PersistenceHandle::new`]) — for a node whose committed state
/// does not live in MDBX. The engine treats a `SaveBlocks` reply as durable.
// Built once per node launch, so the size gap between the variants costs nothing.
#[expect(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum EnginePersistence<N: ProviderNodeTypes> {
    /// Spawn reth's built-in persistence service over the given provider factory.
    Default {
        /// Provider factory handed to the persistence service.
        provider: ProviderFactory<N>,
        /// Pruner run by the persistence service.
        pruner: PrunerWithFactory<ProviderFactory<N>>,
    },
    /// Use a persistence service the caller has already spawned, and owns durability,
    /// pruning and crash recovery for.
    External(PersistenceHandle<N::Primitives>),
}

impl<N: ProviderNodeTypes> EnginePersistence<N> {
    fn into_handle(self, sync_metrics_tx: MetricEventsSender) -> PersistenceHandle<N::Primitives> {
        match self {
            Self::Default { provider, pruner } => {
                PersistenceHandle::<N::Primitives>::spawn_service(provider, pruner, sync_metrics_tx)
            }
            Self::External(handle) => handle,
        }
    }
}

/// Components needed to build the engine [`ChainOrchestrator`] that drives the chain forward.
///
/// [`build`](Self::build) spawns and wires together the following components:
///
/// - **[`BasicBlockDownloader`]** — downloads blocks on demand from the network during live sync.
/// - **[`PersistenceHandle`]** — spawns the persistence service on a background thread for writing
///   blocks and performing pruning outside the critical consensus path.
/// - **[`EngineApiTreeHandler`]** — spawns the tree handler that processes engine API requests
///   (`newPayload`, `forkchoiceUpdated`) and maintains the in-memory chain state.
/// - **[`EngineApiRequestHandler`]** + **[`EngineHandler`]** — glue that routes incoming CL
///   messages to the tree handler and manages download requests.
/// - **[`PipelineSync`]** — wraps the staged sync [`Pipeline`] for backfill sync when the node
///   needs to catch up over large block ranges.
///
/// The returned orchestrator implements [`Stream`] and yields
/// [`ChainEvent`]s.
///
/// [`ChainEvent`]: crate::chain::ChainEvent
#[derive(Debug)]
pub struct EngineOrchestratorBuilder<N, P, Client, S, V, C>
where
    N: ProviderNodeTypes,
{
    /// The engine API flavor to run.
    pub engine_kind: EngineApiKind,
    /// Consensus used to validate downloaded and incoming blocks.
    pub consensus: Arc<dyn FullConsensus<N::Primitives>>,
    /// Network client used to download blocks during live sync.
    pub client: Client,
    /// Incoming consensus layer messages.
    pub incoming_requests: S,
    /// Staged sync pipeline used for backfill.
    pub pipeline: Pipeline<N>,
    /// Runtime the pipeline runs on.
    pub pipeline_task_spawner: Runtime,
    /// How the engine gets its persistence service. See [`EnginePersistence`].
    pub persistence: EnginePersistence<N>,
    /// Provider backing the engine tree; see [`build`](Self::build) for its bounds.
    pub blockchain_db: P,
    /// Handle to the payload builder service.
    pub payload_builder: PayloadBuilderHandle<N::Payload>,
    /// Validator for incoming payloads.
    pub payload_validator: V,
    /// Overlay manager for state on top of the database.
    pub overlay_manager: OverlayManager<N::Primitives>,
    /// Engine tree configuration.
    pub tree_config: TreeConfig,
    /// Sender for sync metric events.
    pub sync_metrics_tx: MetricEventsSender,
    /// EVM configuration used to execute payloads.
    pub evm_config: C,
    /// Runtime used to spawn engine tree tasks.
    pub runtime: Runtime,
}

impl<N, P, Client, S, V, C> EngineOrchestratorBuilder<N, P, Client, S, V, C>
where
    N: ProviderNodeTypes,
    // `EngineApiTreeHandler`'s own bounds, plus `CanonicalStateProvider` for the one
    // call `build` makes itself.
    P: DatabaseProviderFactory
        + BlockReader<
            Block = <N::Primitives as NodePrimitives>::Block,
            Header = <N::Primitives as NodePrimitives>::BlockHeader,
        > + StateProviderFactory
        + StateReader<Receipt = <N::Primitives as NodePrimitives>::Receipt>
        + BalProvider
        + CanonicalStateProvider<Primitives = N::Primitives>
        + Clone
        + 'static,
    P::Provider: BlockReader<
            Block = <N::Primitives as NodePrimitives>::Block,
            Header = <N::Primitives as NodePrimitives>::BlockHeader,
        > + PruneCheckpointReader
        + StageCheckpointReader
        + ChangeSetReader
        + StorageChangeSetReader
        + StorageSettingsCache
        + 'static,
    Client: BlockClient<Block = <N::Primitives as NodePrimitives>::Block>
        + BlockAccessListsClient
        + 'static,
    S: Stream<Item = BeaconEngineMessage<N::Payload>> + Send + Sync + Unpin + 'static,
    V: EngineValidator<N::Payload> + WaitForCaches,
    C: ConfigureEvm<Primitives = N::Primitives> + 'static,
{
    /// Spawns the engine services and returns the [`ChainOrchestrator`] driving them.
    pub fn build(
        self,
    ) -> EngineOrchestrator<N::Payload, N::Primitives, Client, S, PipelineSync<N>> {
        let Self {
            engine_kind,
            consensus,
            client,
            incoming_requests,
            pipeline,
            pipeline_task_spawner,
            persistence,
            blockchain_db,
            payload_builder,
            payload_validator,
            overlay_manager,
            tree_config,
            sync_metrics_tx,
            evm_config,
            runtime,
        } = self;

        let downloader = BasicBlockDownloader::new(client, consensus.clone());

        let persistence_handle = persistence.into_handle(sync_metrics_tx);

        let canonical_in_memory_state = blockchain_db.canonical_in_memory_state();

        let (to_tree_tx, from_tree) = EngineApiTreeHandler::spawn_new(
            blockchain_db,
            consensus,
            payload_validator,
            persistence_handle,
            payload_builder,
            canonical_in_memory_state,
            overlay_manager,
            tree_config,
            engine_kind,
            evm_config,
            runtime,
        );

        let engine_handler = EngineApiRequestHandler::new(to_tree_tx, from_tree);
        let handler = EngineHandler::new(engine_handler, downloader, incoming_requests);

        let backfill_sync = PipelineSync::new(pipeline, pipeline_task_spawner);

        ChainOrchestrator::new(handler, backfill_sync)
    }
}
