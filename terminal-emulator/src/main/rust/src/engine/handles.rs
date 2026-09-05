//! Strong-reference engine handles and the current render binding.
//!
//! Map lookup, publication, and revocation share one linearization lock. Removing
//! a handle prevents new leases, but existing `Arc` leases remain valid. Handles
//! are positive, never reused within a registry, and permanently exhaust at
//! `i64::MAX`; they are not allocation addresses.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

struct State<T> {
    last_handle: i64,
    entries: BTreeMap<i64, Arc<T>>,
    binding: Option<(i64, Arc<T>)>,
}

/// Owns live engines and at most one additional strong render-binding reference.
///
/// No payload code runs under the state mutex. Displaced references are moved
/// out and dropped after unlocking, including values rejected on exhaustion.
/// Poison is recovered because mutations never invoke user callbacks.
pub struct EngineHandles<T> {
    state: Mutex<State<T>>,
}

impl<T> EngineHandles<T> {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State {
                last_handle: 0,
                entries: BTreeMap::new(),
                binding: None,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State<T>> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// Issues a fresh positive handle, or rejects the value on exhaustion.
    pub fn insert(&self, value: Arc<T>) -> Option<i64> {
        let mut state = self.lock();
        let Some(handle) = state.last_handle.checked_add(1) else {
            drop(state);
            drop(value);
            return None;
        };
        let displaced = state.entries.insert(handle, value);
        state.last_handle = handle;
        drop(state);
        // Monotonic allocation means this is None; still never drop an Arc
        // under the lock if this implementation is changed in the future.
        drop(displaced);
        Some(handle)
    }

    /// Clones a strong lease while publication ownership is still protected.
    pub fn acquire(&self, handle: i64) -> Option<Arc<T>> {
        if handle <= 0 {
            return None;
        }
        let state = self.lock();
        let lease = state.entries.get(&handle).cloned();
        drop(state);
        lease
    }

    /// Revokes new access and atomically detaches only this handle's binding.
    /// Existing leases and another session's binding are unaffected.
    pub fn remove(&self, handle: i64) -> Option<Arc<T>> {
        if handle <= 0 {
            return None;
        }
        let mut state = self.lock();
        let removed = state.entries.remove(&handle);
        let displaced = if state.binding.as_ref().map(|binding| binding.0) == Some(handle) {
            state.binding.take()
        } else {
            None
        };
        drop(state);
        drop(displaced);
        removed
    }

    /// Publishes a live handle, or clears publication when `handle == 0`.
    /// Invalid/stale handles return false without changing the current binding.
    pub fn publish(&self, handle: i64) -> bool {
        if handle < 0 {
            return false;
        }
        let mut state = self.lock();
        let replacement = if handle == 0 {
            None
        } else {
            let Some(lease) = state.entries.get(&handle).cloned() else {
                return false;
            };
            Some((handle, lease))
        };
        let displaced = std::mem::replace(&mut state.binding, replacement);
        drop(state);
        drop(displaced);
        true
    }

    /// Returns the binding identity and a strong in-flight render lease.
    pub fn current(&self) -> Option<(i64, Arc<T>)> {
        let state = self.lock();
        let binding = state
            .binding
            .as_ref()
            .map(|(handle, value)| (*handle, Arc::clone(value)));
        drop(state);
        binding
    }
}

impl<T> Default for EngineHandles<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::EngineHandles;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Weak};

    struct LockProbe {
        registry: Weak<EngineHandles<LockProbe>>,
        drops: Arc<AtomicUsize>,
        unlocked_drops: Arc<AtomicUsize>,
    }

    impl Drop for LockProbe {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
            if let Some(registry) = self.registry.upgrade() {
                if registry.state.try_lock().is_ok() {
                    self.unlocked_drops.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
    }

    fn probe(
        registry: &Arc<EngineHandles<LockProbe>>,
        drops: &Arc<AtomicUsize>,
        unlocked_drops: &Arc<AtomicUsize>,
    ) -> Arc<LockProbe> {
        Arc::new(LockProbe {
            registry: Arc::downgrade(registry),
            drops: Arc::clone(drops),
            unlocked_drops: Arc::clone(unlocked_drops),
        })
    }

    #[test]
    fn maximum_handle_is_issued_once_and_exhaustion_is_permanent() {
        let registry = EngineHandles::new();
        registry.state.lock().unwrap().last_handle = i64::MAX - 1;
        let last = registry.insert(Arc::new(7)).unwrap();
        assert_eq!(last, i64::MAX);
        assert!(registry.publish(last));
        assert!(registry.insert(Arc::new(8)).is_none());
        assert_eq!(registry.current().unwrap().0, last);
        assert_eq!(*registry.acquire(last).unwrap(), 7);
        assert_eq!(*registry.remove(last).unwrap(), 7);
        assert!(registry.insert(Arc::new(9)).is_none());
        assert!(registry.current().is_none());
        assert_eq!(registry.state.lock().unwrap().last_handle, i64::MAX);
    }

    #[test]
    fn rejected_insert_and_removed_owner_drop_after_unlock() {
        let registry = Arc::new(EngineHandles::new());
        let drops = Arc::new(AtomicUsize::new(0));
        let unlocked_drops = Arc::new(AtomicUsize::new(0));
        let handle = registry
            .insert(probe(&registry, &drops, &unlocked_drops))
            .unwrap();
        assert!(registry.publish(handle));
        drop(registry.remove(handle));
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert_eq!(unlocked_drops.load(Ordering::SeqCst), 1);
        registry.state.lock().unwrap().last_handle = i64::MAX;
        assert!(registry
            .insert(probe(&registry, &drops, &unlocked_drops))
            .is_none());
        assert_eq!(drops.load(Ordering::SeqCst), 2);
        assert_eq!(unlocked_drops.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn displaced_binding_drops_after_unlock() {
        // Remove just the map owner through private access to make displacement
        // the last Arc drop: normal API invariants keep a map owner alive.
        for replacement in [false, true] {
            let registry = Arc::new(EngineHandles::new());
            let drops = Arc::new(AtomicUsize::new(0));
            let unlocked_drops = Arc::new(AtomicUsize::new(0));
            let handle = registry
                .insert(probe(&registry, &drops, &unlocked_drops))
                .unwrap();
            assert!(registry.publish(handle));
            let map_owner = registry.state.lock().unwrap().entries.remove(&handle);
            drop(map_owner);
            let next = if replacement {
                registry
                    .insert(probe(&registry, &drops, &unlocked_drops))
                    .unwrap()
            } else {
                0
            };
            assert!(registry.publish(next));
            assert_eq!(drops.load(Ordering::SeqCst), 1);
            assert_eq!(unlocked_drops.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn poison_recovery_preserves_live_entries_and_binding() {
        let registry = EngineHandles::new();
        let handle = registry.insert(Arc::new(42)).unwrap();
        assert!(registry.publish(handle));
        let result = std::panic::catch_unwind(|| {
            let _guard = registry.state.lock().unwrap();
            panic!("intentional test-only poisoning");
        });
        assert!(result.is_err());
        assert_eq!(*registry.acquire(handle).unwrap(), 42);
        assert_eq!(registry.current().unwrap().0, handle);
        let next = registry.insert(Arc::new(43)).unwrap();
        assert!(next > handle);
        assert!(registry.publish(next));
        assert_eq!(*registry.remove(handle).unwrap(), 42);
        assert_eq!(registry.current().unwrap().0, next);
        assert!(registry.publish(0));
        assert!(registry.current().is_none());
    }
}
