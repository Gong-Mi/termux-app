// Include the production std-only module so its private exhaustion/drop tests
// also run in this registered integration target, without a second implementation.
#[path = "../src/engine/handles.rs"]
mod handles;

use handles::EngineHandles;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

struct DropSentinel(Arc<AtomicUsize>);

impl Drop for DropSentinel {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn handles_are_positive_monotonic_and_never_reused() {
    let registry = EngineHandles::new();
    let first = registry.insert(Arc::new(11)).unwrap();
    assert!(first > 0);
    assert_eq!(*registry.acquire(first).unwrap(), 11);
    assert_eq!(*registry.remove(first).unwrap(), 11);
    let second = registry.insert(Arc::new(22)).unwrap();
    assert!(second > first);
    assert!(registry.acquire(first).is_none());
    assert_eq!(*registry.acquire(second).unwrap(), 22);
}

#[test]
fn invalid_and_double_destroy_are_noops() {
    let registry = EngineHandles::default();
    let handle = registry.insert(Arc::new(5)).unwrap();
    assert!(registry.publish(handle));
    for invalid in [0, -1, i64::MIN, i64::MAX] {
        assert!(registry.acquire(invalid).is_none());
        assert!(registry.remove(invalid).is_none());
        assert_eq!(registry.current().unwrap().0, handle);
    }
    assert!(registry.remove(handle).is_some());
    assert!(registry.remove(handle).is_none());
    assert!(registry.current().is_none());
}

#[test]
fn invalid_and_stale_publish_preserve_binding_but_zero_clears_it() {
    let registry = EngineHandles::new();
    assert!(registry.current().is_none());
    assert!(registry.publish(0));
    let old = registry.insert(Arc::new(1)).unwrap();
    let live = registry.insert(Arc::new(2)).unwrap();
    registry.remove(old);
    assert!(registry.publish(live));
    for invalid in [old, -1, i64::MIN, i64::MAX] {
        assert!(!registry.publish(invalid));
        assert_eq!(registry.current().unwrap().0, live);
    }
    assert!(registry.publish(live));
    assert_eq!(*registry.current().unwrap().1, 2);
    assert!(registry.publish(0));
    assert!(registry.current().is_none());
    assert!(registry.acquire(live).is_some());
}

#[test]
fn replacing_a_with_b_then_destroying_a_does_not_detach_b() {
    let registry = EngineHandles::new();
    let a = registry.insert(Arc::new("a")).unwrap();
    let b = registry.insert(Arc::new("b")).unwrap();
    assert!(registry.publish(a));
    assert!(registry.publish(b));
    registry.remove(a);
    let (current, value) = registry.current().unwrap();
    assert_eq!(current, b);
    assert_eq!(*value, "b");
    assert!(!registry.publish(a));
    assert_eq!(registry.current().unwrap().0, b);
}

#[test]
fn registry_owns_map_and_binding_and_drops_each_payload_once() {
    let drops = Arc::new(AtomicUsize::new(0));
    let value = Arc::new(DropSentinel(Arc::clone(&drops)));
    let registry = EngineHandles::new();
    let handle = registry.insert(Arc::clone(&value)).unwrap();
    assert_eq!(Arc::strong_count(&value), 2);
    assert!(registry.publish(handle));
    assert_eq!(Arc::strong_count(&value), 3);
    assert!(registry.publish(handle));
    assert_eq!(Arc::strong_count(&value), 3);
    assert!(registry.publish(0));
    assert_eq!(Arc::strong_count(&value), 2);
    assert!(registry.publish(handle));
    drop(value);
    drop(registry);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn remove_returns_owner_after_revoking_map_and_binding() {
    let drops = Arc::new(AtomicUsize::new(0));
    let registry = EngineHandles::new();
    let handle = registry
        .insert(Arc::new(DropSentinel(Arc::clone(&drops))))
        .unwrap();
    assert!(registry.publish(handle));
    let removed = registry.remove(handle).unwrap();
    assert!(registry.current().is_none());
    assert!(registry.acquire(handle).is_none());
    assert!(!registry.publish(handle));
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert_eq!(Arc::strong_count(&removed), 1);
    drop(removed);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn in_flight_acquire_and_render_leases_survive_destroy() {
    let drops = Arc::new(AtomicUsize::new(0));
    let registry = Arc::new(EngineHandles::new());
    let handle = registry
        .insert(Arc::new(DropSentinel(Arc::clone(&drops))))
        .unwrap();
    assert!(registry.publish(handle));
    let acquired = Arc::new(Barrier::new(2));
    let destroyed = Arc::new(Barrier::new(2));
    let worker = {
        let registry = Arc::clone(&registry);
        let acquired = Arc::clone(&acquired);
        let destroyed = Arc::clone(&destroyed);
        thread::spawn(move || {
            let lease = registry.acquire(handle).unwrap();
            let (render_handle, render_lease) = registry.current().unwrap();
            assert_eq!(render_handle, handle);
            assert!(Arc::ptr_eq(&lease, &render_lease));
            acquired.wait();
            destroyed.wait();
            assert_eq!(lease.0.load(Ordering::SeqCst), 0);
            assert_eq!(Arc::strong_count(&lease), 2);
            drop(render_lease);
            drop(lease);
        })
    };
    acquired.wait();
    drop(registry.remove(handle));
    assert!(registry.current().is_none());
    assert!(registry.acquire(handle).is_none());
    assert!(!registry.publish(handle));
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    destroyed.wait();
    worker.join().unwrap();
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn destroy_before_late_publish_cannot_republish() {
    let registry = Arc::new(EngineHandles::new());
    let handle = registry.insert(Arc::new(1)).unwrap();
    assert!(registry.publish(handle));
    let destroyed = Arc::new(Barrier::new(2));
    let worker = {
        let registry = Arc::clone(&registry);
        let destroyed = Arc::clone(&destroyed);
        thread::spawn(move || {
            destroyed.wait();
            assert!(!registry.publish(handle));
            assert!(registry.current().is_none());
        })
    };
    registry.remove(handle);
    destroyed.wait();
    worker.join().unwrap();
}

#[test]
fn racing_publish_and_destroy_always_finish_unpublished() {
    let registry = Arc::new(EngineHandles::new());
    for _ in 0..256 {
        let handle = registry.insert(Arc::new(1)).unwrap();
        let start = Arc::new(Barrier::new(3));
        thread::scope(|scope| {
            let registry = &registry;
            let start = &start;
            scope.spawn(move || {
                start.wait();
                registry.publish(handle);
            });
            scope.spawn(move || {
                start.wait();
                drop(registry.remove(handle));
            });
            start.wait();
        });
        assert!(registry.current().is_none());
        assert!(registry.acquire(handle).is_none());
        assert!(!registry.publish(handle));
    }
}
