use crate::message::ChatMessage;

#[derive(Clone)]
pub enum AppEvent {
    Error(String),
    Sending(ChatMessage),
    Sent(ChatMessage),
    Received(ChatMessage),
    Acked(ChatMessage),
}
pub trait AppEventObserver: Send + Sync {
    fn on_event(&mut self, event: AppEvent);
}
