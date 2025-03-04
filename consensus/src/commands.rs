use std::io::{self, Write};

/// A reserved process ID to indicate an "init" (process initialization) request.
pub const INIT_REQUEST: u64 = u64::MAX;

/// Commands that the consensus tool can process.
pub enum Command {
    /// An init command with the binary content of the WASM file.
    Init(Vec<u8>),
    /// A normal message directed to a specific process.
    Msg(u64, String),
}

/// Reads a WASM file from disk given a file path and returns its bytes.
pub fn read_wasm_file(file_path: &str) -> Vec<u8> {
    match std::fs::read(file_path) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Error reading WASM file {}: {}", file_path, e);
            Vec::new()
        }
    }
}

/// Parse a command line into a Command.
/// Accepted formats:
///   - "init <wasm_file_path>"   (to request process initialization using a specific WASM file)
///   - "msg <pid> <text>"         (to send a message to an existing process)
///   - "exit" is handled in the caller.
pub fn parse_command(line: &str) -> Option<Command> {
    let trimmed = line.trim();
    if trimmed.eq_ignore_ascii_case("exit") {
        return None;
    }
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    if tokens.is_empty() {
        return Some(Command::Msg(0, String::new()));
    }
    match tokens[0].to_lowercase().as_str() {
        "init" => {
            // existing init command handling...
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
            if tokens.len() < 3 {
                eprintln!("Usage: msg <pid> <message>");
                return Some(Command::Msg(0, String::new()));
            }
            let pid = match tokens[1].parse::<u64>() {
                Ok(num) => num,
                Err(_) => {
                    eprintln!("Invalid PID: {}", tokens[1]);
                    return Some(Command::Msg(0, String::new()));
                }
            };
            let message = tokens[2..].join(" ");
            Some(Command::Msg(pid, message))
        },
        "clock" => {
            if tokens.len() < 2 {
                eprintln!("Usage: clock <time>");
                return Some(Command::Msg(0, String::new()));
            }
            let time_value = tokens[1];
            // Create a clock record with process ID 0.
            let message = format!("clock:{}", time_value);
            Some(Command::Msg(0, message))
        },
        _ => {
            eprintln!("Unknown command. Use 'init <wasm_file_path>', 'msg <pid> <message>', or 'clock <time>'.");
            Some(Command::Msg(0, String::new()))
        }
    }
}

