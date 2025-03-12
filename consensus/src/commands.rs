use std::io::Write;

/// High-level command variants.
#[derive(Clone, Debug)]
pub enum Command {
    Clock(u64),
    Init(Vec<u8>),
    FDMsg(u64, Vec<u8>),
    Ftp(u64, String), // New variant: includes a PID and FTP command string.
}

/// Reads a WASM file from disk.
pub fn read_wasm_file(file_path: &str) -> Vec<u8> {
    std::fs::read(file_path).unwrap_or_else(|e| {
        eprintln!("Error reading WASM file {}: {}", file_path, e);
        Vec::new()
    })
}
/// Parse a text command into a high-level Command.
/// Supported commands:
///   - init <wasm_file>
///   - msg <pid> <message>
///   - ftp <pid> <ftp_command>
///   - clock <nanoseconds>
pub fn parse_command(line: &str) -> Option<Command> {
    let trimmed = line.trim();
    if trimmed.eq_ignore_ascii_case("exit") {
        return None;
    }
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }
    match tokens[0].to_lowercase().as_str() {
        "init" => {
            // "init <wasm_file>"
            let file_path = if tokens.len() >= 2 {
                tokens[1].to_string()
            } else {
                eprint!("Enter WASM file path: ");
                std::io::stderr().flush().ok()?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input).ok()?;
                input.trim().to_string()
            };
            let wasm_bytes = read_wasm_file(&file_path);
            Some(Command::Init(wasm_bytes))
        },
        "msg" => {
            // "msg <pid> <message>"
            if tokens.len() < 3 {
                eprintln!("Usage: msg <pid> <message>");
                return None;
            }
            let pid = tokens[1].parse::<u64>().unwrap_or(0);
            let message = tokens[2..].join(" ");
            Some(Command::FDMsg(pid, message.into_bytes()))
        },
        "ftp" => {
            // "ftp <pid> <ftp_command>"
            if tokens.len() < 3 {
                eprintln!("Usage: ftp <pid> <ftp_command>");
                return None;
            }
            let pid = tokens[1].parse::<u64>().unwrap_or(0);
            let ftp_cmd = tokens[2..].join(" ");
            Some(Command::Ftp(pid, ftp_cmd))
        },
        "clock" => {
            // "clock <nanoseconds>"
            if tokens.len() < 2 {
                eprintln!("Usage: clock <nanoseconds>");
                return None;
            }
            let delta = tokens[1].parse::<u64>().unwrap_or(0);
            Some(Command::Clock(delta))
        },
        _ => {
            eprintln!("Unknown command. Use 'init', 'msg', 'ftp', or 'clock'.");
            None
        }
    }
}