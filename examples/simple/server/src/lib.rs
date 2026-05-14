use spacetimedb::*;

#[spacetimedb::table(accessor = player, public)]
pub struct Player {
    #[primary_key]
    pub identity: Identity,
    pub online: bool,
    pub x: f32,
    pub y: f32,
}

#[spacetimedb::reducer(client_connected)]
pub fn identity_connected(ctx: &ReducerContext) {
    if let Some(mut player) = ctx.db.player().identity().find(ctx.sender()) {
        player.online = true;
        ctx.db.player().identity().update(player);
    } else {
        ctx.db.player().insert(Player {
            identity: ctx.sender(),
            online: true,
            x: 0.0,
            y: 0.0,
        });
    }
}

#[spacetimedb::reducer(client_disconnected)]
pub fn identity_disconnected(ctx: &ReducerContext) {
    if let Some(mut player) = ctx.db.player().identity().find(ctx.sender()) {
        player.online = false;
        ctx.db.player().identity().update(player);
    }
}

#[reducer]
pub fn move_player(ctx: &ReducerContext, x: f32, y: f32) {
    if let Some(mut player) = ctx.db.player().identity().find(ctx.sender()) {
        player.x = x;
        player.y = y;
        ctx.db.player().identity().update(player);
    }
}
