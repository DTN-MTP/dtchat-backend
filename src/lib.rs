pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/proto.rs"));
}

pub mod config;
pub mod db;
pub mod dtchat;
pub mod event;
pub mod message;
pub mod prediction;
pub mod proto_message;
pub mod time;

pub use socket_engine::{
    endpoint::{Endpoint, EndpointProto},
    engine::Engine,
};
