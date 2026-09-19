//! Per-row-type control over whether table messages carry the SDK row event.

use bevy_ecs::prelude::{Resource, World};
use std::any::TypeId;

/// The row types whose table messages omit the SDK event.
///
/// The SDK invokes a row callback once per changed row, so carrying the event costs one clone of
/// a generated `Event` per row -- and that event holds the reducer that caused the change,
/// arguments included. A row type nobody reads the event for opts out with
/// [`StdbPlugin::without_event`](crate::prelude::StdbPlugin::without_event), and its callbacks
/// skip the clone.
///
/// Keyed by row type rather than by accessor because one channel carries every accessor over a
/// row type, so a table and a view over the same row must agree.
#[derive(Resource, Default, Clone)]
pub(crate) struct RowEventPolicy {
    /// Row types that opted out. Read once per bind, never per row.
    omitted: Vec<TypeId>,
}

impl RowEventPolicy {
    /// Records that `row_type` omits the event. Repeat calls are idempotent.
    pub(crate) fn omit(&mut self, row_type: TypeId) {
        if !self.omitted.contains(&row_type) {
            self.omitted.push(row_type);
        }
    }

    /// Returns whether messages for `TRow` carry the event.
    fn carries_event<TRow: 'static>(&self) -> bool {
        !self.omitted.contains(&TypeId::of::<TRow>())
    }
}

/// Returns whether messages for `TRow` carry the SDK event.
///
/// Called once per table bind -- when a connection becomes active -- not per row.
pub(crate) fn carries_event<TRow: 'static>(world: &World) -> bool {
    world
        .get_resource::<RowEventPolicy>()
        .is_none_or(|policy| policy.carries_event::<TRow>())
}

#[cfg(test)]
mod tests {
    use super::RowEventPolicy;
    use std::any::TypeId;

    struct MonsterRow;
    struct PlayerRow;

    #[test]
    fn a_row_type_carries_its_event_until_it_opts_out() {
        let mut policy = RowEventPolicy::default();
        assert!(policy.carries_event::<MonsterRow>());

        policy.omit(TypeId::of::<MonsterRow>());
        assert!(!policy.carries_event::<MonsterRow>());
    }

    #[test]
    fn opting_one_row_type_out_leaves_the_others_alone() {
        let mut policy = RowEventPolicy::default();
        policy.omit(TypeId::of::<MonsterRow>());

        assert!(policy.carries_event::<PlayerRow>());
    }

    #[test]
    fn opting_out_twice_is_idempotent() {
        // `without_event` is reachable once per accessor, and a table and a view over one row
        // both resolve to the same row type.
        let mut policy = RowEventPolicy::default();
        policy.omit(TypeId::of::<MonsterRow>());
        policy.omit(TypeId::of::<MonsterRow>());

        assert_eq!(policy.omitted.len(), 1);
        assert!(!policy.carries_event::<MonsterRow>());
    }
}
