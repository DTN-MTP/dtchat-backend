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

    fn add_message(&mut self, msg: ChatMessage) {
        self.messages.push(msg);
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
