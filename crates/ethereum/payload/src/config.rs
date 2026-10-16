pub use alloy_eips::eip1559::calculate_block_gas_limit;
use alloy_eips::eip1559::ETHEREUM_BLOCK_GAS_LIMIT_30M;
use alloy_primitives::{Address, Bytes, B256};
use core::fmt;
use std::sync::Arc;

/// Identifies the execution a state root is being asked for.
///
/// A build is identified by its *contents*, not by its height: two builds for the same height
/// can be in flight at once (`BasicPayloadJob::resolve_kind` races a second build against an
/// unfinished one), so a resolver keyed on the block number alone can answer with the root of
/// the other build.
///
/// Transactions are identified by `(signer, nonce)` rather than by hash, because that is what an
/// executor can reconstruct: an `Evm` is handed a `TxEnv`, which carries the signer and the nonce
/// but not the hash of the envelope they came from. The pair is no weaker as an identity — a
/// block cannot contain two transactions with the same signer and nonce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadStateRootRequest<'a> {
    /// Hash of the block being built on.
    pub parent_hash: B256,
    /// Number of the block being built.
    pub number: u64,
    /// `(signer, nonce)` of each transaction that was executed, in block order.
    pub transactions: &'a [(Address, u64)],
}

/// Supplies the state root of a block whose root is a product of execution.
///
/// Returning `None` leaves the root to the usual computation, so a resolver that does not
/// recognise a request is never a failure.
pub type PayloadStateRootResolver =
    Arc<dyn for<'a> Fn(&PayloadStateRootRequest<'a>) -> Option<B256> + Send + Sync>;

/// Settings for the Ethereum builder.
#[derive(Clone)]
pub struct EthereumBuilderConfig {
    /// Desired gas limit.
    pub desired_gas_limit: u64,
    /// Waits for the first payload to be built if there is no payload built when the payload is
    /// being resolved.
    pub await_payload_on_missing: bool,
    /// Maximum number of blobs to include per block (EIP-7872).
    ///
    /// If `None`, defaults to the protocol maximum.
    pub max_blobs_per_block: Option<u64>,
    /// Extra data for built blocks.
    pub extra_data: Bytes,
    /// Whether payload builds should skip state-root computation.
    pub skip_state_root: bool,
    /// Supplies the state root once execution has ended, for chains whose root is a product of
    /// execution rather than a walk of the account trie.
    ///
    /// Consulted after post-execution changes, so a root that only exists then — one computed by
    /// the executor's own store, say — can still reach the built header. `None` keeps the stock
    /// behaviour.
    pub state_root_resolver: Option<PayloadStateRootResolver>,
}

// Hand-written: a resolver is a function, which has no equality and no useful `Debug`. Two
// configs are equal when their settings are, and the resolver's presence is reported rather
// than its contents.
impl PartialEq for EthereumBuilderConfig {
    fn eq(&self, other: &Self) -> bool {
        self.desired_gas_limit == other.desired_gas_limit
            && self.await_payload_on_missing == other.await_payload_on_missing
            && self.max_blobs_per_block == other.max_blobs_per_block
            && self.extra_data == other.extra_data
            && self.skip_state_root == other.skip_state_root
    }
}

impl Eq for EthereumBuilderConfig {}

impl fmt::Debug for EthereumBuilderConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EthereumBuilderConfig")
            .field("desired_gas_limit", &self.desired_gas_limit)
            .field("await_payload_on_missing", &self.await_payload_on_missing)
            .field("max_blobs_per_block", &self.max_blobs_per_block)
            .field("extra_data", &self.extra_data)
            .field("skip_state_root", &self.skip_state_root)
            .field("has_state_root_resolver", &self.state_root_resolver.is_some())
            .finish()
    }
}

impl Default for EthereumBuilderConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl EthereumBuilderConfig {
    /// Create new payload builder config.
    pub const fn new() -> Self {
        Self {
            desired_gas_limit: ETHEREUM_BLOCK_GAS_LIMIT_30M,
            await_payload_on_missing: true,
            max_blobs_per_block: None,
            extra_data: Bytes::new(),
            skip_state_root: false,
            state_root_resolver: None,
        }
    }

    /// Supply the state root from execution rather than computing it.
    pub fn with_state_root_resolver(mut self, resolver: PayloadStateRootResolver) -> Self {
        self.state_root_resolver = Some(resolver);
        self
    }

    /// Set desired gas limit.
    pub const fn with_gas_limit(mut self, desired_gas_limit: u64) -> Self {
        self.desired_gas_limit = desired_gas_limit;
        self
    }

    /// Configures whether the initial payload should be awaited when the payload job is being
    /// resolved and no payload has been built yet.
    pub const fn with_await_payload_on_missing(mut self, await_payload_on_missing: bool) -> Self {
        self.await_payload_on_missing = await_payload_on_missing;
        self
    }

    /// Set the maximum number of blobs per block (EIP-7872).
    pub const fn with_max_blobs_per_block(mut self, max_blobs_per_block: Option<u64>) -> Self {
        self.max_blobs_per_block = max_blobs_per_block;
        self
    }

    /// Set the extra data for built blocks.
    pub fn with_extra_data(mut self, extra_data: Bytes) -> Self {
        self.extra_data = extra_data;
        self
    }

    /// Configures whether payload builds should skip state-root computation.
    pub const fn with_skip_state_root(mut self, skip_state_root: bool) -> Self {
        self.skip_state_root = skip_state_root;
        self
    }
}

impl EthereumBuilderConfig {
    /// Returns the gas limit for the next block based
    /// on parent and desired gas limits.
    pub fn gas_limit(&self, parent_gas_limit: u64) -> u64 {
        self.gas_limit_with_target(parent_gas_limit, None)
    }

    /// Returns the gas limit for the next block based on the parent gas limit and an optional
    /// target from payload attributes.
    pub fn gas_limit_with_target(
        &self,
        parent_gas_limit: u64,
        target_gas_limit: Option<u64>,
    ) -> u64 {
        calculate_block_gas_limit(
            parent_gas_limit,
            target_gas_limit.unwrap_or(self.desired_gas_limit),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gas_limit_uses_payload_target_when_present() {
        let parent_gas_limit = 30_000_000;
        let target_gas_limit = parent_gas_limit - 100;
        let config = EthereumBuilderConfig::new().with_gas_limit(parent_gas_limit + 100);

        assert_eq!(
            config.gas_limit_with_target(parent_gas_limit, Some(target_gas_limit)),
            target_gas_limit
        );
    }

    #[test]
    fn gas_limit_falls_back_to_configured_target() {
        let parent_gas_limit = 30_000_000;
        let desired_gas_limit = parent_gas_limit + 100;
        let config = EthereumBuilderConfig::new().with_gas_limit(desired_gas_limit);

        assert_eq!(config.gas_limit_with_target(parent_gas_limit, None), desired_gas_limit);
        assert_eq!(config.gas_limit(parent_gas_limit), desired_gas_limit);
    }
}
