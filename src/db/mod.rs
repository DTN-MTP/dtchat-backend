use chrono::{DateTime, Utc};

use crate::message::ChatMessage;
pub mod simple_vec;

pub enum MarkIntent {
    Acked(DateTime<Utc>),
    Sent(DateTime<Utc>),
    Failed,
}

pub trait ChatDataBase: Send + Sync {
    fn get_messages_filtered(&mut self);
    fn add_message(&mut self, msg: ChatMessage);
    fn mark_as(&mut self, uuid: &String, intent: MarkIntent) -> Option<ChatMessage>;
}
