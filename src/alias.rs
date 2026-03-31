//! Convenience aliases for reading `bevy_stdb` [`Message`](bevy_ecs::prelude::Message)s.
//!
//! This module groups the [`MessageReader`] aliases used for connection lifecycle
//! messages and table update messages so system signatures can stay concise.

use crate::message::{
    DeleteMessage, InsertMessage, InsertUpdateMessage, StdbConnectedMessage,
    StdbConnectionErrorMessage, StdbDisconnectedMessage, UpdateMessage,
};
use bevy_ecs::prelude::MessageReader;

/// A [`MessageReader`] for [`InsertMessage<T>`].
pub type ReadInsertMessage<'w, 's, T> = MessageReader<'w, 's, InsertMessage<T>>;

/// A [`MessageReader`] for [`UpdateMessage<T>`].
pub type ReadUpdateMessage<'w, 's, T> = MessageReader<'w, 's, UpdateMessage<T>>;

/// A [`MessageReader`] for [`DeleteMessage<T>`].
pub type ReadDeleteMessage<'w, 's, T> = MessageReader<'w, 's, DeleteMessage<T>>;

/// A [`MessageReader`] for [`InsertUpdateMessage<T>`].
pub type ReadInsertUpdateMessage<'w, 's, T> = MessageReader<'w, 's, InsertUpdateMessage<T>>;

/// A [`MessageReader`] for [`StdbConnectedMessage`].
pub type ReadStdbConnectedMessage<'w, 's> = MessageReader<'w, 's, StdbConnectedMessage>;

/// A [`MessageReader`] for [`StdbDisconnectedMessage`].
pub type ReadStdbDisconnectedMessage<'w, 's> = MessageReader<'w, 's, StdbDisconnectedMessage>;

/// A [`MessageReader`] for [`StdbConnectionErrorMessage`].
pub type ReadStdbConnectionErrorMessage<'w, 's> = MessageReader<'w, 's, StdbConnectionErrorMessage>;
