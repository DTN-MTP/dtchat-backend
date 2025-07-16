use std::sync::{Arc, Mutex};

use chrono::Utc;
use socket_engine::{
    endpoint::Endpoint,
    engine::Engine,
    event::{
        EngineObserver, ErrorEventSocket, EventSocket, GeneralSocketEvent, SocketEngineEvent,
        TcpErrorEvent, TcpEvent, UdpEvent,
    },
};
use uuid::Uuid;

use crate::{
    event::{
        AppEventObserver, ChatAppErrorEvent, ChatAppEvent, ChatAppInfoEvent, NetworkErrorEvent,
        NetworkEvent,
    },
    message::{ChatMessage, MessageStatus, SortStrategy},
    proto::{proto_message::MsgType, ProtoMessage},
};

pub fn generate_uuid() -> String {
    Uuid::new_v4().to_string()
}

#[derive(Clone)]
pub struct Peer {
    pub uuid: String,
    pub name: String,
    pub endpoints: Vec<Endpoint>,
}

#[derive(PartialEq, Eq)]
enum MessageType {
    Ack,
    Text,
}

pub struct ChatModel {
    pub sort_strategy: SortStrategy,
    pub localpeer: Peer,
    pub peers: Vec<Peer>,
    pub messages: Vec<ChatMessage>,
    observers: Vec<Arc<Mutex<dyn AppEventObserver>>>,
    network_engine: Option<Engine>,
    pending_send_list: Vec<(MessageType, String, Option<String>)>, // msg_type, uuid, original_msg_id pour ACK
}

impl EngineObserver for ChatModel {
    fn on_engine_event(&mut self, event: SocketEngineEvent) {
        match event {
            SocketEngineEvent::Info(socket_event) => match socket_event {
                EventSocket::General(general_info) => match general_info {
                    GeneralSocketEvent::DataReceived(bytes, source_endpoint) => {
                        // Retransmettre l'événement réseau aux observateurs
                        self.notify_observers(ChatAppEvent::SocketEngineInfo(
                            NetworkEvent::Socket(EventSocket::General(
                                GeneralSocketEvent::DataReceived(bytes.clone(), source_endpoint.clone()),
                            )),
                        ));

                        let decode_res = ProtoMessage::decode_from_vec(bytes);

                        match decode_res {
                            Ok(proto_msg) => {
                                self.treat_proto_message(proto_msg, source_endpoint);
                            }
                            Err(decode_err) => {
                                self.notify_observers(ChatAppEvent::Error(
                                    ChatAppErrorEvent::ProtocolDecode(format!(
                                        "Protobuf decode error: {}",
                                        decode_err
                                    )),
                                ));
                            }
                        };
                    }
                    GeneralSocketEvent::DataSent(uuid, endpoint) => {
                        let uuid_clone = uuid.clone();
                        self.notify_observers(ChatAppEvent::SocketEngineInfo(
                            NetworkEvent::Socket(EventSocket::General(
                                GeneralSocketEvent::DataSent(uuid, endpoint),
                            )),
                        ));

                        self.mark_as_sent(&uuid_clone);
                    }
                },
                EventSocket::Tcp(tcp_event) => match tcp_event {
                    TcpEvent::ListenerStarted(addr) => {
                        self.notify_observers(ChatAppEvent::SocketEngineInfo(
                            NetworkEvent::Socket(EventSocket::Tcp(TcpEvent::ListenerStarted(
                                addr,
                            ))),
                        ));
                    }
                    TcpEvent::ConnectionEstablished(addr) => {
                        self.notify_observers(ChatAppEvent::SocketEngineInfo(
                            NetworkEvent::Socket(EventSocket::Tcp(TcpEvent::ConnectionEstablished(
                                addr,
                            ))),
                        ));
                    }
                    TcpEvent::ConnectionClosed(addr) => {
                        self.notify_observers(ChatAppEvent::SocketEngineInfo(
                            NetworkEvent::Socket(EventSocket::Tcp(TcpEvent::ConnectionClosed(
                                addr,
                            ))),
                        ));
                    }
                },
                EventSocket::Udp(udp_event) => match udp_event {
                    UdpEvent::PacketSizeSent(size) => {
                        self.notify_observers(ChatAppEvent::SocketEngineInfo(
                            NetworkEvent::Socket(EventSocket::Udp(UdpEvent::PacketSizeSent(size))),
                        ));
                    }
                    UdpEvent::PacketSizeReceived(size) => {
                        self.notify_observers(ChatAppEvent::SocketEngineInfo(NetworkEvent::Socket(
                            EventSocket::Udp(UdpEvent::PacketSizeReceived(size)),
                        )))
                    }
                },
            },
            SocketEngineEvent::Error(error_event) => match error_event {
                ErrorEventSocket::General(general_error) => {
                    self.notify_observers(ChatAppEvent::SocketEngineError(
                        NetworkErrorEvent::SocketError(ErrorEventSocket::General(general_error)),
                    ));
                }
                ErrorEventSocket::Tcp(tcp_error) => match tcp_error {
                    TcpErrorEvent::ConnectionRefused(addr, endpoint) => {
                        self.notify_observers(ChatAppEvent::SocketEngineError(
                            NetworkErrorEvent::SocketError(ErrorEventSocket::Tcp(
                                TcpErrorEvent::ConnectionRefused(addr, endpoint),
                            )),
                        ));
                    }
                    TcpErrorEvent::ConnectionTimeout(addr, endpoint) => {
                        self.notify_observers(ChatAppEvent::SocketEngineError(
                            NetworkErrorEvent::SocketError(ErrorEventSocket::Tcp(
                                TcpErrorEvent::ConnectionTimeout(addr, endpoint),
                            )),
                        ));
                    }
                },
            },
        }
    }
}

