//! Bevy integration for SpacetimeDB.
//!
//! See [`StdbPlugin`](crate::prelude::StdbPlugin) for configuration and setup.
pub(crate) mod channel_bridge;

mod alias;
mod connection;
mod message;
mod plugin;
mod reconnect;
mod subscription;
mod table;

/// Common imports for `bevy_stdb`.
pub mod prelude {
    pub use crate::{
        alias::{
            ReadDeleteMessage, ReadInsertMessage, ReadInsertUpdateMessage,
            ReadStdbConnectedMessage, ReadStdbConnectionErrorMessage, ReadStdbDisconnectedMessage,
            ReadUpdateMessage,
        },
        connection::{StdbConnection, StdbConnectionController, StdbConnectionState},
        message::{
            DeleteMessage, InsertMessage, InsertUpdateMessage, StdbConnectedMessage,
            StdbConnectionErrorMessage, StdbDisconnectedMessage, UpdateMessage,
        },
        plugin::StdbPlugin,
        reconnect::StdbReconnectOptions,
        subscription::StdbSubscriptions,
    };
}
