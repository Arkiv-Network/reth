//! [`CanonicalStateProvider`]: access to a node's [`CanonicalInMemoryState`], as a trait.

use crate::CanonicalInMemoryState;
use reth_primitives_traits::NodePrimitives;

/// A provider that owns the node's [`CanonicalInMemoryState`].
///
/// The engine calls this once at startup to get the handle it shares with the tree
/// handler. The handle must be the *same* shared state the provider itself reads from,
/// not a detached copy: the engine writes the canonical head and the safe/finalized
/// markers through it, and a provider that clones the wrong way will silently serve
/// stale reads.
pub trait CanonicalStateProvider {
    /// The node primitive types this state is keyed on.
    type Primitives: NodePrimitives;

    /// Returns a handle to the canonical in-memory state.
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
