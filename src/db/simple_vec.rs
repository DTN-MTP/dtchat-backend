use crate::{
    db::{ChatDataBase, MarkIntent},
    message::{ChatMessage, MessageStatus},
};

pub struct SimpleVecDB {
    messages: Vec<ChatMessage>,
}

impl Default for SimpleVecDB {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
        }
    }
}

impl ChatDataBase for SimpleVecDB {
    fn get_messages_filtered(&mut self) {
        todo!()
    }

    fn get_last_messages(&self, count: usize) -> Vec<ChatMessage> {
        let len = self.messages.len();
        let start = if count > len { 0 } else { len - count };
        self.messages[start..].to_vec()
    }

    fn add_message(&mut self, msg: ChatMessage) {
        self.messages.push(msg);
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
