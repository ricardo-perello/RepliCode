use std::io::Write;
use log::error;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkOperation {
    Connect {
        dest_addr: String,
        dest_port: u16,
        src_port: u16,
    },
    Send {
        src_port: u16,
        data: Vec<u8>,
    },
    Close {
        src_port: u16,
    },
    Listen {
        src_port: u16,
    },
    Accept {
        src_port: u16,
        new_port: u16,  // Port for the new accepted connection
    }
}

/// High-level command variants.
#[derive(Clone, Debug)]
pub enum Command {
    Clock(u64),
    Init {
        wasm_bytes: Vec<u8>,
        dir_path: Option<String>,
        args: Vec<String>
    },
    FDMsg(u64, Vec<u8>),
    NetworkIn(u64, u16, Vec<u8>),  // pid, dest_port, data
    NetworkOut(u64, NetworkOperation), // pid, operation
}

/// Reads a WASM file from disk.
pub fn read_wasm_file(file_path: &str) -> Vec<u8> {
    std::fs::read(file_path).unwrap_or_else(|e| {
        error!("Error reading WASM file {}: {}", file_path, e);
        Vec::new()
    })
}

/// Parse a text command into a high-level Command.
/// Supported commands:
///   - init <wasm_file> [-d directory] [-a 'arg1 arg2 ...']
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
            if tokens.len() < 2 {
                error!("Usage: init <wasm_file> [-d directory] [-a 'arg1 arg2 ...']");
                return None;
            }
            
            let file_path = tokens[1].to_string();
            let wasm_bytes = read_wasm_file(&file_path);
            
            let mut dir_path = None;
            let mut args = Vec::new();
            let mut i = 2;
            
            while i < tokens.len() {
                match tokens[i] {
                    "-d" => {
                        if i + 1 < tokens.len() {
                            dir_path = Some(tokens[i + 1].to_string());
                            i += 2;
                        } else {
                            error!("-d flag requires a directory path");
                            return None;
                        }
                    },
                    "-a" => {
                        if i + 1 < tokens.len() {
                            // Take everything after -a as a single string and split it
                            let args_str = tokens[i + 1];
                            // Remove surrounding quotes if present
                            let args_str = args_str.trim_matches(|c| c == '"' || c == '\'');
                            args = args_str.split_whitespace()
                                .map(|s| s.to_string())
                                .collect();
                            i += 2;
                        } else {
                            error!("-a flag requires arguments");
                            return None;
                        }
                    },
                    _ => {
                        error!("Unknown flag: {}", tokens[i]);
                        return None;
                    }
                }
            }
            
            Some(Command::Init { wasm_bytes, dir_path, args })
        },
        "msg" => {
            // "msg <pid> <message>"
            if tokens.len() < 3 {
                error!("Usage: msg <pid> <message>");
                return None;
            }
            let pid = tokens[1].parse::<u64>().unwrap_or(0);
            let message = tokens[2..].join(" ");
            Some(Command::FDMsg(pid, message.into_bytes()))
        },
        "clock" => {
            // "clock <nanoseconds>"
            if tokens.len() < 2 {
                error!("Usage: clock <nanoseconds>");
                return None;
            }
            let delta = tokens[1].parse::<u64>().unwrap_or(0);
            Some(Command::Clock(delta))
        },
        _ => {
            error!("Unknown command. Use 'init', 'msg', 'ftp', or 'clock'.");
            None
        }
    }
}