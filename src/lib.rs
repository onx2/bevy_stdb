//! Bevy integration for SpacetimeDB.
//!
//! This crate provides [`plugin::StdbPlugin`] and related types for configuring
//! SpacetimeDB connections in Bevy apps.
//!
//! Most application code should import [`prelude`].
pub(crate) mod channel_bridge;

pub mod alias;
pub mod connection;
pub mod message;
pub mod plugin;
pub mod reconnect;
pub mod subscription;
pub mod table;

/// Common imports for applications using `bevy_stdb`.
///
/// This module re-exports the primary plugin type, connection resources,
/// reconnect options, subscription helpers, and message aliases most apps use
/// directly.
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
