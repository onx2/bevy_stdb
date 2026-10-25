//! Per-row-type control over whether table changes carry the SpacetimeDB context.

use bevy_ecs::prelude::{Resource, World};
use std::any::TypeId;

/// The row types whose table changes carry the SpacetimeDB context.
///
/// The SDK invokes a row callback once per changed row, so carrying the context costs one clone of
/// its generated `Event`, reducer arguments included, per row. A row type carries it only once it
/// is bound with [`TableCapability::ctx`](crate::prelude::TableCapability::ctx).
///
/// Keyed by row type rather than by accessor because one channel carries every accessor over a
/// row type, so a table and a view over the same row must agree.
#[derive(Resource, Default, Clone)]
pub(crate) struct RowCtxPolicy {
    /// Row types that opted in. Read once per bind, never per row.
    carried: Vec<TypeId>,
}

impl RowCtxPolicy {
    /// Records that `row_type` carries the context. Repeat calls are idempotent.
    pub(crate) fn carry(&mut self, row_type: TypeId) {
        if !self.carried.contains(&row_type) {
            self.carried.push(row_type);
        }
    }

    /// Returns whether changes for `TRow` carry the context.
    fn carries_ctx<TRow: 'static>(&self) -> bool {
        self.carried.contains(&TypeId::of::<TRow>())
    }
}

/// Returns whether changes for `TRow` carry the SpacetimeDB context.
///
/// Called once per table bind, when a connection becomes active, not per row.
pub(crate) fn carries_ctx<TRow: 'static>(world: &World) -> bool {
    world
        .get_resource::<RowCtxPolicy>()
        .is_some_and(|policy| policy.carries_ctx::<TRow>())
}

#[cfg(test)]
mod tests {
    use super::RowCtxPolicy;
    use std::any::TypeId;

    struct MonsterRow;
    struct PlayerRow;

    #[test]
    fn a_row_type_carries_no_ctx_until_it_opts_in() {
        let mut policy = RowCtxPolicy::default();
        assert!(!policy.carries_ctx::<MonsterRow>());

        policy.carry(TypeId::of::<MonsterRow>());
        assert!(policy.carries_ctx::<MonsterRow>());
    }

    #[test]
    fn opting_one_row_type_in_leaves_the_others_alone() {
        let mut policy = RowCtxPolicy::default();
        policy.carry(TypeId::of::<MonsterRow>());

        assert!(!policy.carries_ctx::<PlayerRow>());
    }

    #[test]
    fn opting_in_twice_is_idempotent() {
        // A table and a view over one row both resolve to the same row type.
        let mut policy = RowCtxPolicy::default();
        policy.carry(TypeId::of::<MonsterRow>());
        policy.carry(TypeId::of::<MonsterRow>());

        assert_eq!(policy.carried.len(), 1);
        assert!(policy.carries_ctx::<MonsterRow>());
    }
}
