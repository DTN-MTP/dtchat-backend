use std::collections::HashMap;

use crate::{
    dtchat::{Peer, Room},
    message::ChatMessage,
    time::DTChatTime,
};
pub mod simple_vec;

pub enum MarkIntent {
    Acked(DTChatTime),
    Sent(DTChatTime),
    Failed,
}

pub trait ChatDataBase: Send + Sync {
    fn get_rooms(&self) -> &HashMap<String, Room>;
    // Peers
    fn get_other_peers(&self) -> &HashMap<String, Peer>;
    fn get_localpeer(&self) -> &Peer;
    // Messages
    fn get_last_messages(&self, count: usize) -> &[ChatMessage];
    fn get_all_messages(&self) -> &Vec<ChatMessage>;
    fn add_message(&mut self, msg: ChatMessage) -> bool;
    fn mark_as(&mut self, uuid: &String, intent: MarkIntent) -> Option<ChatMessage>;
}
