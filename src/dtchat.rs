use std::{
    collections::HashMap,
    fs,
    path::Path,
    sync::{Arc, Mutex},
};

use socket_engine::{
    endpoint::{Endpoint, EndpointProto},
    engine::Engine,
    event::{ConnectionEvent, DataEvent, EngineObserver, ErrorEvent, SocketEngineEvent},
};
use uuid::Uuid;

use crate::{
    config::AppConfig,
    db::{ChatDataBase, MarkIntent},
    event::{
        AppEventObserver, ChatAppErrorEvent, ChatAppEvent, ChatAppInfoEvent, NetworkErrorEvent,
        NetworkEvent,
    },
    message::{ChatMessage, Content, RoomMessage, SortStrategy},
    prediction::PredictionConfig,
    proto::{proto_message::MsgType, ProtoMessage},
    time::DTChatTime,
};

pub fn generate_uuid() -> String {
    Uuid::new_v4().to_string()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Peer {
    pub uuid: String,
    pub name: String,
    pub endpoints: Vec<Endpoint>,
    pub color: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Room {
    pub uuid: String,
    pub name: String,
    pub participants: Vec<(String, Endpoint)>,
}

#[derive(PartialEq, Eq)]
enum MessageType {
    Ack,
    Text,
}

pub enum ASabrInitState {
    Enabled(PredictionConfig),
    Error(String),
    Disabled,
}

pub struct ChatModel {
    pub sort_strategy: SortStrategy,

    observers: Vec<Arc<Mutex<dyn AppEventObserver>>>,
    network_engine: Option<Engine>,
    pending_send_list: Vec<(MessageType, String, Option<String>)>, // msg_type, uuid, original_msg_id pour ACK
    db: Box<dyn ChatDataBase>,
    a_sabr: ASabrInitState,
    reception_folder: String,
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
    pub fn new() -> Self {
        let (db, pred, reception_folder) = AppConfig::new();
        Self {
            // TODO: have an SQL(ite) db.rs
            sort_strategy: SortStrategy::Standard,
            observers: Vec::new(),
            network_engine: None,
            pending_send_list: Vec::new(),
            db,
            a_sabr: pred,
            reception_folder,
        }
    }

    pub fn start(&mut self, engine: Engine) {
        self.network_engine = Some(engine);
        let endpoints = &self.db.get_localpeer().endpoints;
        if let Some(eng) = &mut self.network_engine {
            for endpoint in endpoints {
                eng.start_listener_async(endpoint.clone());
            }
        }
        let message = match &self.a_sabr {
            ASabrInitState::Enabled(_) => "A-SABR prediction enabled".to_string(),
            ASabrInitState::Error(err) => {
                format!("Initialization of A-SABR prediction failed: {err}")
            }
            ASabrInitState::Disabled => {
                "A-SABR prediction disabled (cp_path must be set)".to_string()
            }
        };
        self.notify_observers(ChatAppEvent::Info(message));
    }
    pub fn is_pbat_enabled(&self) -> bool {
        if let ASabrInitState::Enabled(_) = self.a_sabr {
            return true;
        }
        false
    }

    fn treat_file_and_text(&mut self, msg_opt: Option<ChatMessage>, proto_msg: &ProtoMessage) {
        if let Some(msg) = msg_opt {
            self.add_message(msg.clone());

            match Endpoint::from_str(proto_msg.source_endpoint.as_str()) {
                Ok(endpoint) => self.send_ack_to_peer(&msg, endpoint),
                Err(_err) => {
                    self.notify_observers(ChatAppEvent::Error(ChatAppErrorEvent::ProtocolDecode(
                        "Received proto message source endpoint cannot be parsed".to_string(),
                    )));
                }
            }
        }
    }

    pub fn treat_proto_message(&mut self, proto_msg: ProtoMessage) {
        match &proto_msg.msg_type {
            Some(MsgType::Text(text_part)) => {
                let chat_msg =
                    ChatMessage::new_received(&proto_msg, Content::Text(text_part.text.clone()));
                self.treat_file_and_text(chat_msg, &proto_msg)
            }

            Some(MsgType::File(file_part)) => {
                let chat_msg =
                    ChatMessage::new_received(&proto_msg, Content::File(file_part.name.clone()));
                let full_path = Path::new(&self.reception_folder).join(file_part.name.clone());
                match fs::write(&full_path, file_part.data.clone()) {
                    Ok(_) => {
                        self.notify_observers(ChatAppEvent::Info(format!(
                            "File stored: {}",
                            file_part.name
                        )));
                    }
                    Err(err) => {
                        self.notify_observers(ChatAppEvent::Error(
                            ChatAppErrorEvent::InternalError(format!(
                                "Unable to save received file: {}",
                                err
                            )),
                        ));
                    }
                }

                self.treat_file_and_text(chat_msg, &proto_msg)
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

    pub fn get_other_peers_for_room(&self, room_uuid: &String) -> Option<Vec<(String, Endpoint)>> {
        let rooms = self.db.get_rooms();
        for (uuid, room) in rooms {
            if *uuid != *room_uuid {
                continue;
            }
            let mut is_allowed = false;
            let mut participations: Vec<(String, Endpoint)> = Vec::new();
            for reg in &room.participants {
                if reg.0 == self.db.get_localpeer().uuid {
                    is_allowed = true;
                } else {
                    participations.push(reg.clone());
                }
            }
            if is_allowed {
                return Some(participations);
            }
        }
        None
    }

    pub fn send_to_room(
        &mut self,
        content: &Content,
        room_uuid: &String,
        try_prediction: bool,
    ) -> Option<RoomMessage> {
        let participants_opt = self.get_other_peers_for_room(room_uuid);
        if let Some(participants) = participants_opt {
            let mut room_msg = RoomMessage {
                uuid: generate_uuid(),
                room_uuid: room_uuid.clone(),
                messages: Vec::new(),
            };
            if participants.len() == 0 {
                return None;
            }

            for (peer_uuid, endpoint) in participants {
                room_msg.messages.push(self.send_to_peer(
                    content,
                    &room_uuid,
                    peer_uuid,
                    &endpoint,
                    try_prediction,
                ));
            }
            return Some(room_msg);
        }
        None
    }

    pub fn send_to_peer(
        &mut self,
        content: &Content,
        room_uuid: &String,
        peer_uuid: String,
        endpoint: &Endpoint,
        try_prediction: bool,
    ) -> String {
        let mut chatmsg = ChatMessage::new_to_send(
            &self.db.get_localpeer().uuid,
            room_uuid,
            content.clone(),
            endpoint.clone(),
        );
        let sending_uuid = chatmsg.uuid.clone();

        let local_endpoint = self.find_local_endpoint_for_protocol(endpoint.proto.clone());

        self.pending_send_list
            .push((MessageType::Text, sending_uuid.clone(), None));

        let mut size_serialized = None;

        if let Some(engine) = &mut self.network_engine {
            match ProtoMessage::new_text(&chatmsg, local_endpoint.clone()) {
                Ok(create_proto) => match create_proto.encode_to_vec() {
                    Ok(bytes) => {
                        size_serialized = Some(bytes.len());
                        engine.send_async(local_endpoint, endpoint.clone(), bytes, sending_uuid);
                    }
                    Err(err) => {
                        self.notify_observers(ChatAppEvent::Error(
                            ChatAppErrorEvent::ProtocolEncode(format!(
                                "Failed to encode message: {}",
                                err
                            )),
                        ));
                    }
                },
                Err(err) => self.notify_observers(ChatAppEvent::Error(
                    ChatAppErrorEvent::InternalError(format!("Failed to encode message: {}", err)),
                )),
            }
        }
        if try_prediction {
            let bp_local_endpoint_opt = self.find_local_endpoint_for_protocol(EndpointProto::Bp);
            let bp_peer_endpoint_opt =
                self.find_peer_endpoint_for_protocol(peer_uuid, EndpointProto::Bp);

            if let (Some(src_eid), Some(dest_eid)) = (bp_local_endpoint_opt, bp_peer_endpoint_opt) {
                // In theory we should add transport overhead..
                if let (Some(size_sent), ASabrInitState::Enabled(a_sabr)) =
                    (size_serialized, &mut self.a_sabr)
                {
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
        return chatmsg.uuid;
    }

    pub fn send_ack_to_peer(&mut self, for_msg: &ChatMessage, target_endpoint: Endpoint) {
        let local_endpoint = self.find_local_endpoint_for_protocol(target_endpoint.proto.clone());

        let proto_msg = ProtoMessage::new_ack(
            for_msg,
            self.db.get_localpeer().uuid.clone(),
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
                    self.notify_observers(ChatAppEvent::Message(ChatAppInfoEvent::AckSent(
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

        let event = if self.db.get_localpeer().uuid == new_msg.sender_uuid {
            ChatAppEvent::Message(ChatAppInfoEvent::Sending(new_msg.clone()))
        } else {
            ChatAppEvent::Message(ChatAppInfoEvent::Received(new_msg.clone()))
        };
        self.notify_observers(event);
    }

    fn mark_as_acked(&mut self, message_uuid: &String, timestamp: i64) {
        if let Some(received_at) = DTChatTime::from_timestamp_millis(timestamp) {
            if let Some(message) = self
                .db
                .mark_as(&message_uuid, MarkIntent::Acked(received_at))
            {
                self.notify_observers(ChatAppEvent::Message(ChatAppInfoEvent::AckReceived(
                    message,
                )));
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

    pub fn get_other_peers(&self) -> HashMap<String, Peer> {
        self.db.get_other_peers().clone()
    }
    pub fn get_localpeer(&self) -> Peer {
        self.db.get_localpeer().clone()
    }
    pub fn get_rooms(&self) -> HashMap<String, Room> {
        self.db.get_rooms().clone()
    }

    pub fn get_last_messages(&mut self, count: usize) -> Vec<ChatMessage> {
        self.db.get_last_messages(count).to_vec()
    }

    pub fn get_all_messages(&self) -> Vec<ChatMessage> {
        self.db.get_all_messages().clone()
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
                self.notify_observers(ChatAppEvent::Message(ChatAppInfoEvent::Sent(message)));
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
        let binding = self.db.get_other_peers();
        let peer_opt = &binding.get(&peer_id);
        if let Some(peer) = peer_opt {
            return peer
                .endpoints
                .iter()
                .find(|ep| ep.proto == target_proto)
                .cloned();
        }

        None
    }

    fn find_local_endpoint_for_protocol(&self, target_proto: EndpointProto) -> Option<Endpoint> {
        self.db
            .get_localpeer()
            .endpoints
            .iter()
            .find(|ep| ep.proto == target_proto)
            .cloned()
    }
}
