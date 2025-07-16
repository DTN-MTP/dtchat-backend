use std::sync::{Arc, Mutex};

use chrono::Utc;
use socket_engine::{
    endpoint::Endpoint,
    engine::Engine,
    event::{
        EngineObserver, ConnectionEvent, DataEvent, ErrorEvent, SocketEngineEvent,
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

pub fn extract_message_id_from_data(data: &Vec<u8>) -> Option<String> {
    match ProtoMessage::decode_from_vec(data.clone()) {
        Ok(proto_msg) => {
            let uuid = &proto_msg.uuid;
            if uuid.len() >= 8 {
                Some(uuid[..8].to_string())
            } else {
                Some(uuid.clone())
            }
        },
        Err(_) => None,
    }
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
            SocketEngineEvent::Data(data_event) => match data_event {
                DataEvent::Received { data, from } => {
                    let real_peer_endpoint = self.map_to_real_peer_endpoint(&from);
                    
                    self.notify_observers(ChatAppEvent::SocketEngineInfo(
                        NetworkEvent::Data(DataEvent::Received {
                            data: data.clone(),
                            from: real_peer_endpoint,
                        }),
                    ));

                    let decode_res = ProtoMessage::decode_from_vec(data);

                    match decode_res {
                        Ok(proto_msg) => {
                            self.treat_proto_message(proto_msg, from);
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
                DataEvent::Sent { message_id, to, bytes_sent } => {
                    self.notify_observers(ChatAppEvent::SocketEngineInfo(
                        NetworkEvent::Data(DataEvent::Sent {
                            message_id: message_id.clone(),
                            to,
                            bytes_sent,
                        }),
                    ));

                    self.mark_as_sent(&message_id);
                }
            },
            SocketEngineEvent::Connection(connection_event) => match connection_event {
                ConnectionEvent::ListenerStarted { endpoint } => {
                    self.notify_observers(ChatAppEvent::SocketEngineInfo(
                        NetworkEvent::Connection(ConnectionEvent::ListenerStarted {
                            endpoint,
                        }),
                    ));
                }
                ConnectionEvent::Established { remote } => {
                    self.notify_observers(ChatAppEvent::SocketEngineInfo(
                        NetworkEvent::Connection(ConnectionEvent::Established {
                            remote: remote.clone(),
                        }),
                    ));

                    self.mark_messages_as_successfully_sent();
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
                ErrorEvent::ConnectionFailed { endpoint, .. } => {
                    self.notify_observers(ChatAppEvent::SocketEngineError(
                        NetworkErrorEvent::SocketError(error_event.clone()),
                    ));
                    
                    self.notify_observers(ChatAppEvent::Error(
                        ChatAppErrorEvent::HostNotReachable(format!("{}", endpoint))
                    ));
                    
                    self.mark_pending_messages_as_failed(&endpoint);
                }
                ErrorEvent::SendFailed { endpoint, .. } => {
                    self.notify_observers(ChatAppEvent::SocketEngineError(
                        NetworkErrorEvent::SocketError(error_event.clone()),
                    ));
                    
                    self.mark_pending_messages_as_failed(&endpoint);
                }
                ErrorEvent::ReceiveFailed { .. } => {
                    self.notify_observers(ChatAppEvent::SocketEngineError(
                        NetworkErrorEvent::SocketError(error_event.clone()),
                    ));
                }
                ErrorEvent::SocketError { endpoint, .. } => {
                    self.notify_observers(ChatAppEvent::SocketEngineError(
                        NetworkErrorEvent::SocketError(error_event.clone()),
                    ));
                    
                    self.mark_pending_messages_as_failed(&endpoint);
                }
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

    pub fn treat_proto_message(&mut self, proto_msg: ProtoMessage, source_endpoint: Endpoint) {
        match &proto_msg.msg_type {
            Some(MsgType::Text(text_part)) => {
                let chat_msg = ChatMessage::new_received(&proto_msg, &text_part);
                if let Some(msg) = chat_msg {
                    self.add_message(msg.clone());
                    self.send_ack_to_peer(&msg);
                }
            }
            Some(MsgType::Ack(ack)) => {
                self.apply_ack(&ack.message_uuid, source_endpoint);
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
                        for_msg.uuid.clone(),
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

    fn apply_ack(&mut self, message_uuid: &String, source_endpoint: Endpoint) {
        let mut result = None;
        let datetime = Utc::now();
        for message in &mut self.messages {
            if message.uuid == *message_uuid {
                message.receive_time = Some(datetime);
                result = Some(message.clone());
            }
        }

        if let Some(ref msg) = result {
            let real_peer_endpoint = self.map_to_real_peer_endpoint(&source_endpoint);
            
            let ack_sender_peer = self.peers.iter()
                .find(|peer| peer.endpoints.contains(&real_peer_endpoint))
                .map(|peer| peer.uuid.clone())
                .unwrap_or_else(|| format!("{}", real_peer_endpoint));

            self.notify_observers(ChatAppEvent::Info(ChatAppInfoEvent::AckReceived(
                msg.uuid.clone(),
                ack_sender_peer,
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

            let mut updated_message = None;
            for message in &mut self.messages {
                if message.uuid == *target_uuid {
                    message.send_completed = Some(Utc::now());
                    updated_message = Some(message.clone());
                    break;
                }
            }
            
            if let Some(message) = updated_message {
                self.notify_observers(ChatAppEvent::Info(ChatAppInfoEvent::Sent(message)));
            }
            return;
        }

        self.notify_observers(ChatAppEvent::Error(ChatAppErrorEvent::InternalError(
            format!(
                "Unable to apply sent success status for message: {}",
                target_uuid
            ),
        )));
    }


    fn map_to_real_peer_endpoint(&self, temp_endpoint: &Endpoint) -> Endpoint {
        if let Endpoint::Tcp(temp_addr) = temp_endpoint {
            let temp_ip = temp_addr.split(':').next().unwrap_or("");
            
            self.peers.iter()
                .find(|peer| {
                    peer.endpoints.iter().any(|endpoint| {
                        match endpoint {
                            Endpoint::Tcp(peer_addr) => {
                                peer_addr.split(':').next().unwrap_or("") == temp_ip
                            },
                            _ => false,
                        }
                    })
                })
                .and_then(|peer| peer.endpoints.first())
                .cloned()
                .unwrap_or_else(|| temp_endpoint.clone())
        } else {
            temp_endpoint.clone()
        }
    }

    fn mark_pending_messages_as_failed(&mut self, _endpoint: &Endpoint) {
        let messages_to_fail: Vec<String> = self.messages
            .iter()
            .filter(|message| {
                matches!(message.status, MessageStatus::Sending | MessageStatus::Sent) 
                    && message.sender_uuid == self.localpeer.uuid
            })
            .map(|message| message.uuid.clone())
            .collect();
        
        for message in &mut self.messages {
            if matches!(message.status, MessageStatus::Sending | MessageStatus::Sent) 
                && message.sender_uuid == self.localpeer.uuid {
                message.status = MessageStatus::Failed;
            }
        }
        
        for message_uuid in messages_to_fail {
            self.notify_observers(ChatAppEvent::Info(ChatAppInfoEvent::MessageStatusChanged(
                message_uuid, 
                MessageStatus::Failed
            )));
        }
    }

    fn mark_messages_as_successfully_sent(&mut self) {
        let messages_to_update: Vec<String> = self.messages
            .iter()
            .filter(|message| {
                matches!(message.status, MessageStatus::Sending) 
                    && message.sender_uuid == self.localpeer.uuid
            })
            .map(|message| message.uuid.clone())
            .collect();
        
        for message in &mut self.messages {
            if matches!(message.status, MessageStatus::Sending) 
                && message.sender_uuid == self.localpeer.uuid {
                message.status = MessageStatus::Sent;
            }
        }
        
        for message_uuid in messages_to_update {
            self.notify_observers(ChatAppEvent::Info(ChatAppInfoEvent::MessageStatusChanged(
                message_uuid, 
                MessageStatus::Sent
            )));
        }
    }
}
