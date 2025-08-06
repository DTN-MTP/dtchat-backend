use std::sync::{Arc, Mutex};

use dtchat_backend::{
    dtchat::ChatModel,
    event::{
        AppEventObserver, ChatAppErrorEvent, ChatAppEvent, ChatAppInfoEvent, NetworkErrorEvent,
        NetworkEvent,
    },
    message::{ChatMessage, MessageStatus},
    time::DTChatTime,
};
use socket_engine::{
    engine::Engine,
    event::{ConnectionEvent, DataEvent},
};

use std::collections::VecDeque;
use std::io::{self, Write};

// Helper function to safely extract first 8 characters of a message ID
fn safe_message_id_display(id: &str) -> &str {
    if id.len() >= 8 {
        &id[..8]
    } else {
        id
    }
}

#[derive(Clone, Debug)]
pub enum EventLevel {
    Info,    // Informations normales
    Debug,   // Détails techniques (réseau)
    Warning, // Problèmes non critiques
    Error,   // Erreurs
}

#[derive(Clone, Debug)]
pub struct EventWithLevel {
    level: EventLevel,
    message: String,
    timestamp: DTChatTime,
}

pub struct TerminalScreen {
    local_uuid: String,
    messages: VecDeque<ChatMessage>,
    network_events: VecDeque<EventWithLevel>, // Événements réseau
    app_events: VecDeque<EventWithLevel>,     // Événements d'application
    max_lines: usize,
    input_line: String,
}

impl TerminalScreen {
    pub fn new(local_uuid: String, max_lines: usize) -> Self {
        Self {
            local_uuid,
            messages: VecDeque::new(),
            network_events: VecDeque::new(),
            app_events: VecDeque::new(),
            max_lines,
            input_line: String::new(),
        }
    }

    pub fn set_input(&mut self, input: String) {
        self.input_line = input;
    }

    fn add_network_event(&mut self, level: EventLevel, message: String) {
        let event = EventWithLevel {
            level,
            message,
            timestamp: DTChatTime::now(),
        };
        self.network_events.push_back(event);

        // Garder seulement les 6 derniers événements réseau
        if self.network_events.len() > 10 {
            self.network_events.pop_front();
        }
    }

    fn add_app_event(&mut self, level: EventLevel, message: String) {
        let event = EventWithLevel {
            level,
            message,
            timestamp: DTChatTime::now(),
        };
        self.app_events.push_back(event);

        // Garder seulement les 6 derniers événements d'application
        if self.app_events.len() > 10 {
            self.app_events.pop_front();
        }
    }

    fn update_message_status(&mut self, msg: ChatMessage) {
        println!("{:?}", msg.get_shipment_status_otp());
        for m in &mut self.messages {
            if m.uuid == msg.uuid || m.uuid.starts_with(&msg.uuid) {
                *m = msg;
                break;
            }
        }
    }

