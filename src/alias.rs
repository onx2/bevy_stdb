//! [`MessageReader`] type aliases for connection lifecycle and table messages.

use crate::message::{
    DeleteMessage, InsertMessage, InsertUpdateMessage, StdbConnectErrorMessage,
    StdbConnectedMessage, StdbDisconnectedMessage, StdbSubscriptionAppliedMessage,
    StdbSubscriptionErrorMessage, UpdateMessage,
};
#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
use crate::message::{
    StdbLoginFailedMessage, StdbLoginSucceededMessage, StdbLogoutFailedMessage,
    StdbLogoutSucceededMessage,
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

/// A [`MessageReader`] for [`StdbConnectErrorMessage`].
pub type ReadStdbConnectErrorMessage<'w, 's> = MessageReader<'w, 's, StdbConnectErrorMessage>;

/// A [`MessageReader`] for [`StdbSubscriptionAppliedMessage<K>`].
pub type ReadStdbSubscriptionAppliedMessage<'w, 's, K> =
    MessageReader<'w, 's, StdbSubscriptionAppliedMessage<K>>;

/// A [`MessageReader`] for [`StdbSubscriptionErrorMessage<K>`].
pub type ReadStdbSubscriptionErrorMessage<'w, 's, K> =
    MessageReader<'w, 's, StdbSubscriptionErrorMessage<K>>;

/// A [`MessageReader`] for [`StdbLoginSucceededMessage`].
#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
pub type ReadStdbLoginSucceededMessage<'w, 's> = MessageReader<'w, 's, StdbLoginSucceededMessage>;

/// A [`MessageReader`] for [`StdbLoginFailedMessage`].
#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
pub type ReadStdbLoginFailedMessage<'w, 's> = MessageReader<'w, 's, StdbLoginFailedMessage>;

/// A [`MessageReader`] for [`StdbLogoutSucceededMessage`].
#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
pub type ReadStdbLogoutSucceededMessage<'w, 's> = MessageReader<'w, 's, StdbLogoutSucceededMessage>;

/// A [`MessageReader`] for [`StdbLogoutFailedMessage`].
#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
pub type ReadStdbLogoutFailedMessage<'w, 's> = MessageReader<'w, 's, StdbLogoutFailedMessage>;
