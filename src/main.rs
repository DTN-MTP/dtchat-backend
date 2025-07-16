use std::{
    env,
    sync::{Arc, Mutex},
};

use dtchat_backend::{
    dtchat::{ChatModel, Peer, extract_message_id_from_data},
    event::{
        AppEventObserver, ChatAppErrorEvent, ChatAppEvent, ChatAppInfoEvent, NetworkErrorEvent,
        NetworkEvent,
    },
    message::{ChatMessage, MessageStatus},
};
use socket_engine::{
    endpoint::Endpoint,
    engine::Engine,
    event::{ConnectionEvent, DataEvent},
};

use chrono::{DateTime, Utc};
use std::collections::VecDeque;
use std::io::{self, Write};

// Helper function to format endpoints in a readable way
fn format_endpoint(endpoint: &Endpoint) -> String {
    match endpoint {
        Endpoint::Tcp(addr) => format!("TCP {}", addr),
        Endpoint::Udp(addr) => format!("UDP {}", addr),
        Endpoint::Bp(addr) => format!("BP {}", addr),
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
    timestamp: DateTime<Utc>,
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
            timestamp: Utc::now(),
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
            timestamp: Utc::now(),
        };
        self.app_events.push_back(event);

        // Garder seulement les 6 derniers événements d'application
        if self.app_events.len() > 10 {
            self.app_events.pop_front();
        }
    }

    fn update_message_status(&mut self, msg_id: &str, new_status: MessageStatus) {
        for m in &mut self.messages {
            if m.uuid == msg_id || m.uuid.starts_with(msg_id) {
                m.status = new_status;
            }
        }
    }

    pub fn render(&self) {
        // Clear screen and move cursor to top
        print!("\x1b[2J\x1b[H");

        // Print header with connection info
        println!("\x1b[1;96m╔══════════════════════════════════════════════════════════════════════════════╗");
        println!(
            "\x1b[1;96m║                       DTChat - {:<40} ║",
            self.local_uuid.to_uppercase()
        );
        println!("\x1b[1;96m╚══════════════════════════════════════════════════════════════════════════════╝\x1b[0m");
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
                    MessageStatus::Acknowledged => ("ACKED", "\x1b[32m"),
                    MessageStatus::Sent => ("SENT", "\x1b[33m"),
                    MessageStatus::Sending => ("SENDING", "\x1b[90m"),
                    MessageStatus::Received => ("RECEIVED", "\x1b[34m"),
                };

                // Nouveau format : [<STATUS>] [acked_time:send_time] <message>
                let acked_time_str = match msg.send_completed {
                    Some(t) => t.format("%H:%M:%S").to_string(),
                    None => String::new(),
                };
       
                let send_time_str: String = msg.send_time.format("%H:%M:%S").to_string();

                let time_display = format!("[{}:{}]", acked_time_str, send_time_str);

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

                let time_str = event.timestamp.format("%H:%M:%S").to_string();

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

                let time_str = event.timestamp.format("%H:%M:%S").to_string();

                println!(
                    "  {}{} [{}] {}\x1b[0m",
                    color, level_indicator, time_str, event.message
                );
            }
        }

        let used_lines = 8 + 
            self.messages.len().min(8).max(1) + 
            self.network_events.len().min(6).max(1) + 
            self.app_events.len().min(6).max(1);

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
                        DataEvent::Received { data, from } => {
                            let msg_id = extract_message_id_from_data(&data)
                                .unwrap_or_else(|| "unknown".to_string());
                            (
                                EventLevel::Info,
                                format!("Received message {} from {}", &msg_id[..8], format_endpoint(&from)),
                            )
                        },
                        DataEvent::Sent { message_id, to, bytes_sent } => (
                            EventLevel::Info,
                            format!("Sent message {} to {} ({} bytes)", &message_id[..8], format_endpoint(&to), bytes_sent),
                        ),
                    },
                    NetworkEvent::Connection(connection_event) => match connection_event {
                        ConnectionEvent::ListenerStarted { endpoint } => (
                            EventLevel::Info,
                            format!("Listening on {}", format_endpoint(&endpoint)),
                        ),
                        ConnectionEvent::Established { remote } => {
                            // Extraire seulement l'adresse IP:port du remote endpoint
                            let client_addr = match remote {
                                Endpoint::Tcp(addr) => addr,
                                Endpoint::Udp(addr) => addr,
                                Endpoint::Bp(addr) => addr,
                            };
                            (
                                EventLevel::Debug,
                                format!("Connection established (client: {})", client_addr),
                            )
                        },
                        ConnectionEvent::Closed { remote } => {
                            let message = match remote {
                                Some(remote_ep) => {
                                    let client_addr = match remote_ep {
                                        Endpoint::Tcp(addr) => addr,
                                        Endpoint::Udp(addr) => addr,
                                        Endpoint::Bp(addr) => addr,
                                    };
                                    format!("Connection closed (client: {})", client_addr)
                                },
                                None => "Connection closed (no client info)".to_string(),
                            };
                            (EventLevel::Debug, message)
                        },
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
            ChatAppEvent::Info(info_event) => match info_event {
                ChatAppInfoEvent::Sending(chat_message) => {
                    let msg_id = &chat_message.uuid[..8]; // Premier 8 caractères de l'UUID
                    self.add_app_event(
                        EventLevel::Info,
                        format!("Sending message {}", msg_id),
                    );

                    if !self.messages.iter().any(|m| m.uuid == chat_message.uuid) {
                        self.messages.push_back(chat_message);
                        if self.messages.len() > self.max_lines {
                            self.messages.pop_front();
                        }
                    }
                }
                ChatAppInfoEvent::Sent(sent_message) => {
                    self.update_message_status(&sent_message.uuid, MessageStatus::Sent);
                    for m in &mut self.messages {
                        if m.uuid == sent_message.uuid {
                            m.send_completed = sent_message.send_completed;
                            break;
                        }
                    }
                }
                ChatAppInfoEvent::Received(chat_message) => {
                    let msg_id = &chat_message.uuid[..8]; // Premier 8 caractères de l'UUID
                    self.update_message_status(&chat_message.uuid, MessageStatus::Received);
                    self.add_app_event(EventLevel::Info, format!("Message {} received", msg_id));
                    if !self.messages.iter().any(|m| m.uuid == chat_message.uuid) {
                        self.messages.push_back(chat_message);
                        if self.messages.len() > self.max_lines {
                            self.messages.pop_front();
                        }
                    }
                }
                ChatAppInfoEvent::AckSent(uuid, _peer_uuid) => {
                    self.update_message_status(&uuid, MessageStatus::Acknowledged);
                    let msg_id = &uuid[..8]; // Premier 8 caractères de l'UUID
                    self.add_app_event(
                        EventLevel::Info,
                        format!("Ack sent for message {}", msg_id),
                    );
                }
                ChatAppInfoEvent::AckReceived(uuid, _peer_uuid) => {
                    self.update_message_status(&uuid, MessageStatus::Acknowledged);
                    let msg_id = &uuid[..8]; // Premier 8 caractères de l'UUID
                    self.add_app_event(
                        EventLevel::Info,
                        format!("Ack received for message {}", msg_id),
                    );
                }
                ChatAppInfoEvent::MessageStatusChanged(uuid, new_status) => {
                    self.update_message_status(&uuid, new_status);
                    let msg_id = &uuid[..8]; // Premier 8 caractères de l'UUID
                    let status_text = match new_status {
                        MessageStatus::Sending => "sending",
                        MessageStatus::Sent => "sent",
                        MessageStatus::Received => "received", 
                        MessageStatus::Acknowledged => "acknowledged",
                        MessageStatus::Failed => "failed",
                    };
                    self.add_app_event(
                        EventLevel::Info,
                        format!("Message {} status changed to {}", msg_id, status_text),
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
                    ChatAppErrorEvent::NoEngineAttached => "Network engine not available".to_string(),
                    ChatAppErrorEvent::InternalError(details) => {
                        format!("Internal error: {}", details)
                    }
                    ChatAppErrorEvent::HostNotReachable(endpoint) => {
                        format!("Host not reachable: {}", endpoint)
                    }
                };

                self.add_app_event(EventLevel::Error, error_text);
            }
        }

        self.render();
    }
}

