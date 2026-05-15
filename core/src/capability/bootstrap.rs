//! Process-wide default registry + tool probe.
//!
//! Most call sites in the daemon-host path don't have a registry
//! threaded through (yet) — they reach for `default_registry()` and
//! build a `Scope` around their existing `project_path`. The
//! singleton model is intentional: registration is feature-gated at
//! the call site (`#[cfg(feature = "github-cli")]` controls whether
//! `GhCliRemoteProvider` is registered), and the same `ToolProbe`
//! must back every scope so cache invalidation (`RecheckGhAuth`)
//! actually affects what subsequent resolves see.

use std::sync::{Arc, OnceLock};

use crate::scope::{Scope, SystemScope, ToolProbe};

use super::CapabilityRegistry;

static REGISTRY: OnceLock<CapabilityRegistry> = OnceLock::new();
static TOOL_PROBE: OnceLock<Arc<ToolProbe>> = OnceLock::new();

/// Shared `ToolProbe`. Always returns the same `Arc` for the
/// process so `invalidate("gh")` from one site affects everyone.
pub fn default_tool_probe() -> &'static Arc<ToolProbe> {
    TOOL_PROBE.get_or_init(|| Arc::new(ToolProbe::new()))
}

/// Process-wide capability registry. Registers the default impls
/// shipped with this crate (`CliGit` always; `GhCliRemoteProvider`
/// behind the `github-cli` feature).
pub fn default_registry() -> &'static CapabilityRegistry {
    REGISTRY.get_or_init(build_default_registry)
}

fn build_default_registry() -> CapabilityRegistry {
    let reg = CapabilityRegistry::new();
    reg.register::<dyn crate::git::Git>(Arc::new(crate::git::CliGit));
    #[cfg(feature = "github-cli")]
    {
        reg.register::<dyn crate::git_remote::GitRemoteProvider>(Arc::new(
            crate::git_remote::GhCliRemoteProvider,
        ));
    }
    reg
}

/// Build a `Scope::System` rooted at the shared `ToolProbe`. Used by
/// call sites that only need system-level context (e.g. boot-time
/// gh-auth probe).
pub fn system_scope() -> Scope {
    Scope::System(SystemScope::new(default_tool_probe().clone()))
}

/// Build a `Scope::Project` for a given on-disk repo root + project
/// id, using the shared `ToolProbe`. The id is free-form; call
/// sites that don't have one ready (e.g. lookups keyed by branch)
/// pass an empty string.
pub fn project_scope(project_id: impl Into<String>, root: std::path::PathBuf) -> Scope {
    let sys = SystemScope::new(default_tool_probe().clone());
    Scope::Project(crate::scope::ProjectScope::new(sys, project_id.into(), root))
}
