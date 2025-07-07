use std::{sync::{Arc, Mutex}};

use chrono::{DateTime, Utc};
use socket_engine::{
    endpoint::Endpoint,
    engine::Engine,
    event::{EngineObserver, SocketEngineEvent},
};
use uuid::Uuid;

use crate::{
    event::{AppEvent, AppEventObserver},
    message::{relative_cmp, standard_cmp, ChatMessage, SortStrategy},
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

pub struct ChatModel {
    pub sort_strategy: SortStrategy,
    pub localpeer: Peer,
    pub peers: Vec<Peer>,
    pub messages: Vec<ChatMessage>,

    observers: Vec<Arc<Mutex<dyn AppEventObserver>>>,
    network_engine: Option<Engine>,
    pending_send_list: Vec<(bool, String)>,
}

impl EngineObserver for ChatModel {
    fn notify(&mut self, event: SocketEngineEvent) {
        match event {
            SocketEngineEvent::Reception(bytes) => {
                let decode_res = ProtoMessage::decode_from_vec(bytes);
                match decode_res {
                    Ok(proto_msg) => self.treat_proto_message(proto_msg),
                    Err(decode_err) => {
                        self.notify_observers(AppEvent::Error(format!("Protobuf: {}", decode_err)))
                    }
                };
            }
            SocketEngineEvent::SentError((_err_msg, _uuid)) => todo!(),
            SocketEngineEvent::Sent(uuid) => {
                self.mark_as_sent(&uuid);
            }
        }
    }
}

impl ChatModel {
    pub fn new(localpeer: Peer, peers: Vec<Peer>) -> Self {
        Self {
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
                eng.start_listener(endpoint.clone());
            }
        }
    }

    pub fn treat_proto_message(&mut self, proto_msg: ProtoMessage) {
        match &proto_msg.msg_type {
            Some(MsgType::Text(text_part)) => {
                let chat_msg = ChatMessage::new_received(&proto_msg, &text_part);

                match chat_msg {
                    Some(msg) => {
                        self.add_message(msg.clone());
                        self.send_ack_to_peer(&msg);
                    }
                    None => self
                        .notify_observers(AppEvent::Error(format!("Protobuf: Invalid timestamp"))),
                }
            }
            Some(MsgType::Ack(ack)) => self.apply_ack(&ack.message_uuid, Utc::now()),
            None => self.notify_observers(AppEvent::Error(format!(
                "Messaging: received proto variant of unknown type"
            ))),
        }
    }

    pub fn add_observer(&mut self, obs: Arc<Mutex<dyn AppEventObserver>>) {
        self.observers.push(obs);
    }

    pub fn notify_observers(&self, event: AppEvent) {
        for obs in &self.observers {
            obs.lock().unwrap().on_event(event.clone());
        }
    }

    pub fn send_to_peer(&mut self, text: &String, room: &String, endpoint: &Endpoint) {
        let chatmsg = ChatMessage::new_to_send(&self.localpeer.uuid, room, text);
        let sending_uuid = chatmsg.uuid.clone();
        self.pending_send_list.push((true, sending_uuid.clone()));
        self.add_message(chatmsg.clone());

        if let Some(engine) = &mut self.network_engine {
            let bytes_res = ProtoMessage::new_text(&chatmsg).encode_to_vec();
            match bytes_res {
                Ok(bytes) => {
                    let _ = engine.send_async(endpoint.clone(), bytes, sending_uuid);
                    // todo let _
                }
                Err(_err) => todo!(),
            }
        } else {
            self.notify_observers(AppEvent::Error("DTChat: no engined attached".to_string()));
        }
    }

    pub fn send_ack_to_peer(&mut self, for_msg: &ChatMessage) {
        // todo
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
        self.pending_send_list.push((false, proto_msg.uuid.clone()));
        if let Some(engine) = &mut self.network_engine {
            match proto_msg.encode_to_vec() {
                // failure to send ack could be checked
                Ok(bytes) => {
                    let _ =
                        engine.send_async(peer.endpoints[0].clone(), bytes, proto_msg.uuid.clone());
                }
                Err(_) => todo!(),
            };
        } else {
            self.notify_observers(AppEvent::Error("DTChat: no engined attached".to_string()));
        }
    }

    pub fn add_message(&mut self, new_msg: ChatMessage) {
        let idx = match &self.sort_strategy {
            SortStrategy::Standard => self
                .messages
                .binary_search_by(|msg| standard_cmp(msg, &new_msg))
                .unwrap_or_else(|i| i),
            SortStrategy::Relative(peer_uuid) => self
                .messages
                .binary_search_by(|msg| relative_cmp(msg, &new_msg, peer_uuid.as_str()))
                .unwrap_or_else(|i| i),
        };
        self.messages.insert(idx, new_msg.clone());

        let event = if self.localpeer.uuid == new_msg.sender_uuid {
            AppEvent::Sending(new_msg)
        } else {
            AppEvent::Received(new_msg)
        };
        self.notify_observers(event);
    }

    pub fn apply_ack(&mut self, target_uuid: &String, datetime: DateTime<Utc>) {

        let mut result = None;

        for message in &mut self.messages {
            if message.uuid == *target_uuid {
                message.receive_time = Some(datetime);
                result = Some(message.clone());
            }
        }

        if let Some(ref msg) = result {
            self.notify_observers(AppEvent::Acked(msg.clone()));
        } else {
            self.notify_observers(AppEvent::Error(format!(
                "Messaging: received ack for unknown message {}",
                target_uuid
            )));
        }
    }

    pub fn mark_as_sent(&mut self, target_uuid: &String) {
        if let Some(pos) = self
            .pending_send_list
            .iter()
            .position(|(_, s)| s == target_uuid)
        {
            let (not_an_ack, _uuid) = self.pending_send_list.remove(pos);
            // If it's an ack, nothing more to do
            if !not_an_ack {
                return
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
                self.notify_observers(AppEvent::Sent(msg.clone()));
                return;
            }
        }

        self.notify_observers(AppEvent::Error(
            format!("Unable to apply sent success status ({})", target_uuid),
        ));

     }
}
