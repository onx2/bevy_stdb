//! Per-row-type control over whether table messages carry the SDK row event.

use bevy_ecs::prelude::{Resource, World};
use std::any::TypeId;

/// The row types whose table messages carry the SDK event.
///
/// The SDK invokes a row callback once per changed row, so carrying the event costs one clone of
/// a generated `Event` per row -- and that event holds the reducer that caused the change,
/// arguments included. Most readers only want the row, so a row type carries the event only once
/// it opts in with [`StdbPlugin::with_event`](crate::prelude::StdbPlugin::with_event).
///
/// Keyed by row type rather than by accessor because one channel carries every accessor over a
/// row type, so a table and a view over the same row must agree.
#[derive(Resource, Default, Clone)]
pub(crate) struct RowEventPolicy {
    /// Row types that opted in. Read once per bind, never per row.
    carried: Vec<TypeId>,
}

impl RowEventPolicy {
    /// Records that `row_type` carries the event. Repeat calls are idempotent.
    pub(crate) fn carry(&mut self, row_type: TypeId) {
        if !self.carried.contains(&row_type) {
            self.carried.push(row_type);
        }
    }

    /// Returns whether messages for `TRow` carry the event.
    fn carries_event<TRow: 'static>(&self) -> bool {
        self.carried.contains(&TypeId::of::<TRow>())
    }
}

/// Returns whether messages for `TRow` carry the SDK event.
///
/// Called once per table bind -- when a connection becomes active -- not per row.
pub(crate) fn carries_event<TRow: 'static>(world: &World) -> bool {
    world
        .get_resource::<RowEventPolicy>()
        .is_some_and(|policy| policy.carries_event::<TRow>())
}

#[cfg(test)]
mod tests {
    use super::RowEventPolicy;
    use std::any::TypeId;

    struct MonsterRow;
    struct PlayerRow;

    #[test]
    fn a_row_type_carries_no_event_until_it_opts_in() {
        let mut policy = RowEventPolicy::default();
        assert!(!policy.carries_event::<MonsterRow>());

        policy.carry(TypeId::of::<MonsterRow>());
        assert!(policy.carries_event::<MonsterRow>());
    }

    #[test]
    fn opting_one_row_type_in_leaves_the_others_alone() {
        let mut policy = RowEventPolicy::default();
        policy.carry(TypeId::of::<MonsterRow>());

        assert!(!policy.carries_event::<PlayerRow>());
    }

    #[test]
    fn opting_in_twice_is_idempotent() {
        // `with_event` is reachable once per accessor, and a table and a view over one row
        // both resolve to the same row type.
        let mut policy = RowEventPolicy::default();
        policy.carry(TypeId::of::<MonsterRow>());
        policy.carry(TypeId::of::<MonsterRow>());

        assert_eq!(policy.carried.len(), 1);
        assert!(policy.carries_event::<MonsterRow>());
    }
}
