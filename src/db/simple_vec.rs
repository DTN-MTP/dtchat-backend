use crate::{
    db::{ChatDataBase, MarkIntent},
    dtchat::{Peer, Room},
    message::{ChatMessage, MessageStatus},
};

pub struct SimpleVecDB {
    messages: Vec<ChatMessage>,
    localpeer: Peer,
    peers: Vec<Peer>,
    rooms: Vec<Room>,
}

impl SimpleVecDB {
    pub fn new(
        messages: Vec<ChatMessage>,
        localpeer: Peer,
        peers: Vec<Peer>,
        rooms: Vec<Room>,
    ) -> Self {
        Self {
            messages,
            localpeer,
            peers,
            rooms,
        }
    }
}

impl ChatDataBase for SimpleVecDB {
    // Peers
    fn get_rooms(&self) -> Vec<Room> {
        return self.rooms.clone();
    }

    // Peers
    fn get_other_peers(&self) -> Vec<crate::dtchat::Peer> {
        return self.peers.clone();
    }
    fn get_localpeer(&self) -> crate::dtchat::Peer {
        return self.localpeer.clone();
    }

    // Messages
    fn get_last_messages(&self, count: usize) -> Vec<ChatMessage> {
        let len = self.messages.len();
        let start = if count > len { 0 } else { len - count };
        self.messages[start..].to_vec()
    }

    fn add_message(&mut self, msg: ChatMessage) -> bool {
        self.messages.push(msg);
        true
    }

    fn get_all_messages(&self) -> Vec<ChatMessage> {
        self.messages.clone()
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
