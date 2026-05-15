//! Capability registry.
//!
//! A *capability* is a domain trait (`Git`, `GitRemoteProvider`, …)
//! that 0+ implementations can register for. At call time, a caller
//! asks the registry to `resolve::<dyn Trait>(&scope)` and gets a
//! `Vec<Arc<dyn Trait>>` of every registered impl whose `applies`
//! predicate accepts the caller's scope.
//!
//! Three properties make this useful:
//!
//! 1. **0+ multiplicity.** Zero matches is a normal state surfaced
//!    to callers (UI hides the affordance, returns "not available").
//!    Multiple matches is also normal: today only one
//!    `GitRemoteProvider` impl ships (GitHub via `gh`), but a future
//!    Bitbucket impl coexists without changing call sites.
//! 2. **Multi-input dispatch.** `applies(&scope)` runs against the
//!    full typed scope the call site supplied. Capability impls can
//!    look at platform, tool availability, project root, remote URL
//!    (via another capability the caller already resolved) — any
//!    factual context the scope carries.
//! 3. **One trait, one concern.** The registry forces traits to be
//!    object-safe and self-contained: an impl that needs Git's
//!    output to decide what to do reads it from the caller's earlier
//!    `resolve` result, not from a parent trait. (See
//!    `crate::git_remote`: `matches_remote(remote_url)` takes the
//!    URL as a parameter rather than reaching into a `Git`
//!    capability.)

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::scope::Scope;

mod bootstrap;

pub use bootstrap::{default_registry, default_tool_probe, project_scope, system_scope};

/// Behaviour every capability impl must provide. Capability traits
/// (e.g. `Git`, `GitRemoteProvider`) extend this so registry-side
/// filtering is uniform.
pub trait CapabilityImpl {
    /// Whether this impl is available + applicable for `scope`.
    /// `false` excludes the impl from `resolve` results.
    fn applies(&self, scope: &Scope) -> bool;
}

pub struct CapabilityRegistry {
    inner: RwLock<HashMap<TypeId, Vec<Box<dyn Any + Send + Sync>>>>,
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }

    /// Register `imp` under capability `T` (typically `dyn SomeTrait`).
    pub fn register<T>(&self, imp: Arc<T>)
    where
        T: ?Sized + CapabilityImpl + Send + Sync + 'static,
    {
        let mut w = self.inner.write().unwrap();
        w.entry(TypeId::of::<Arc<T>>())
            .or_default()
            .push(Box::new(imp));
    }

    /// Return every registered impl of capability `T` whose
    /// `applies` predicate accepts `scope`. Empty `Vec` is a
    /// first-class normal state — callers should branch on it
    /// rather than treat it as an error.
    pub fn resolve<T>(&self, scope: &Scope) -> Vec<Arc<T>>
    where
        T: ?Sized + CapabilityImpl + Send + Sync + 'static,
    {
        let r = self.inner.read().unwrap();
        r.get(&TypeId::of::<Arc<T>>())
            .into_iter()
            .flat_map(|v| v.iter())
            .filter_map(|any| any.downcast_ref::<Arc<T>>().cloned())
            .filter(|imp| imp.applies(scope))
            .collect()
    }

    /// Test-only convenience: count registered impls of `T` without
    /// applying `applies` (lets tests separate "registered" from
    /// "applicable in scope").
    #[cfg(test)]
    pub fn count<T>(&self) -> usize
    where
        T: ?Sized + 'static,
    {
        let r = self.inner.read().unwrap();
        r.get(&TypeId::of::<Arc<T>>()).map(|v| v.len()).unwrap_or(0)
    }
}

impl Default for CapabilityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::{Scope, SystemScope, ToolProbe};

    trait Greeter: CapabilityImpl + Send + Sync {
        fn greet(&self) -> &'static str;
    }

    struct AlwaysHi;
    impl CapabilityImpl for AlwaysHi {
        fn applies(&self, _: &Scope) -> bool {
            true
        }
    }
    impl Greeter for AlwaysHi {
        fn greet(&self) -> &'static str {
            "hi"
        }
    }

    struct NeverApplies;
    impl CapabilityImpl for NeverApplies {
        fn applies(&self, _: &Scope) -> bool {
            false
        }
    }
    impl Greeter for NeverApplies {
        fn greet(&self) -> &'static str {
            "never"
        }
    }

    fn dummy_scope() -> Scope {
        let probe = Arc::new(ToolProbe::new());
        let sys = SystemScope::new(probe);
        Scope::System(sys)
    }

    #[test]
    fn zero_impls_resolves_to_empty_vec() {
        let reg = CapabilityRegistry::new();
        let scope = dummy_scope();
        let v = reg.resolve::<dyn Greeter>(&scope);
        assert!(v.is_empty());
    }

    #[test]
    fn one_impl_resolves_when_applies() {
        let reg = CapabilityRegistry::new();
        reg.register::<dyn Greeter>(Arc::new(AlwaysHi));
        let scope = dummy_scope();
        let v = reg.resolve::<dyn Greeter>(&scope);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].greet(), "hi");
    }

    #[test]
    fn applies_false_filters_impl_out() {
        let reg = CapabilityRegistry::new();
        reg.register::<dyn Greeter>(Arc::new(NeverApplies));
        let scope = dummy_scope();
        assert!(reg.resolve::<dyn Greeter>(&scope).is_empty());
        // But it *is* registered:
        assert_eq!(reg.count::<dyn Greeter>(), 1);
    }

    #[test]
    fn multiple_impls_coexist() {
        let reg = CapabilityRegistry::new();
        reg.register::<dyn Greeter>(Arc::new(AlwaysHi));
        reg.register::<dyn Greeter>(Arc::new(AlwaysHi));
        let scope = dummy_scope();
        let v = reg.resolve::<dyn Greeter>(&scope);
        assert_eq!(v.len(), 2);
    }
}
