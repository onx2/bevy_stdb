mod module_bindings;
mod stdb;

use bevy::{asset::AssetMetaCheck, prelude::*};
use bevy_stdb::prelude::*;
use module_bindings::*;
use stdb::*;

const MOVE_SPEED: f32 = 1_000.0;

#[derive(Component, Debug, Default)]
pub struct PlayerMarker;

#[derive(Component, Debug, Default)]
pub struct NetTransform {
    x: f32,
    y: f32,
}

fn main() -> AppExit {
    App::new().add_plugins(AppPlugin).run()
}

pub struct AppPlugin;
impl Plugin for AppPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    // Wasm builds will check for meta files (that don't exist) if this isn't set.
                    // This causes errors and even panics on web build on itch.
                    // See https://github.com/bevyengine/bevy_github_ci_template/issues/48.
                    meta_check: AssetMetaCheck::Never,
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Window {
                        title: String::from("bevy_stdb simple example"),
                        fit_canvas_to_parent: true,
                        ..default()
                    }
                    .into(),
                    ..default()
                }),
        );

        app.add_plugins(MyStdbPlugin);

        app.add_systems(Startup, (spawn_camera, spawn_helper_text, request_connect));
        app.add_systems(
            Update,
            (
                subscribe_on_connect,
                spawn_player,
                sync_position,
                interpolate,
            )
                .chain(),
        );
        app.add_systems(
            Update,
            handle_move_request.run_if(resource_exists::<StdbConn>),
        );
    }
}

fn spawn_helper_text(mut commands: Commands) {
    commands.spawn((
        Text::new("Use WASD to move."),
        Node {
            position_type: PositionType::Absolute,
            top: px(16),
            left: px(16),
            ..default()
        },
    ));
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((Name::new("Camera"), Camera2d));
}

fn request_connect(mut stdb_cmds: StdbCmds) {
    stdb_cmds.connect(StdbConnectOptions::default());
}

fn subscribe_on_connect(mut msgs: ReadStdbConnectedMessage, mut subs: ResMut<StdbSubs>) {
    for msg in msgs.read() {
        subs.subscribe_query(SubKey::Player, |q| {
            q.from.player().r#where(|p| p.identity.eq(msg.identity))
        });
    }
}

fn spawn_player(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut msgs: ReadInsertMessage<Player>,
) {
    for msg in msgs.read() {
        commands.spawn((
            PlayerMarker,
            Mesh2d(meshes.add(Circle::new(20.0))),
            MeshMaterial2d(materials.add(Color::srgb(0.2, 0.4, 1.0))),
            Transform::from_xyz(msg.row.x, msg.row.y, 0.0),
            NetTransform {
                x: msg.row.x,
                y: msg.row.y,
            },
        ));
    }
}

/// Interpolate the rendered position of the player toward the server authority's position
fn interpolate(
    time: Res<Time>,
    mut player: Single<(&mut Transform, &NetTransform), With<PlayerMarker>>,
    window: Single<&Window>, // Added window to check screen bounds
) {
    let dt = time.delta_secs();
    let (mut transform, net_transform) = player.into_inner();
    let target = Vec3::new(net_transform.x, net_transform.y, transform.translation.z);

    // Calculate how far the target is from our current visual position
    let distance = transform.translation.distance(target);

    // If the distance is larger than half the screen width, we assume the player
    // wrapped around the screen edge (teleported).
    let wrap_threshold = window.width() / 2.0;

    if distance > wrap_threshold {
        // Snap instantly to the new position
        transform.translation = target;
    } else {
        // Otherwise, smoothly interpolate normal movement
        transform.translation.smooth_nudge(&target, 18.0, dt);
    }
}

/// Store the server authority position on the player
fn sync_position(
    mut player: Single<&mut NetTransform, With<PlayerMarker>>,
    mut msgs: ReadUpdateMessage<Player>,
) {
    for msg in msgs.read() {
        player.x = msg.new.x;
        player.y = msg.new.y;
    }
}

fn handle_move_request(
    conn: Res<StdbConn>,
    player: Single<&Transform, With<PlayerMarker>>,
    window: Single<&Window>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
) {
    let mut direction = Vec2::ZERO;

    if keys.pressed(KeyCode::KeyW) {
        direction.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) {
        direction.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyA) {
        direction.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) {
        direction.x += 1.0;
    }

    if direction == Vec2::ZERO {
        return;
    }

    let step = direction.normalize() * MOVE_SPEED * time.delta_secs();
    let half_w = window.width() / 2.0;
    let half_h = window.height() / 2.0;

    let _ = conn.reducers().move_player(
        (player.translation.x + step.x + half_w).rem_euclid(window.width()) - half_w,
        (player.translation.y + step.y + half_h).rem_euclid(window.height()) - half_h,
    );
}

#[cfg(test)]
mod registration_tests {
    use crate::module_bindings::*;
    use bevy::prelude::*;
    use bevy_stdb::prelude::*;

