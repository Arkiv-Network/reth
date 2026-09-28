//! Access to a node's [`CanonicalInMemoryState`], as a trait.
//!
//! # Why this exists
//!
//! The engine keeps the chain's unpersisted tip — canonical head, safe and finalized
//! markers, the pending block, and the in-memory blocks above the database tip — in a
//! [`CanonicalInMemoryState`]. Whoever launches the engine has to hand that value to
//! the tree handler.
//!
//! Historically the only way to obtain it was `BlockchainProvider::canonical_in_memory_state`,
//! an *inherent* method. An inherent method cannot be abstracted over, so every caller
//! that needed it had to name `BlockchainProvider` concretely — which in turn forced the
//! engine orchestrator and the node launcher to hardcode that one provider type, even
//! though everything below them was already generic.
//!
//! [`CanonicalStateProvider`] is that method, promoted to a trait, so an alternative
//! provider (one backed by a different storage engine, say) can supply the same value and
//! be accepted everywhere `BlockchainProvider` was.

use crate::CanonicalInMemoryState;
use reth_primitives_traits::NodePrimitives;

/// A provider that owns the node's [`CanonicalInMemoryState`].
///
/// Implement this for any provider you intend to run the engine on. The engine calls it
/// exactly once at startup, to obtain the handle it then shares with the tree handler;
/// it is not on any hot path.
///
/// # Contract
///
/// The returned handle must be the *same* shared state the provider itself reads from —
/// typically an `Arc` clone, not a copy. The engine writes the canonical head, the
/// finalized and safe markers, and the in-memory block set through this handle, and the
/// provider is expected to observe those writes when answering queries for
/// `latest`, `pending`, `safe` and `finalized`. A provider that hands out a detached
/// clone will appear to work and then serve stale reads forever.
///
/// # Example
///
/// A provider that keeps the state in a field — the shape `BlockchainProvider` uses:
///
/// ```ignore
/// use reth_chain_state::{CanonicalInMemoryState, CanonicalStateProvider};
/// use reth_ethereum_primitives::EthPrimitives;
///
/// #[derive(Clone)]
/// struct MyProvider {
///     // Cheap to clone, and every clone observes the same writes.
///     canonical_in_memory_state: CanonicalInMemoryState<EthPrimitives>,
/// }
///
/// impl CanonicalStateProvider for MyProvider {
///     type Primitives = EthPrimitives;
///
///     fn canonical_in_memory_state(&self) -> CanonicalInMemoryState<Self::Primitives> {
///         self.canonical_in_memory_state.clone()
///     }
/// }
/// ```
pub trait CanonicalStateProvider {
    /// The node primitive types this state is keyed on.
    type Primitives: NodePrimitives;

    /// Returns a handle to the canonical in-memory state.
    ///
    /// Cheap: this clones a shared handle, it does not copy the state.
    fn canonical_in_memory_state(&self) -> CanonicalInMemoryState<Self::Primitives>;
}

impl<T: CanonicalStateProvider> CanonicalStateProvider for &T {
    type Primitives = T::Primitives;

    fn canonical_in_memory_state(&self) -> CanonicalInMemoryState<Self::Primitives> {
        (*self).canonical_in_memory_state()
    }
}

impl<T: CanonicalStateProvider> CanonicalStateProvider for std::sync::Arc<T> {
    type Primitives = T::Primitives;

    fn canonical_in_memory_state(&self) -> CanonicalInMemoryState<Self::Primitives> {
        (**self).canonical_in_memory_state()
    }
}
