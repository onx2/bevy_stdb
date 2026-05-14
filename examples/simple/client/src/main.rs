mod module_bindings;
mod stdb;

use bevy::{asset::AssetMetaCheck, prelude::*};
use bevy_stdb::prelude::*;
use module_bindings::*;
use stdb::*;

const MOVE_SPEED: f32 = 200.0;

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

        app.add_plugins(MyStdbPlugin);

        app.add_systems(Startup, (spawn_camera, request_connect));
        app.add_systems(
            Update,
            (subscribe_on_connect, spawn_player, sync_position).chain(),
        );
        app.add_systems(
            Update,
            handle_move_request.run_if(resource_exists::<StdbConn>),
        );
    }
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

    let _ = conn.reducers().move_player(
        (player.translation.x + step.x + half_w).rem_euclid(window.width()) - half_w,
        (player.translation.y + step.y + half_h).rem_euclid(window.height()) - half_h,
    );
}
