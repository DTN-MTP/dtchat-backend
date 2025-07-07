use std::{
    env,
    sync::{Arc, Mutex},
};

use dtchat_backend::{
    dtchat::{ChatModel, Peer},
    event::AppEventObserver,
    message::ChatMessage,
};
use socket_engine::{endpoint::Endpoint, engine::Engine};

use std::collections::VecDeque;
use std::io::{self, Write};

pub struct TerminalScreen {
    local_uuid: String,
    messages: VecDeque<ChatMessage>,
    status: String,
    max_lines: usize,
    input_line: String,
}

impl TerminalScreen {
    pub fn new(local_uuid: String, max_lines: usize) -> Self {
        Self {
            local_uuid,
            messages: VecDeque::new(),
            status: "Ready".to_string(),
            max_lines,
            input_line: String::new(),
        }
    }

    pub fn set_input(&mut self, input: String) {
        self.input_line = input;
    }

    pub fn render(&self) {
        // Clear screen and move cursor to top
        print!("\x1b[2J\x1b[H");

        // Print all messages
        for msg in &self.messages {
            if msg.sender_uuid == self.local_uuid {
                print!("[>|");
            } else {
                print!("[<|");
            }
            let formated_rx = if let Some(datetime) = msg.receive_time {
                datetime.format("%H:%M:%S").to_string()
            } else {
                "??".to_string()
            };
            let formated_tx = if let Some(datetime) = msg.send_completed {
                datetime.format("%H:%M:%S").to_string()
            } else {
                "??".to_string()
            };

            print!("{}", formated_tx);
            print!("|{}] ", formated_rx);
            println!("{}", msg.text);
        }

        // Fill remaining space if needed
        let remaining_lines = self.max_lines.saturating_sub(self.messages.len());
        for _ in 0..remaining_lines {
            println!();
        }
        // Print status line

        print!("Status: {}", self.status);
        println!();
        // Print input line at bottom
        print!("> {}", self.input_line);

        io::stdout().flush().unwrap();
    }
}

impl AppEventObserver for TerminalScreen {
    fn on_event(&mut self, event: dtchat_backend::event::AppEvent) {
        match event {
            dtchat_backend::event::AppEvent::Error(err) => self.status = err,
            dtchat_backend::event::AppEvent::Sending(chat_message) => {
                self.messages.push_back(chat_message);
                // Keep only max_lines messages
                if self.messages.len() > self.max_lines {
                    self.messages.pop_front();
                }
                self.status = "message sent".to_string()
            }
            dtchat_backend::event::AppEvent::Received(chat_message) => {
                self.messages.push_back(chat_message);
                // Keep only max_lines messages
                if self.messages.len() > self.max_lines {
                    self.messages.pop_front();
                }
                self.status = "message received".to_string()
            }
            dtchat_backend::event::AppEvent::Acked(ack_message) => {
                for m in &mut self.messages {
                    if m.uuid == ack_message.uuid {
                        m.receive_time = ack_message.receive_time;
                    }
                }
                self.status = "message sent received".to_string()
            }
            dtchat_backend::event::AppEvent::Sent(sent_message) => {
                for m in &mut self.messages {
                    if m.uuid == sent_message.uuid {
                        m.send_completed = sent_message.send_completed;
                    }
                }
            }
        }
        self.render();
    }
}

fn main() {
    // --- 1) parse CLI argument
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <local-endpoint> <distant-endpoint>", args[0]);
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
    let network_engine = Engine::new(chat_model.clone());

    let screen = Arc::new(Mutex::new(TerminalScreen::new(args[1].clone(), 10)));
    chat_model.lock().unwrap().add_observer(screen.clone());
    chat_model.lock().unwrap().start(network_engine);

    // Main input loop
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
