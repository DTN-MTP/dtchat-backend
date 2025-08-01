use crate::dtchat::generate_uuid;
use crate::message::ChatMessage;
use crate::proto::proto_message::MsgType;
use crate::proto::{AckMessage, ProtoMessage, TextMessage};
use prost::Message;
use socket_engine::endpoint::Endpoint;

impl ProtoMessage {
    pub fn new_text(msg: &ChatMessage, local_endpoint: Option<Endpoint>) -> ProtoMessage {
        ProtoMessage {
            uuid: msg.uuid.clone(),
            sender_uuid: msg.sender_uuid.clone(),
            timestamp: msg.send_time.timestamp_millis(),
            room_uuid: msg.room_uuid.clone(),
            source_endpoint: local_endpoint.map_or("??".to_string(), |ep| ep.to_string()),
            msg_type: Some(MsgType::Text(TextMessage {
                text: msg.text.clone(),
            })),
        }
    }
    pub fn new_ack(
        for_msg: &ChatMessage,
        local_peer_uuid: String,
        local_endpoint: Option<Endpoint>,
        timestamp: i64,
    ) -> ProtoMessage {
        ProtoMessage {
            uuid: generate_uuid(),
            sender_uuid: local_peer_uuid,
            timestamp,
            room_uuid: for_msg.room_uuid.clone(),
            source_endpoint: local_endpoint.map_or("??".to_string(), |ep| ep.to_string()),
            msg_type: Some(MsgType::Ack(AckMessage {
                message_uuid: for_msg.uuid.clone(),
            })),
        }
    }

    pub fn encode_to_vec(&self) -> Result<Vec<u8>, prost::EncodeError> {
        let mut buf: Vec<u8> = Vec::with_capacity(self.encoded_len());
        self.encode(&mut buf)?;
        Ok(buf)
    }

    pub fn decode_from_vec(vec: Vec<u8>) -> Result<ProtoMessage, prost::DecodeError> {
        ProtoMessage::decode(vec.as_slice())
    }
}
