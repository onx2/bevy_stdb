//! Bevy integration for SpacetimeDB.
//!
//! This crate provides [`StdbPlugin`] and related types for configuring
//! SpacetimeDB connections in Bevy apps.
//!
//! Most application code should import [`prelude`].
pub(crate) mod channel_bridge;

mod alias;
mod connection;
mod message;
mod plugin;
mod reconnect;
mod subscription;
mod table;

/// Common imports for `bevy_stdb`.
///
/// This module re-exports the types most applications use directly.
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