    fn app_with(
        bind: impl FnOnce(
            StdbPlugin<DbConnection, RemoteModule>,
        ) -> StdbPlugin<DbConnection, RemoteModule>,
    ) -> App {
        let mut app = App::new();
        let plugin = StdbPlugin::<DbConnection, RemoteModule>::default()
            .with_uri(String::from("http://localhost:3000"))
            .with_database_name(String::from("bevy-stdb-simple"))
            .with_background_driver(DbConnection::run_threaded);
        app.add_plugins(MinimalPlugins);
        app.add_plugins(bind(plugin));
        app
    }

    /// The channel every bound capability sends into must exist before a connection binds it,
    /// whatever order the capabilities were registered in. `channel_sender` panics otherwise,
    /// and it runs when the connection becomes active rather than at startup.
    fn assert_change_channel_registered(app: &App) {
        let _ = app
            .world()
            .resource::<StdbChannels>()
            .sender::<TableChange<Player>>();
    }

    #[test]
    fn add_table_registers_the_change_channel() {
        assert_change_channel_registered(&app_with(|p| p.add_table::<PlayerTableAccessor>()));
    }

    #[test]
    fn insert_update_before_insert_registers_the_change_channel() {
        // `InsertUpdate` sends no `TableChange` of its own, so claiming it first must not
        // convince a later `Insert` that the channel is already registered.
        assert_change_channel_registered(&app_with(|p| {
            p.bind_insert_update::<PlayerTableAccessor>()
                .bind_insert::<PlayerTableAccessor>()
        }));
    }
}

#[cfg(test)]
mod fanout_tests {
    use crate::module_bindings::*;
    use bevy::ecs::system::RunSystemOnce;
    use bevy::prelude::*;
    use bevy_stdb::prelude::*;
    use spacetimedb_sdk::{Event, Identity};
    use std::sync::Arc;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_plugins(
            StdbPlugin::<DbConnection, RemoteModule>::default()
                .with_uri(String::from("http://localhost:3000"))
                .with_database_name(String::from("bevy-stdb-simple"))
                .with_background_driver(DbConnection::run_threaded)
                .add_table::<PlayerTableAccessor>(),
        );
        app
    }

    fn player(x: f32) -> Player {
        Player { identity: Identity::from_byte_array([0; 32]), online: true, x, y: 0.0 }
    }

    #[test]
    fn one_change_fans_out_to_every_bound_stream() {
        let mut app = app();
        let tx = app
            .world()
            .resource::<StdbChannels>()
            .sender::<TableChange<Player>>();

        tx.send(TableChange::Insert {
            event: Arc::new(Event::SubscribeApplied),
            row: player(1.0),
        })
        .unwrap();
        tx.send(TableChange::Update {
            event: Arc::new(Event::SubscribeApplied),
            old: player(1.0),
            new: player(2.0),
        })
        .unwrap();
        tx.send(TableChange::Delete {
            event: Arc::new(Event::SubscribeApplied),
            row: player(2.0),
        })
        .unwrap();
        app.update();

        let w = app.world_mut();
        let unified = w
            .run_system_once(|mut r: ReadTableChangeMessage<Player>| r.read().count())
            .unwrap();
        let inserts = w
            .run_system_once(|mut r: ReadInsertMessage<Player>| r.read().count())
            .unwrap();
        let updates = w
            .run_system_once(|mut r: ReadUpdateMessage<Player>| r.read().count())
            .unwrap();
        let deletes = w
            .run_system_once(|mut r: ReadDeleteMessage<Player>| r.read().count())
            .unwrap();
        let upserts = w
            .run_system_once(|mut r: ReadInsertUpdateMessage<Player>| r.read().count())
            .unwrap();

        assert_eq!((unified, inserts, updates, deletes, upserts), (3, 1, 1, 1, 2));
    }

    #[test]
    fn the_unified_stream_keeps_callback_order() {
        let mut app = app();
        let tx = app
            .world()
            .resource::<StdbChannels>()
            .sender::<TableChange<Player>>();

        for x in 0..5 {
            let e = Arc::new(Event::SubscribeApplied);
            let _ = tx.send(if x % 2 == 0 {
                TableChange::Insert { event: e, row: player(x as f32) }
            } else {
                TableChange::Delete { event: e, row: player(x as f32) }
            });
        }
        app.update();

        let seen = app
            .world_mut()
            .run_system_once(|mut r: ReadTableChangeMessage<Player>| {
                r.read()
                    .map(|c| match c {
                        TableChange::Insert { row, .. } => ('i', row.x),
                        TableChange::Delete { row, .. } => ('d', row.x),
                        TableChange::Update { new, .. } => ('u', new.x),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap();

        assert_eq!(
            seen,
            vec![('i', 0.0), ('d', 1.0), ('i', 2.0), ('d', 3.0), ('i', 4.0)]
        );
    }

    #[test]
    fn the_event_is_shared_not_cloned_per_stream() {
        let mut app = app();
        let tx = app
            .world()
            .resource::<StdbChannels>()
            .sender::<TableChange<Player>>();
        let event = Arc::new(Event::SubscribeApplied);

        tx.send(TableChange::Insert { event: Arc::clone(&event), row: player(1.0) })
            .unwrap();
        app.update();

        // held here, plus TableChange, InsertMessage and InsertUpdateMessage
        assert_eq!(Arc::strong_count(&event), 4);
    }
}
