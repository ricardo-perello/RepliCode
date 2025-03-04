use std::io::{self, Write};


/// A network message structure.
#[derive(Clone, Debug)]
pub struct NetworkMessage {
    pub src: u64,
    pub dst: u64,
    pub payload: Vec<u8>,
}

/// High-level command variants.
#[derive(Clone, Debug)]
pub enum Command {
    /// Clock update (nanoseconds).
    Clock(u64),
    /// New WASM binary for process initialization.
    Init(Vec<u8>),
    /// File-descriptor based message: target process ID and binary payload.
    FDMsg(u64, Vec<u8>),
    /// Network message for NAT routing.
    NetMsg(NetworkMessage),
}

/// Reads a WASM file from disk.
pub fn read_wasm_file(file_path: &str) -> Vec<u8> {
    std::fs::read(file_path).unwrap_or_else(|e| {
        eprintln!("Error reading WASM file {}: {}", file_path, e);
        Vec::new()
    })
}

/// Parse a text command into a high-level Command.
/// Supported commands (typed as text on the terminal):
///   - init <wasm_file_path>
///   - msg <pid> <message>
///   - clock <nanoseconds>
///   - net <src> <dst> <payload>
pub fn parse_command(line: &str) -> Option<Command> {
    let trimmed = line.trim();
    if trimmed.eq_ignore_ascii_case("exit") {
        return None;
    }
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    if tokens.is_empty() {
        return Some(Command::FDMsg(0, Vec::new()));
    }
    match tokens[0].to_lowercase().as_str() {
        "init" => {
            // "init <wasm_file_path>"
            let file_path = if tokens.len() >= 2 {
                tokens[1].to_string()
            } else {
                eprint!("Enter WASM file path: ");
                io::stderr().flush().ok()?;
                let mut input = String::new();
                io::stdin().read_line(&mut input).ok()?;
                input.trim().to_string()
            };
            let wasm_bytes = read_wasm_file(&file_path);
            Some(Command::Init(wasm_bytes))
        },
        "msg" => {
            // "msg <pid> <message>"
            if tokens.len() < 3 {
                eprintln!("Usage: msg <pid> <message>");
                return Some(Command::FDMsg(0, Vec::new()));
            }
            let pid = tokens[1].parse::<u64>().unwrap_or(0);
            let message = tokens[2..].join(" ").into_bytes();
            Some(Command::FDMsg(pid, message))
        },
        "clock" => {
            // "clock <nanoseconds>"
            if tokens.len() < 2 {
                eprintln!("Usage: clock <nanoseconds>");
                return Some(Command::FDMsg(0, Vec::new()));
            }
            let delta = tokens[1].parse::<u64>().unwrap_or(0);
            Some(Command::Clock(delta))
        },
        "net" => {
            // "net <src> <dst> <payload>"
            if tokens.len() < 4 {
                eprintln!("Usage: net <src> <dst> <payload>");
                return Some(Command::FDMsg(0, Vec::new()));
            }
            let src = tokens[1].parse::<u64>().unwrap_or(0);
            let dst = tokens[2].parse::<u64>().unwrap_or(0);
            let payload = tokens[3..].join(" ").into_bytes();
            Some(Command::NetMsg(NetworkMessage { src, dst, payload }))
        },
        _ => {
            eprintln!("Unknown command. Use 'init', 'msg', 'clock', or 'net'.");
            Some(Command::FDMsg(0, Vec::new()))
        }
    }
}
