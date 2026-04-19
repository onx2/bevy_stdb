//! [`MessageReader`] type aliases for connection lifecycle and table messages.
use crate::message::{
    AuthFailureMessage, AuthSuccessMessage, DeleteMessage, InsertMessage, InsertUpdateMessage,
    RequestLoginMessage, RequestLogoutMessage, StdbConnectedMessage, StdbConnectionErrorMessage,
    StdbDisconnectedMessage, StdbSubscriptionAppliedMessage, StdbSubscriptionErrorMessage,
    UpdateMessage,
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

/// A [`MessageReader`] for [`RequestLoginMessage`].
pub type ReadRequestLoginMessage<'w, 's> = MessageReader<'w, 's, RequestLoginMessage>;

/// A [`MessageReader`] for [`RequestLogoutMessage`].
pub type ReadRequestLogoutMessage<'w, 's> = MessageReader<'w, 's, RequestLogoutMessage>;

/// A [`MessageReader`] for [`AuthSuccessMessage`].
pub type ReadAuthSuccessMessage<'w, 's> = MessageReader<'w, 's, AuthSuccessMessage>;

/// A [`MessageReader`] for [`AuthFailureMessage`].
pub type ReadAuthFailureMessage<'w, 's> = MessageReader<'w, 's, AuthFailureMessage>;

/// A [`MessageReader`] for [`StdbSubscriptionAppliedMessage<K>`].
pub type ReadStdbSubscriptionAppliedMessage<'w, 's, K> =
    MessageReader<'w, 's, StdbSubscriptionAppliedMessage<K>>;

/// A [`MessageReader`] for [`StdbSubscriptionErrorMessage<K>`].
pub type ReadStdbSubscriptionErrorMessage<'w, 's, K> =
    MessageReader<'w, 's, StdbSubscriptionErrorMessage<K>>;
