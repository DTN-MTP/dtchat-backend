use std::collections::HashMap;

use crate::{
    db::{ChatDataBase, MarkIntent},
    dtchat::{Peer, Room},
    message::{ChatMessage, MessageStatus},
};

pub struct SimpleVecDB {
    messages: Vec<ChatMessage>,
    localpeer: Peer,
    peers: HashMap<String, Peer>,
    rooms: HashMap<String, Room>,
}

impl SimpleVecDB {
    pub fn new(
        messages: Vec<ChatMessage>,
        localpeer: Peer,
        peers: Vec<Peer>,
        rooms: Vec<Room>,
    ) -> Self {
        let mut peer_map = HashMap::new();
        let mut room_map = HashMap::new();

        peers.iter().for_each(|p| {
            peer_map.insert(p.uuid.clone(), p.clone());
        });

        rooms.iter().for_each(|r| {
            room_map.insert(r.uuid.clone(), r.clone());
        });

        Self {
            messages,
            localpeer,
            peers: peer_map,
            rooms: room_map,
        }
    }
}

impl ChatDataBase for SimpleVecDB {
    // Peers
    fn get_rooms(&self) -> &HashMap<String, Room> {
        return &self.rooms;
    }

    // Peers
    fn get_other_peers(&self) -> &HashMap<String, Peer> {
        return &self.peers;
    }
    fn get_localpeer(&self) -> &Peer {
        return &self.localpeer;
    }

    // Messages
    fn get_last_messages(&self, count: usize) -> &[ChatMessage] {
        let len = self.messages.len();
        let start = if count > len { 0 } else { len - count };
        &self.messages[start..]
    }

    fn add_message(&mut self, msg: ChatMessage) -> bool {
        self.messages.push(msg);
        true
    }

    fn get_all_messages(&self) -> &Vec<ChatMessage> {
        &self.messages
    }

    fn mark_as(&mut self, uuid: &String, intent: super::MarkIntent) -> Option<ChatMessage> {
        for message in &mut self.messages {
            if message.uuid == *uuid {
                match intent {
                    MarkIntent::Acked(date_time) => {
                        message.receive_time = Some(date_time);
                        message.status = MessageStatus::ReceivedByPeer;

                        return Some(message.clone());
                    }
                    MarkIntent::Sent(date_time) => {
                        message.send_completed = Some(date_time);
                        message.status = MessageStatus::Sent;
                        return Some(message.clone());
                    }
                    MarkIntent::Failed => {
                        message.status = MessageStatus::Failed;
                        return Some(message.clone());
                    }
                }
            }
        }
        None
    }
}
