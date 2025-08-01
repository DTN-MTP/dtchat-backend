use std::sync::{Arc, Mutex};

use socket_engine::{
    endpoint::{Endpoint, EndpointProto},
    engine::Engine,
    event::{ConnectionEvent, DataEvent, EngineObserver, ErrorEvent, SocketEngineEvent},
};
use uuid::Uuid;

use crate::{
    db::{ChatDataBase, MarkIntent},
    event::{
        AppEventObserver, ChatAppErrorEvent, ChatAppEvent, ChatAppInfoEvent, NetworkErrorEvent,
        NetworkEvent,
    },
    message::{ChatMessage, SortStrategy},
    prediction::PredictionConfig,
    proto::{proto_message::MsgType, ProtoMessage},
    time::DTChatTime,
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
    a_sabr: Option<PredictionConfig>,
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
                    token,
                    to,
                    bytes_sent,
                } => {
                    self.notify_observers(ChatAppEvent::SocketEngineInfo(NetworkEvent::Data(
                        DataEvent::Sent {
                            token: token.clone(),
                            to,
                            bytes_sent,
                        },
                    )));

                    self.mark_as_sent(&token);
                }
                DataEvent::Sending { token, to, bytes } => {
                    self.notify_observers(ChatAppEvent::SocketEngineInfo(NetworkEvent::Data(
                        DataEvent::Sending { token, to, bytes },
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
                    endpoint: _,
                    reason: _,
                    token,
                } => {
                    self.notify_observers(ChatAppEvent::SocketEngineError(
                        NetworkErrorEvent::SocketError(error_event.clone()),
                    ));

                    self.mark_pending_message_as_failed(token);
                }
                ErrorEvent::SendFailed {
                    endpoint: _,
                    reason: _,
                    token,
                } => {
                    self.notify_observers(ChatAppEvent::SocketEngineError(
                        NetworkErrorEvent::SocketError(error_event.clone()),
                    ));
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
            a_sabr: None,
        }
    }
    pub fn start(&mut self, engine: Engine) {
        self.network_engine = Some(engine);
        if let Some(eng) = &mut self.network_engine {
            for endpoint in &mut self.localpeer.endpoints {
                eng.start_listener_async(endpoint.clone());
            }
        }
        let a_sabr_res = PredictionConfig::try_init();
        match a_sabr_res {
            Ok(a_sabr) => self.a_sabr = Some(a_sabr),
            Err(e) => self.notify_observers(ChatAppEvent::Error(ChatAppErrorEvent::InternalError(
                format!("No prediction: {}", e),
            ))),
        }
    }
    pub fn is_pbat_enabled(&self) -> bool {
        self.a_sabr.is_some()
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
                self.mark_as_acked(&ack.message_uuid, proto_msg.timestamp);
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

    pub fn send_to_peer(
        &mut self,
        text: &String,
        room: &String,
        peer_uuid: String,
        endpoint: &Endpoint,
        try_prediction: bool,
    ) {
        let mut chatmsg =
            ChatMessage::new_to_send(&self.localpeer.uuid, room, text, endpoint.clone());
        let sending_uuid = chatmsg.uuid.clone();

        let local_endpoint = self.find_local_endpoint_for_protocol(endpoint.proto.clone());

        self.pending_send_list
            .push((MessageType::Text, sending_uuid.clone(), None));

        let mut size_serialized = None;

        if let Some(engine) = &mut self.network_engine {
            let bytes_res =
                ProtoMessage::new_text(&chatmsg, local_endpoint.clone()).encode_to_vec();
            match bytes_res {
                Ok(bytes) => {
                    size_serialized = Some(bytes.len());
                    engine.send_async(local_endpoint, endpoint.clone(), bytes, sending_uuid);
                }
                Err(err) => {
                    self.notify_observers(ChatAppEvent::Error(ChatAppErrorEvent::ProtocolEncode(
                        format!("Failed to encode message: {}", err),
                    )));
                }
            }
        }
        if try_prediction {
            let bp_local_endpoint_opt = self.find_local_endpoint_for_protocol(EndpointProto::Bp);
            let bp_peer_endpoint_opt =
                self.find_peer_endpoint_for_protocol(peer_uuid, EndpointProto::Bp);

            if let (Some(src_eid), Some(dest_eid)) = (bp_local_endpoint_opt, bp_peer_endpoint_opt) {
                // In theory we should add transport overhead..
                if let (Some(size_sent), Some(a_sabr)) = (size_serialized, &mut self.a_sabr) {
                    if let Ok(arrival_time) = a_sabr.predict(
                        src_eid.endpoint.as_str(),
                        dest_eid.endpoint.as_str(),
                        size_sent as f64,
                    ) {
                        chatmsg.predicted_arrival_time = Some(arrival_time);
                    }
                }
            }
        }
        self.add_message(chatmsg.clone());
    }

    pub fn send_ack_to_peer(&mut self, for_msg: &ChatMessage, target_endpoint: Endpoint) {
        let local_endpoint = self.find_local_endpoint_for_protocol(target_endpoint.proto.clone());

        let proto_msg = ProtoMessage::new_ack(
            for_msg,
            self.localpeer.uuid.clone(),
            local_endpoint.clone(),
            DTChatTime::now().timestamp_millis(),
        );
        self.pending_send_list.push((
            MessageType::Ack,
            proto_msg.uuid.clone(),
            Some(for_msg.uuid.clone()),
        ));
        if let Some(engine) = &mut self.network_engine {
            match proto_msg.encode_to_vec() {
                Ok(bytes) => {
                    engine.send_async(
                        local_endpoint,
                        target_endpoint.clone(),
                        bytes,
                        proto_msg.uuid.clone(),
                    );
                    self.notify_observers(ChatAppEvent::Info(ChatAppInfoEvent::AckSent(
                        for_msg.clone(),
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

    fn mark_as_acked(&mut self, message_uuid: &String, timestamp: i64) {
        if let Some(received_at) = DTChatTime::from_timestamp_millis(timestamp) {
            if let Some(message) = self
                .db
                .mark_as(&message_uuid, MarkIntent::Acked(received_at))
            {
                self.notify_observers(ChatAppEvent::Info(ChatAppInfoEvent::AckReceived(message)));
            } else {
                self.notify_observers(ChatAppEvent::Error(ChatAppErrorEvent::MessageNotFound(
                    format!("Received ack for unknown message: {}", message_uuid),
                )));
            }
        } else {
            self.notify_observers(ChatAppEvent::Error(ChatAppErrorEvent::ProtocolDecode(
                format!(
                    "Protobuf decode error: invalid timestamp to ack message {}",
                    message_uuid
                ),
            )));
        }
    }

    pub fn get_messages_filtered(&mut self) {
        self.db.get_messages_filtered();
    }
    pub fn get_last_messages(&mut self, count: usize) -> Vec<ChatMessage> {
        self.db.get_last_messages(count)
    }

    pub fn get_all_messages(&self) -> Vec<ChatMessage> {
        self.db.get_all_messages()
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

            if let Some(message) = self
                .db
                .mark_as(&target_uuid, MarkIntent::Sent(DTChatTime::now()))
            {
                self.notify_observers(ChatAppEvent::Info(ChatAppInfoEvent::Sent(message)));
            } else {
                self.notify_observers(ChatAppEvent::Error(ChatAppErrorEvent::MessageNotFound(
                    format!("Message cannot be found in the database: {}", target_uuid),
                )));
            }
            return;
        }
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
                    if let Some(_message) = self.db.mark_as(&target_uuid, MarkIntent::Failed) {
                        // TODO: Same
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
        }
    }

    fn find_peer_endpoint_for_protocol(
        &self,
        peer_id: String,
        target_proto: EndpointProto,
    ) -> Option<Endpoint> {
        for peer in &self.peers {
            if peer.uuid != peer_id {
                continue;
            }
            return peer
                .endpoints
                .iter()
                .find(|ep| ep.proto == target_proto)
                .cloned();
        }
        None
    }

    fn find_local_endpoint_for_protocol(&self, target_proto: EndpointProto) -> Option<Endpoint> {
        self.localpeer
            .endpoints
            .iter()
            .find(|ep| ep.proto == target_proto)
            .cloned()
    }
}
