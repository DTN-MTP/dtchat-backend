use core::cmp::Ordering;

use chrono::{DateTime, Utc};
use socket_engine::endpoint::{Endpoint, EndpointProto};

use crate::{
    dtchat::generate_uuid,
    proto::{ProtoMessage, TextMessage},
};

#[derive(Copy, Clone, Debug)]
pub enum MessageStatus {
    Sending,        // Message en cours d'envoi
    Sent,           // Message envoyé avec succès
    ReceivedByPeer, // Message acquitté
    Failed,         // Message en erreur
    Received,       // Message reçu
}

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub uuid: String,
    pub sender_uuid: String,
    pub room_uuid: String,
    pub text: String,
    pub send_time: DateTime<Utc>,
    pub send_completed: Option<DateTime<Utc>>,
    pub predicted_arrival_time: Option<DateTime<Utc>>,
    pub receive_time: Option<DateTime<Utc>>,
    pub status: MessageStatus,
    pub network_endpoint: Endpoint,
}

fn get_timestamps_frm_opt(datetime_opt: Option<DateTime<Utc>>) -> Option<i64> {
    if let Some(datetime) = datetime_opt {
        return Some(datetime.timestamp_millis() as i64);
    }
    None
}

impl ChatMessage {
    pub fn new_to_send(
        sender_uuid: &String,
        room_uuid: &String,
        text: &String,
        network_endpoint: Endpoint,
    ) -> Self {
        ChatMessage {
            uuid: generate_uuid(),
            sender_uuid: sender_uuid.clone(),
            room_uuid: room_uuid.clone(),
            text: text.clone(),
            send_time: Utc::now(),
            send_completed: None,
            predicted_arrival_time: None,
            receive_time: None,
            status: MessageStatus::Sending,
            network_endpoint,
        }
    }

    pub fn new_received(proto_msg: &ProtoMessage, text_part: &TextMessage) -> Option<Self> {
        if let Some(datetime) = DateTime::from_timestamp_millis(proto_msg.timestamp) {
            if let Some(network_endpoint) = Endpoint::from_str(&proto_msg.source_endpoint).ok() {
                return Some(ChatMessage {
                    uuid: proto_msg.uuid.clone(),
                    sender_uuid: proto_msg.sender_uuid.clone(),
                    room_uuid: proto_msg.room_uuid.clone(),
                    text: text_part.text.clone(),
                    send_time: datetime.clone(),
                    send_completed: Some(datetime),
                    predicted_arrival_time: None,
                    receive_time: Some(Utc::now()),
                    status: MessageStatus::Received,
                    network_endpoint,
                });
            }
        }
        None
    }

    pub fn get_shipment_status_otp(
        &self,
    ) -> (DateTime<Utc>, Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
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

pub fn filter_by_network_endpoint(
    messages: &[ChatMessage],
    protocol_filter: EndpointProto,
) -> Vec<ChatMessage> {
    messages
        .iter()
        .filter(|msg| msg.network_endpoint.proto == protocol_filter)
        .cloned()
        .collect()
}