    pub fn render(&self) {
        // Clear screen and move cursor to top
        print!("\x1b[2J\x1b[H");

        // Print header with connection info
        println!(
            "\x1b[1;96m=== DTChat - {} ===\x1b[0m",
            self.local_uuid.to_uppercase()
        );
        println!();

        // Layout en deux colonnes : Messages | Events
        // === SECTION MESSAGES (Colonne gauche) ===
        println!("\x1b[1;37mMessages ({}):\x1b[0m", self.messages.len());

        if self.messages.is_empty() {
            println!("  \x1b[90mEmpty chat\x1b[0m");
        } else {
            // Afficher les 8 derniers messages
            for msg in self.messages.iter().rev().take(8).rev() {
                let (status_indicator, status_color) = match &msg.status {
                    MessageStatus::Failed => ("FAILED", "\x1b[31m"),
                    MessageStatus::ReceivedByPeer => ("ACKED", "\x1b[32m"),
                    MessageStatus::Sent => ("SENT", "\x1b[33m"),
                    MessageStatus::Sending => ("SENDING", "\x1b[90m"),
                    MessageStatus::Received => ("RECEIVED", "\x1b[34m"),
                };

                // Nouveau format : [<STATUS>] [acked_time:send_time] <message>
                let acked_time_str = match msg.receive_time {
                    Some(t) => t.ts_to_str(false, true, None, &chrono::Local),
                    None => "???".to_string(),
                };

                let send_time_str: String =
                    msg.send_time.ts_to_str(false, true, None, &chrono::Local);

                let time_display = format!("[{}:{}]", send_time_str, acked_time_str);

                let msg_color = if msg.sender_uuid == self.local_uuid {
                    "\x1b[37m"
                } else {
                    "\x1b[34m"
                };

                let display_text = if msg.text.len() > 40 {
                    format!("{}...", &msg.text[..37])
                } else {
                    msg.text.clone()
                };

                println!(
                    "  {}[{}] {} {}{}\x1b[0m",
                    status_color, status_indicator, time_display, msg_color, display_text
                );
            }
        }

        println!();

        // === SECTION ÉVÉNEMENTS RÉSEAU ===
        println!(
            "\x1b[1;37mNetwork Events ({}):\x1b[0m",
            self.network_events.len()
        );

        if self.network_events.is_empty() {
            println!("  \x1b[90mAucun événement réseau\x1b[0m");
        } else {
            // Afficher les 6 derniers événements réseau
            for event in self.network_events.iter().rev().take(6).rev() {
                let (level_indicator, color) = match event.level {
                    EventLevel::Info => ("[INFO ]", "\x1b[36m"),    // Cyan
                    EventLevel::Debug => ("[DEBUG]", "\x1b[90m"),   // Gris
                    EventLevel::Warning => ("[WARN ]", "\x1b[33m"), // Jaune
                    EventLevel::Error => ("[ERROR]", "\x1b[31m"),   // Rouge
                };

                let time_str = event.timestamp.ts_to_str(false, true, None, &chrono::Local);

                println!(
                    "  {}{} [{}] {}\x1b[0m",
                    color, level_indicator, time_str, event.message
                );
            }
        }

        println!();

        // === SECTION ÉVÉNEMENTS D'APPLICATION ===
        println!("\x1b[1;37mApp Events ({}):\x1b[0m", self.app_events.len());

        if self.app_events.is_empty() {
            println!("  \x1b[90mAucun événement d'application\x1b[0m");
        } else {
            // Afficher les 6 derniers événements d'application
            for event in self.app_events.iter().rev().take(6).rev() {
                let (level_indicator, color) = match event.level {
                    EventLevel::Info => ("[INFO ]", "\x1b[36m"),    // Cyan
                    EventLevel::Debug => ("[DEBUG]", "\x1b[90m"),   // Gris
                    EventLevel::Warning => ("[WARN ]", "\x1b[33m"), // Jaune
                    EventLevel::Error => ("[ERROR]", "\x1b[31m"),   // Rouge
                };

                let time_str = event.timestamp.ts_to_str(false, true, None, &chrono::Local);

                println!(
                    "  {}{} [{}] {}\x1b[0m",
                    color, level_indicator, time_str, event.message
                );
            }
        }

        let used_lines = 8
            + self.messages.len().min(8).max(1)
            + self.network_events.len().min(6).max(1)
            + self.app_events.len().min(6).max(1);

        let remaining_lines = self.max_lines.saturating_sub(used_lines + 2); // +2 pour input
        for _ in 0..remaining_lines {
            println!();
        }

        // === LIGNE D'INPUT EN BAS ===
        println!("\x1b[1;96m─────────────────────────────────────────────────────────────────────────────\x1b[0m");
        print!("\x1b[1;37m> \x1b[0m{}", self.input_line);

        io::stdout().flush().unwrap();
    }
}

