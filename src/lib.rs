pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/proto.rs"));
}

pub mod db;
pub mod dtchat;
pub mod event;
pub mod message;
pub mod prediction;
pub mod proto_message;
