pub(crate) mod channel_bridge;

pub mod alias;
pub mod connection;
pub mod message;
pub mod plugin;
pub mod reconnect;
pub mod subscription;
pub mod table;

pub mod prelude {
    #[doc(hidden)]
    pub use crate::{
        alias::{
            ReadDeleteMessage, ReadInsertMessage, ReadInsertUpdateMessage,
            ReadStdbConnectedMessage, ReadStdbConnectionErrorMessage, ReadStdbDisconnectedMessage,
            ReadUpdateMessage,
        },
        connection::{StdbConnection, StdbConnectionState},
        message::{
            DeleteMessage, InsertMessage, InsertUpdateMessage, StdbConnectedMessage,
            StdbConnectionErrorMessage, StdbDisconnectedMessage, UpdateMessage,
        },
        plugin::StdbPlugin,
        reconnect::StdbReconnectOptions,
        subscription::StdbSubscriptions,
    };
}