impl ChatModel {


    pub fn new(localpeer: Peer, peers: Vec<Peer>) -> Self {
        Self {
            // TODO: have an SQL(ite) db.rs
            sort_strategy: SortStrategy::Standard,
            localpeer,
            peers,
            messages: Vec::new(),
            observers: Vec::new(),
            network_engine: None,
            pending_send_list: Vec::new(),
        }
    }
    pub fn start(&mut self, engine: Engine) {
        self.network_engine = Some(engine);
        if let Some(eng) = &self.network_engine {
            for endpoint in &self.localpeer.endpoints {
                eng.start_listener_async(endpoint.clone());
            }
        }
    }

    pub fn treat_proto_message(&mut self, proto_msg: ProtoMessage, _source_endpoint: Endpoint) {
        match &proto_msg.msg_type {
            Some(MsgType::Text(text_part)) => {
                let chat_msg = ChatMessage::new_received(&proto_msg, &text_part);
                if let Some(msg) = chat_msg {
                    self.add_message(msg.clone());
                    self.send_ack_to_peer(&msg);
                }
            }
            Some(MsgType::Ack(ack)) => {
                self.apply_ack(&ack.message_uuid);
            }
            None => self.notify_observers(ChatAppEvent::Error(ChatAppErrorEvent::ProtocolDecode(
                "Received proto message with unknown type".to_string(),
            ))),
        }
    }

    pub fn add_observer(&mut self, obs: Arc<Mutex<dyn AppEventObserver>>) {
        self.observers.push(obs);
    }

    pub fn notify_observers(&self, event: ChatAppEvent) {
        for obs in &self.observers {
            obs.lock().unwrap().on_event(event.clone());
        }
    }

    pub fn send_to_peer(&mut self, text: &String, room: &String, endpoint: &Endpoint) {
        let chatmsg = ChatMessage::new_to_send(&self.localpeer.uuid, room, text);
        let sending_uuid = chatmsg.uuid.clone();
        self.pending_send_list
            .push((MessageType::Text, sending_uuid.clone(), None));
        self.add_message(chatmsg.clone());

        if let Some(engine) = &mut self.network_engine {
            let bytes_res = ProtoMessage::new_text(&chatmsg).encode_to_vec();
            match bytes_res {
                Ok(bytes) => {
                    let _ = engine.send_async(endpoint.clone(), bytes, sending_uuid);

                }
                Err(err) => {
                    self.notify_observers(ChatAppEvent::Error(ChatAppErrorEvent::ProtocolEncode(
                        format!("Failed to encode message: {}", err),
                    )));
                }
            }
        }
    }

