use core::cmp::Ordering;
use socket_engine::endpoint::Endpoint;

use crate::{dtchat::generate_uuid, proto::ProtoMessage, time::DTChatTime};

pub struct RoomMessage {
    pub uuid: String,
    pub room_uuid: String,
    pub messages: Vec<String>, // list of uuid replica
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageStatus {
    Sending,        // Message en cours d'envoi
    Sent,           // Message envoyé avec succès
    ReceivedByPeer, // Message acquitté
    Failed,         // Message en erreur
    Received,       // Message reçu
}

#[derive(Clone, Debug)]
pub enum Content {
    Text(String), // message
    File(String), // path
}

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub uuid: String,
    pub sender_uuid: String,
    pub room_uuid: String,
    pub content: Content,
    pub send_time: DTChatTime,
    pub send_completed: Option<DTChatTime>,
    pub predicted_arrival_time: Option<DTChatTime>,
    pub receive_time: Option<DTChatTime>,
    pub status: MessageStatus,
    pub source_endpoint: Endpoint,
}

fn get_timestamps_frm_opt(datetime_opt: Option<DTChatTime>) -> Option<i64> {
    if let Some(datetime) = datetime_opt {
        return Some(datetime.timestamp_millis());
    }
    None
}

impl ChatMessage {
    pub fn new_to_send(
        sender_uuid: &String,
        room_uuid: &String,
        content: Content,
        source_endpoint: Endpoint,
    ) -> Self {
        ChatMessage {
            uuid: generate_uuid(),
            sender_uuid: sender_uuid.clone(),
            room_uuid: room_uuid.clone(),
            content: content.clone(),
            send_time: DTChatTime::now(),
            send_completed: None,
            predicted_arrival_time: None,
            receive_time: None,
            status: MessageStatus::Sending,
            source_endpoint,
        }
    }

    pub fn new_received(proto_msg: &ProtoMessage, content: Content) -> Option<Self> {
        if let Some(datetime) = DTChatTime::from_timestamp_millis(proto_msg.timestamp) {
            if let Some(source_endpoint) = Endpoint::from_str(&proto_msg.source_endpoint).ok() {
                return Some(ChatMessage {
                    uuid: proto_msg.uuid.clone(),
                    sender_uuid: proto_msg.sender_uuid.clone(),
                    room_uuid: proto_msg.room_uuid.clone(),
                    content,
                    send_time: datetime.clone(),
                    send_completed: Some(datetime),
                    predicted_arrival_time: None,
                    receive_time: Some(DTChatTime::now()),
                    status: MessageStatus::Received,
                    source_endpoint,
                });
            }
        }
        None
    }

    pub fn get_shipment_status_otp(&self) -> (DTChatTime, Option<DTChatTime>, Option<DTChatTime>) {
        return (
            self.send_time,
            self.predicted_arrival_time,
            self.receive_time,
        );
    }

    pub fn get_shipment_status_timestamps(&self) -> (i64, Option<i64>, Option<i64>) {
        return (
            self.send_time.timestamp_millis(),
            get_timestamps_frm_opt(self.predicted_arrival_time),
            get_timestamps_frm_opt(self.receive_time),
        );
    }
}

#[derive(PartialEq, Eq, Clone)]
pub enum SortStrategy {
    Standard,
    Relative(String),
}

pub fn insert_with_strategy(
    messages: &mut Vec<ChatMessage>,
    new_msg: ChatMessage,
    strategy: SortStrategy,
) {
    let idx = match strategy {
        SortStrategy::Standard => messages
            .binary_search_by(|msg| standard_cmp(msg, &new_msg))
            .unwrap_or_else(|i| i),
        SortStrategy::Relative(peer_uuid) => messages
            .binary_search_by(|msg| relative_cmp(msg, &new_msg, peer_uuid.as_str()))
            .unwrap_or_else(|i| i),
    };
    messages.insert(idx, new_msg.clone());
}

// Helper for a view having a list of messages
pub fn sort_with_strategy(messages: &mut Vec<ChatMessage>, strategy: SortStrategy) {
    match strategy {
        SortStrategy::Standard => messages.sort_by(standard_cmp),
        SortStrategy::Relative(for_peer) => {
            messages.sort_by(|a, b| relative_cmp(a, b, for_peer.as_str()))
        }
    }
}
pub fn standard_cmp(a: &ChatMessage, b: &ChatMessage) -> Ordering {
    let tx_a = a.send_time;
    let tx_b = b.send_time;

    let rx_a = a.receive_time.unwrap_or(tx_a);
    let rx_b = b.receive_time.unwrap_or(tx_b);
    tx_a.cmp(&tx_b).then(rx_a.cmp(&rx_b))
}

pub fn relative_cmp(a: &ChatMessage, b: &ChatMessage, ctx_peer_uuid: &str) -> Ordering {
    let tx_a = a.send_time;
    let tx_b = b.send_time;

    let rx_a = a.receive_time.unwrap_or(tx_a);
    let rx_b = b.receive_time.unwrap_or(tx_b);

    let anchor_a = if a.sender_uuid == ctx_peer_uuid {
        rx_a
    } else {
        tx_a
    };
    let anchor_b = if b.sender_uuid == ctx_peer_uuid {
        rx_b
    } else {
        tx_b
    };
    anchor_a.cmp(&anchor_b)
}