impl AppEventObserver for TerminalScreen {
    fn on_event(&mut self, event: ChatAppEvent) {
        match event {
            ChatAppEvent::SocketEngineInfo(info_event) => {
                let (level, event_text) = match info_event {
                    NetworkEvent::Data(data_event) => match data_event {
                        DataEvent::Received { data, from } => (
                            EventLevel::Info,
                            format!("Received {} bytes from {}", data.len(), from.to_string()),
                        ),
                        DataEvent::Sent {
                            token,
                            to,
                            bytes_sent,
                        } => (
                            EventLevel::Info,
                            format!(
                                "Sent {} bytes to {} (token: {})",
                                bytes_sent,
                                to.to_string(),
                                safe_message_id_display(&token)
                            ),
                        ),
                        DataEvent::Sending { token, to, bytes } => (
                            EventLevel::Info,
                            format!(
                                "Sending {} bytes to {} (token: {})",
                                bytes,
                                to.to_string(),
                                safe_message_id_display(&token)
                            ),
                        ),
                    },
                    NetworkEvent::Connection(connection_event) => match connection_event {
                        ConnectionEvent::ListenerStarted { endpoint } => (
                            EventLevel::Info,
                            format!("Listening on {}", endpoint.to_string()),
                        ),
                        ConnectionEvent::Established { remote } => {
                            // Extraire seulement l'adresse IP:port du remote endpoint
                            let client_addr = remote.endpoint;
                            (
                                EventLevel::Debug,
                                format!("Connection established (client: {})", client_addr),
                            )
                        }
                        ConnectionEvent::Closed { remote } => {
                            let message = match remote {
                                Some(remote_ep) => {
                                    let client_addr = remote_ep.endpoint;
                                    format!("Connection closed (client: {})", client_addr)
                                }
                                None => "Connection closed (no client info)".to_string(),
                            };
                            (EventLevel::Debug, message)
                        }
                    },
                };

                self.add_network_event(level, event_text);
            }
            ChatAppEvent::SocketEngineError(error_event) => {
                let error_text = match error_event {
                    NetworkErrorEvent::SocketError(socket_error) => {
                        format!("Socket error: {:?}", socket_error)
                    }
                };
                self.add_network_event(EventLevel::Error, error_text);
            }
            ChatAppEvent::Message(info_event) => match info_event {
                ChatAppInfoEvent::Sending(chat_message) => {
                    let msg_id = safe_message_id_display(&chat_message.uuid);
                    self.add_app_event(EventLevel::Info, format!("Sending message {}", msg_id));

                    if !self.messages.iter().any(|m| m.uuid == chat_message.uuid) {
                        self.messages.push_back(chat_message);
                        if self.messages.len() > self.max_lines {
                            self.messages.pop_front();
                        }
                    }
                }
                ChatAppInfoEvent::Sent(sent_message) => {
                    self.update_message_status(sent_message);
                }
                ChatAppInfoEvent::Received(chat_message) => {
                    let uuid = chat_message.uuid.clone();
                    let msg_id = safe_message_id_display(&uuid);
                    self.update_message_status(chat_message.clone());
                    self.add_app_event(EventLevel::Info, format!("Message {} received", msg_id));
                    if !self.messages.iter().any(|m| m.uuid == chat_message.uuid) {
                        self.messages.push_back(chat_message);
                        if self.messages.len() > self.max_lines {
                            self.messages.pop_front();
                        }
                    }
                }
                ChatAppInfoEvent::AckSent(msg, _peer_uuid) => {
                    let uuid = msg.uuid.clone();
                    self.update_message_status(msg);
                    let msg_id = safe_message_id_display(&uuid);
                    self.add_app_event(
                        EventLevel::Info,
                        format!("Ack sent for message {}", msg_id),
                    );
                }
                ChatAppInfoEvent::AckReceived(msg) => {
                    let uuid = msg.uuid.clone();
                    self.update_message_status(msg);
                    let msg_id = safe_message_id_display(&uuid);
                    self.add_app_event(
                        EventLevel::Info,
                        format!("Ack received for message {}", msg_id),
                    );
                }
            },
            ChatAppEvent::Error(error_event) => {
                let error_text = match error_event {
                    ChatAppErrorEvent::ProtocolDecode(details) => {
                        format!("Failed to decode protobuf: {}", details)
                    }
                    ChatAppErrorEvent::ProtocolEncode(details) => {
                        format!("Failed to encode protobuf: {}", details)
                    }
                    ChatAppErrorEvent::InvalidMessage(details) => {
                        format!("Invalid message format: {}", details)
                    }
                    ChatAppErrorEvent::MessageNotFound(msg_id) => {
                        format!("Message {} not found in history", msg_id)
                    }
                    ChatAppErrorEvent::PeerNotFound(peer_id) => {
                        format!("Unknown peer: {}", peer_id)
                    }
                    ChatAppErrorEvent::NoEngineAttached => {
                        "Network engine not available".to_string()
                    }
                    ChatAppErrorEvent::InternalError(details) => {
                        format!("Internal error: {}", details)
                    }
                };

                self.add_app_event(EventLevel::Error, error_text);
            }
            ChatAppEvent::Info(info) => self.add_app_event(EventLevel::Error, info),
        }

        self.render();
    }
}

fn main() {
    let view_height: usize = 10;

    let chat_model = Arc::new(Mutex::new(ChatModel::new()));
    let mut network_engine = Engine::new();
    let local_peer = chat_model.lock().unwrap().get_localpeer();
    let binding = chat_model.lock().unwrap().get_other_peers();
    let distant_peer = binding.iter().next().unwrap().1;

    network_engine.add_observer(chat_model.clone());
    let screen = Arc::new(Mutex::new(TerminalScreen::new(
        local_peer.uuid,
        view_height,
    )));

    chat_model.lock().unwrap().add_observer(screen.clone());
    chat_model.lock().unwrap().start(network_engine);

    loop {
        screen.lock().unwrap().render();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_ok() {
            let input = input.trim();
            if input == "quit" || input == "exit" {
                break;
            }
            if !input.is_empty() {
                chat_model.lock().unwrap().send_to_peer(
                    &input.to_string(),
                    &"room".to_string(),
                    distant_peer.uuid.clone(),
                    &distant_peer.endpoints[0],
                    false,
                );
                // // Room message test
                // chat_model.lock().unwrap().send_to_room(
                //     &input.to_string(),
                //     &"1".to_string(),
                //     false,
                // );
            }
        }
    }
}