    pub fn send_ack_to_peer(&mut self, for_msg: &ChatMessage) {
        let peer = self
            .peers
            .iter()
            .find(|peer| peer.uuid == for_msg.sender_uuid)
            .unwrap();
        let proto_msg = ProtoMessage::new_ack(
            for_msg,
            self.localpeer.uuid.clone(),
            Utc::now().timestamp_millis(),
        );
        self.pending_send_list.push((
            MessageType::Ack,
            proto_msg.uuid.clone(),
            Some(for_msg.uuid.clone()),
        ));
        if let Some(engine) = &mut self.network_engine {
            match proto_msg.encode_to_vec() {
                Ok(bytes) => {
                    let _ =
                        engine.send_async(peer.endpoints[0].clone(), bytes, proto_msg.uuid.clone());
                    self.notify_observers(ChatAppEvent::Info(ChatAppInfoEvent::AckSent(
                        proto_msg.uuid.clone(),
                        peer.uuid.clone(),
                    )));
                }
                Err(err) => {
                    self.notify_observers(ChatAppEvent::Error(ChatAppErrorEvent::ProtocolEncode(
                        format!("Failed to encode ACK: {}", err),
                    )));
                }
            };
        }
    }

    fn add_message(&mut self, mut new_msg: ChatMessage) {
        // TODO: push in DB instead
        // Si le message est reçu (pas envoyé par nous), forcer le status à Acknowledged
        if self.localpeer.uuid != new_msg.sender_uuid {
            new_msg.status = MessageStatus::Acknowledged;
        }
        self.messages.push(new_msg.clone());

        let event = if self.localpeer.uuid == new_msg.sender_uuid {
            ChatAppEvent::Info(ChatAppInfoEvent::Sending(new_msg.clone()))
        } else {
            ChatAppEvent::Info(ChatAppInfoEvent::Received(new_msg.clone()))
        };
        self.notify_observers(event);
    }

    fn apply_ack(&mut self, message_uuid: &String) {
        let mut result = None;
        let datetime = Utc::now();
        for message in &mut self.messages {
            if message.uuid == *message_uuid {
                message.receive_time = Some(datetime);
                result = Some(message.clone());
            }
        }

        if let Some(ref msg) = result {
            self.notify_observers(ChatAppEvent::Info(ChatAppInfoEvent::AckReceived(
                msg.uuid.clone(),
                msg.sender_uuid.clone(),
            )));
        }

        if result.is_none() {
            self.notify_observers(ChatAppEvent::Error(ChatAppErrorEvent::MessageNotFound(
                format!("Received ack for unknown message: {}", message_uuid),
            )));
        }
    }

    pub fn get_messages_filtered() {
        // TODO: with DB
        todo!("Implement message filtering with DB or in-memory sorting")
        // you can test with main.rs at initialization
    }

    pub fn mark_as_sent(&mut self, target_uuid: &String) {
        if let Some(pos) = self
            .pending_send_list
            .iter()
            .position(|(_, s, _)| s == target_uuid)
        {
            let (msg_type, _uuid, _) = self.pending_send_list.remove(pos);
            // If it's an ack, nothing more to do
            if msg_type == MessageType::Ack {
                return;
            }
            let mut result = None;

            for message in &mut self.messages {
                if message.uuid == *target_uuid {
                    message.send_completed = Some(Utc::now());
                    result = Some(message.clone());
                    break;
                }
            }
            if let Some(ref msg) = result {
                self.notify_observers(ChatAppEvent::Info(ChatAppInfoEvent::Sent(msg.clone())));
                return;
            }
        }

        self.notify_observers(ChatAppEvent::Error(ChatAppErrorEvent::InternalError(
            format!(
                "Unable to apply sent success status for message: {}",
                target_uuid
            ),
        )));
    }
}
