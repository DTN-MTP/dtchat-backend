use std::sync::{Arc, Mutex};

use chrono::Utc;
use socket_engine::{
    endpoint::Endpoint,
    engine::Engine,
    event::{ConnectionEvent, ConnectionFailureReason, DataEvent, EngineObserver, ErrorEvent, SocketEngineEvent},
};
use uuid::Uuid;

use crate::{
    db::{ChatDataBase, MarkIntent},
    event::{
        AppEventObserver, ChatAppErrorEvent, ChatAppEvent, ChatAppInfoEvent, NetworkErrorEvent,
        NetworkEvent,
    },
    message::{ChatMessage, SortStrategy},
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
    observers: Vec<Arc<Mutex<dyn AppEventObserver>>>,
    network_engine: Option<Engine>,
    pending_send_list: Vec<(MessageType, String, Option<String>)>, // msg_type, uuid, original_msg_id pour ACK
    db: Box<dyn ChatDataBase>,
}

impl EngineObserver for ChatModel {
    fn on_engine_event(&mut self, event: SocketEngineEvent) {
        match event {
            SocketEngineEvent::Data(data_event) => match data_event {
                DataEvent::Received { data, from } => {
                    self.notify_observers(ChatAppEvent::SocketEngineInfo(NetworkEvent::Data(
                        DataEvent::Received {
                            data: data.clone(),
                            from,
                        },
                    )));

                    let decode_res = ProtoMessage::decode_from_vec(data);

                    match decode_res {
                        Ok(proto_msg) => {
                            self.treat_proto_message(proto_msg);
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
                DataEvent::Sent {
                    message_id,
                    to,
                    bytes_sent,
                } => {
                    self.notify_observers(ChatAppEvent::SocketEngineInfo(NetworkEvent::Data(
                        DataEvent::Sent {
                            message_id: message_id.clone(),
                            to,
                            bytes_sent,
                        },
                    )));

                    self.mark_as_sent(&message_id);
                }
                DataEvent::Sending {
                    message_id,
                    to,
                    bytes,
                } => {
                    self.notify_observers(ChatAppEvent::SocketEngineInfo(NetworkEvent::Data(
                        DataEvent::Sending {
                            message_id: message_id.clone(),
                            to,
                            bytes,
                        },
                    )));
                }
            },
            SocketEngineEvent::Connection(connection_event) => match connection_event {
                ConnectionEvent::ListenerStarted { endpoint } => {
                    self.notify_observers(ChatAppEvent::SocketEngineInfo(
                        NetworkEvent::Connection(ConnectionEvent::ListenerStarted { endpoint }),
                    ));
                }
                ConnectionEvent::Established { remote } => {
                    self.notify_observers(ChatAppEvent::SocketEngineInfo(
                        NetworkEvent::Connection(ConnectionEvent::Established {
                            remote: remote.clone(),
                        }),
                    ));
                }
                ConnectionEvent::Closed { remote } => {
                    self.notify_observers(ChatAppEvent::SocketEngineInfo(
                        NetworkEvent::Connection(ConnectionEvent::Closed {
                            remote: remote.clone(),
                        }),
                    ));
                }
            },
            SocketEngineEvent::Error(error_event) => match &error_event {
                ErrorEvent::ConnectionFailed {
                    endpoint,
                    reason,
                    token,
                } => {
                    let error_message = match reason {
                        ConnectionFailureReason::Refused => {
                            format!("Connection refused to {}", endpoint)
                        }
                        ConnectionFailureReason::Timeout => {
                            format!("Connection timeout to {}", endpoint)
                        }
                        ConnectionFailureReason::NetworkUnreachable => {
                            format!("Network unreachable to {}", endpoint)
                        }
                        ConnectionFailureReason::Other => {
                            format!("Connection failed to {} (unknown reason)", endpoint)
                        }
                    };
                    
                    self.notify_observers(ChatAppEvent::Error(
                        ChatAppErrorEvent::HostError(error_message),
                    ));

                    self.mark_pending_message_as_failed(token);
                }
                ErrorEvent::SendFailed {
                    endpoint: _,
                    reason: _,
                    token,
                } => {
                    self.mark_pending_message_as_failed(token);
                }
                ErrorEvent::ReceiveFailed { .. } => {
                    self.notify_observers(ChatAppEvent::SocketEngineError(
                        NetworkErrorEvent::SocketError(error_event.clone()),
                    ));
                }
                ErrorEvent::SocketError { .. } => {
                    self.notify_observers(ChatAppEvent::SocketEngineError(
                        NetworkErrorEvent::SocketError(error_event.clone()),
                    ));
                }
            },
        }
    }
}

impl ChatModel {
    pub fn new(localpeer: Peer, peers: Vec<Peer>, db: Box<dyn ChatDataBase>) -> Self {
        Self {
            // TODO: have an SQL(ite) db.rs
            sort_strategy: SortStrategy::Standard,
            localpeer,
            peers, // TODO Maybe move that to DB once the sender endpoint is on the protobuf msg
            observers: Vec::new(),
            network_engine: None,
            pending_send_list: Vec::new(),
            db,
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

    pub fn treat_proto_message(&mut self, proto_msg: ProtoMessage) {
        match &proto_msg.msg_type {
            Some(MsgType::Text(text_part)) => {
                let chat_msg = ChatMessage::new_received(&proto_msg, &text_part);
                if let Some(msg) = chat_msg {
                    self.add_message(msg.clone());

                    match Endpoint::from_str(proto_msg.source_endpoint.as_str()) {
                        Ok(endpoint) => self.send_ack_to_peer(&msg, endpoint),
                        Err(_err) => {
                            self.notify_observers(ChatAppEvent::Error(
                                ChatAppErrorEvent::ProtocolDecode(
                                    "Received proto message source endpoint cannot be parsed"
                                        .to_string(),
                                ),
                            ));
                            // TODO: fallback to another endpoint for the distant peer?
                        }
                    }
                }
            }
            Some(MsgType::Ack(ack)) => {
                self.mark_as_acked(&ack.message_uuid);
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
            let bytes_res = ProtoMessage::new_text(&chatmsg, self.localpeer.endpoints[0].clone())
                .encode_to_vec();
            match bytes_res {
                Ok(bytes) => {
                    let _ = engine.send_async_runtime(endpoint.clone(), bytes, sending_uuid);
                }
                Err(err) => {
                    self.notify_observers(ChatAppEvent::Error(ChatAppErrorEvent::ProtocolEncode(
                        format!("Failed to encode message: {}", err),
                    )));
                }
            }
        }
    }

    pub fn send_ack_to_peer(&mut self, for_msg: &ChatMessage, target_endpoint: Endpoint) {
        let proto_msg = ProtoMessage::new_ack(
            for_msg,
            self.localpeer.uuid.clone(),
            self.localpeer.endpoints[0].clone(),
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
                        engine.send_async_runtime(target_endpoint.clone(), bytes, proto_msg.uuid.clone());
                    self.notify_observers(ChatAppEvent::Info(ChatAppInfoEvent::AckSent(
                        for_msg.uuid.clone(),
                        target_endpoint.to_string(),
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

    fn add_message(&mut self, new_msg: ChatMessage) {
        self.db.add_message(new_msg.clone());

        let event = if self.localpeer.uuid == new_msg.sender_uuid {
            ChatAppEvent::Info(ChatAppInfoEvent::Sending(new_msg.clone()))
        } else {
            ChatAppEvent::Info(ChatAppInfoEvent::Received(new_msg.clone()))
        };
        self.notify_observers(event);
    }

    fn mark_as_acked(&mut self, message_uuid: &String) {
        if let Some(message) = self
            .db
            .mark_as(&message_uuid, MarkIntent::Acked(Utc::now()))
        {
            self.notify_observers(ChatAppEvent::Info(ChatAppInfoEvent::AckReceived(
                message.uuid.clone(),
            )));
        } else {
            self.notify_observers(ChatAppEvent::Error(ChatAppErrorEvent::MessageNotFound(
                format!("Received ack for unknown message: {}", message_uuid),
            )));
        }
    }

    pub fn get_messages_filtered(&mut self) {
        self.db.get_messages_filtered();
    }

    pub fn mark_as_sent(&mut self, target_uuid: &String) {
        if let Some(pos) = self
            .pending_send_list
            .iter()
            .position(|(_, s, _)| s == target_uuid)
        {
            let (msg_type, _uuid, _) = self.pending_send_list.remove(pos);
            if msg_type == MessageType::Ack {
                return;
            }

            if let Some(message) = self.db.mark_as(&target_uuid, MarkIntent::Sent(Utc::now())) {
                self.notify_observers(ChatAppEvent::Info(ChatAppInfoEvent::Sent(message)));
            } else {
                self.notify_observers(ChatAppEvent::Error(ChatAppErrorEvent::MessageNotFound(
                    format!("Message cannot be found in the database: {}", target_uuid),
                )));
            }
            return;
        }

        self.notify_observers(ChatAppEvent::Error(ChatAppErrorEvent::InternalError(
            format!(
                "Message cannot be found in the sending pending list: {}",
                target_uuid
            ),
        )));
    }

    fn mark_pending_message_as_failed(&mut self, target_uuid: &String) {
        if let Some(pos) = self
            .pending_send_list
            .iter()
            .position(|(_, s, _)| s == target_uuid)
        {
            let (msg_type, _uuid, _) = self.pending_send_list.remove(pos);

            match msg_type {
                MessageType::Ack => {}
                // TODO: what is the strategy ? retries ? Maybe "nothing", the handling of this can be user
                // action, like pressing a "retry" button,
                MessageType::Text => {
                    if let Some(message) = self.db.mark_as(&target_uuid, MarkIntent::Failed) {
                        // self.notify_observers(ChatAppEvent::Info(ChatAppInfoEvent::AckReceived(())));
                        self.notify_observers(ChatAppEvent::Error(ChatAppErrorEvent::HostError(
                            format!("Message failed to send: {}", message.uuid),
                        )));
                    } else {
                        self.notify_observers(ChatAppEvent::Error(
                            ChatAppErrorEvent::MessageNotFound(format!(
                                "Message cannot be found in the database: {}",
                                target_uuid
                            )),
                        ));
                    }
                }
            }
        } else {
            self.notify_observers(ChatAppEvent::Error(ChatAppErrorEvent::InternalError(
                format!(
                    "Message cannot be found in the sending pending list: {}",
                    target_uuid
                ),
            )));
        }
    }
}
