use crate::message::ChatMessage;
pub use socket_engine::event::{ConnectionEvent, DataEvent, ErrorEvent};

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
    AckSent(ChatMessage, String),
    AckReceived(ChatMessage),
}

#[derive(Clone, Debug)]
pub enum NetworkEvent {
    Data(DataEvent),
    Connection(ConnectionEvent),
}

#[derive(Clone, Debug)]
pub enum NetworkErrorEvent {
    SocketError(ErrorEvent),
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