fn main() {
    // --- 1) parse CLI argument
    let args: Vec<String> = env::args().collect();
    let mut view_height: usize = 10;
    if args.len() < 3 {
        eprintln!(
            "Usage: {} <local-endpoint> <distant-endpoint> [<view-height>, default: 10]",
            args[0]
        );
        std::process::exit(1);
    }

    let local_ep = match Endpoint::from_str(&args[1]) {
        Ok(ep) => ep,
        Err(e) => {
            eprintln!("Invalid local endpoint `{}`: {}", args[1], e);
            std::process::exit(1);
        }
    };
    let distant_ep = match Endpoint::from_str(&args[2]) {
        Ok(ep) => ep,
        Err(e) => {
            eprintln!("Invalid distant endpoint `{}`: {}", args[2], e);
            std::process::exit(1);
        }
    };

    if args.len() >= 4 {
        match &args[3].parse() {
            Ok(n) => view_height = *n,
            _ => {
                eprintln!("Error: '{}' is not a valid positive integer.", &args[3]);
                std::process::exit(1);
            }
        };
    }

    let local_peer = Peer {
        uuid: args[1].clone(),
        name: args[1].clone(),
        endpoints: vec![local_ep],
    };

    let distant_peer = Peer {
        uuid: args[2].clone(),
        name: args[2].clone(),
        endpoints: vec![distant_ep.clone()],
    };

    let chat_model = Arc::new(Mutex::new(ChatModel::new(
        local_peer.clone(),
        vec![distant_peer],
    )));
    let mut network_engine = Engine::new();
    network_engine.add_observer(chat_model.clone());
    let screen = Arc::new(Mutex::new(TerminalScreen::new(
        args[1].clone(),
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
                    &distant_ep,
                );
            }
        }
    }
}
