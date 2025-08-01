use crate::{message::ChatMessage, time::DTChatTime};
pub mod simple_vec;

pub enum MarkIntent {
    Acked(DTChatTime),
    Sent(DTChatTime),
    Failed,
}

pub trait ChatDataBase: Send + Sync {
    fn get_last_messages(&self, count: usize) -> Vec<ChatMessage>;
    fn get_messages_filtered(&mut self);
    fn add_message(&mut self, msg: ChatMessage);
    fn mark_as(&mut self, uuid: &String, intent: MarkIntent) -> Option<ChatMessage>;
    fn get_all_messages(&self) -> Vec<ChatMessage>;
}
