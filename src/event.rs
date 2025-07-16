use crate::message::ChatMessage;
use socket_engine::event::{ErrorEventSocket, EventSocket};

#[derive(Clone, Debug)]
pub enum ChatAppEvent {
    Info(ChatAppInfoEvent),
    Error(ChatAppErrorEvent),
    SocketEngineInfo(NetworkEvent),
    SocketEngineError(NetworkErrorEvent),
}

#[derive(Clone, Debug)]
pub enum ChatAppInfoEvent {
    Sending(ChatMessage),
    Sent(ChatMessage),
    Received(ChatMessage),
    AckSent(String, String),
    AckReceived(String, String),
}

#[derive(Clone, Debug)]
pub enum NetworkEvent {
    Socket(EventSocket),
}

#[derive(Clone, Debug)]
pub enum NetworkErrorEvent {
    SocketError(ErrorEventSocket),
}

#[derive(Clone, Debug)]
pub enum ChatAppErrorEvent {
    ProtocolDecode(String),
    ProtocolEncode(String),
    InvalidMessage(String),
    MessageNotFound(String),
    PeerNotFound(String),
    NoEngineAttached,
    InternalError(String),
}

pub trait AppEventObserver: Send + Sync {
    fn on_event(&mut self, event: ChatAppEvent);
}
