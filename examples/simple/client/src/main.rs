mod module_bindings;

use crate::module_bindings::RemoteModule;
use bevy::{asset::AssetMetaCheck, prelude::*};
use bevy_stdb::prelude::*;
use module_bindings::*;

const MOVE_SPEED: f32 = 200.0;

#[derive(Clone, Eq, Hash, PartialEq, Debug)]
pub enum SubKey {
    Player,
}

pub type StdbConn = StdbConnection<DbConnection>;
pub type StdbCmds<'w, 's> = StdbCommands<'w, 's, DbConnection, RemoteModule>;
pub type StdbSubs = StdbSubscriptions<SubKey, RemoteModule>;

#[derive(Component, Debug, Default)]
pub struct PlayerMarker;

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

        #[cfg(target_arch = "wasm32")]
        let driver = DbConnection::run_background_task;
        #[cfg(not(target_arch = "wasm32"))]
        let driver = DbConnection::run_threaded;

        app.add_plugins(
            StdbPlugin::<DbConnection, RemoteModule>::default()
                .with_uri(String::from("http://localhost:3000"))
                .with_module_name(String::from("bevy-stdb-simple"))
                .with_subscriptions::<SubKey>()
                .add_table::<Player>(|reg, db| reg.bind(db.player()))
                .with_background_driver(driver),
        );

        app.add_systems(Startup, |mut commands: Commands| {
            commands.spawn((Name::new("Camera"), Camera2d));
        });
        app.add_systems(Update, (spawn_player, subscribe_on_connect));
        app.add_systems(
            Update,
            (sync_position, handle_move_request).run_if(resource_exists::<StdbConn>),
        );
        app.add_systems(Startup, request_connect);
    }
}

fn request_connect(mut stdb_cmds: StdbCmds) {
    stdb_cmds.connect(StdbConnectOptions::default());
}

fn subscribe_on_connect(mut msgs: ReadStdbConnectedMessage, mut subs: ResMut<StdbSubs>) {
    for msg in msgs.read() {
        info!("Subscribing to player table");
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
        info!("Spawning in player");
        commands.spawn((
            Name::new("Player"),
            PlayerMarker,
            Mesh2d(meshes.add(Circle::new(20.0))),
            MeshMaterial2d(materials.add(Color::srgb(0.2, 0.4, 1.0))),
            Transform::from_xyz(msg.row.x, msg.row.y, 0.0),
        ));
    }
}

fn sync_position(
    mut player: Single<&mut Transform, With<PlayerMarker>>,
    mut msgs: ReadUpdateMessage<Player>,
) {
    for msg in msgs.read() {
        info!("Updating player position");
        player.translation.x = msg.new.x;
        player.translation.y = msg.new.y;
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

    // rem_euclid wraps negative values correctly
    let new_x = (player.translation.x + step.x + half_w).rem_euclid(window.width()) - half_w;
    let new_y = (player.translation.y + step.y + half_h).rem_euclid(window.height()) - half_h;

    let _ = conn.reducers().move_player(new_x, new_y);
}
