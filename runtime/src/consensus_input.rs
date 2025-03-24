use anyhow::Result;
use std::io::{BufReader, Read};
use std::fs::File;
use byteorder::{LittleEndian, ReadBytesExt};
use log::{info, error};
use std::sync::atomic::{AtomicU64, Ordering};
use crate::runtime::clock::GlobalClock;
use crate::runtime::process;
// Use an AtomicU64 for generating unique process IDs.
static NEXT_PID: AtomicU64 = AtomicU64::new(1);

fn get_next_pid() -> u64 {
    NEXT_PID.fetch_add(1, Ordering::SeqCst)
}

/// Reads new records from a live consensus pipe/socket for one batch only.
/// 
/// Record format (total header: 1 byte msg_type, 8 bytes process_id, 2 bytes payload length):
///   [ msg_type: u8 ][ process_id: u64 ][ payload_length: u16 ][ payload: [u8; payload_length] ]
///
/// Supported message types:
/// - **0**: Clock update. The payload must start with `"clock:"` followed by the nanoseconds value.
/// - **1**: FD update. The payload is expected to be `"fd:<number>,body:<data>"`.
/// - **2**: Init command. The payload is a WASM binary; a new process is created.
/// - **3**: Msg command. The payload is expected to be `"msg:<message>"` (or just a message),
///        and the message is sent (for example, to FD 0).
/// - **4**: FTP update. (Logic to dispatch the FTP command can be added.)
pub fn process_consensus_pipe<R: Read>(consensus_pipe: &mut R, processes: &mut Vec<process::Process>) -> Result<()> {
    let mut reader = BufReader::new(consensus_pipe);

    loop {
        // Read the message type (1 byte)
        let mut msg_type_buf = [0u8; 1];
        if reader.read_exact(&mut msg_type_buf).is_err() {
            break; // No more data.
        }
        let msg_type = msg_type_buf[0];

        // Read process_id (8 bytes)
        let process_id = match reader.read_u64::<LittleEndian>() {
            Ok(pid) => pid,
            Err(_) => break,
        };

        // Read payload length (2 bytes)
        let payload_len = match reader.read_u16::<LittleEndian>() {
            Ok(sz) => sz as usize,
            Err(_) => break,
        };

        // Read the payload.
        let mut payload = vec![0u8; payload_len];
        if let Err(e) = reader.read_exact(&mut payload) {
            error!("Failed to read message from pipe: {}", e);
            break;
        }
        let msg_str = match msg_type {
            0 | 1 | 4 => { // Clock, FD, and FTP messages are text
                match String::from_utf8(payload.clone()) {
                    Ok(s) => s,
                    Err(e) => {
                        error!("Failed to decode pipe message as UTF-8: {}", e);
                        continue;
                    }
                }
            },
            2 => { // Init command - payload is binary WASM
                String::new() // We don't need the string for WASM binary
            },
            _ => {
                error!("Unknown message type: {}", msg_type);
                continue;
            }
        };

        match msg_type {
            0 => { // Clock update.
                if let Some(delta_str) = msg_str.strip_prefix("clock:") {
                    match delta_str.trim().parse::<u64>() {
                        Ok(delta) => {
                            GlobalClock::increment(delta);
                            info!("Global clock incremented by {}", delta);
                        }
                        Err(e) => error!("Invalid clock increment in pipe: {}", e),
                    }
                } else {
                    error!("Invalid clock message format in pipe: {}", msg_str);
                }
                break; // End of batch.
            },
            1 => { // FD update. Expected format: "fd:<number>,body:<data>"
                let parts: Vec<&str> = msg_str.split(",body:").collect();
                if parts.len() != 2 {
                    error!("Invalid pipe message format for FD update: {}", msg_str);
                    continue;
                }
                let fd: i32 = if let Some(fd_part) = parts[0].strip_prefix("fd:") {
                    match fd_part.trim().parse() {
                        Ok(num) => num,
                        Err(_) => {
                            error!("Invalid FD in pipe message: {}", msg_str);
                            continue;
                        }
                    }
                } else {
                    error!("Missing FD prefix in pipe message: {}", msg_str);
                    continue;
                };
                let body = parts[1].trim();
                let mut found = false;
                for process in processes.iter_mut() {
                    if process.id == process_id {
                        found = true;
                        let mut table = process.data.fd_table.lock().unwrap();
                        if let Some(Some(fd_entry)) = table.entries.get_mut(fd as usize) {
                            fd_entry.buffer.extend_from_slice(body.as_bytes());
                            fd_entry.buffer.push(b'\n');
                            info!("Added input to process {}'s FD {} (via pipe)", process_id, fd);
                        } else {
                            error!("Process {} does not have FD {} open (via pipe)", process_id, fd);
                        }
                        process.data.cond.notify_all();
                        break;
                    }
                }
                if !found {
                    error!("No process found with ID {} (via pipe)", process_id);
                }
            },
            2 => { // Init command.
                info!("Received init command from consensus.");
                let new_pid = get_next_pid();
                // For the init command, the payload is a WASM binary.
                // Use the raw payload bytes directly, not the empty msg_str
                let proc = process::start_process_from_bytes(payload, new_pid)?;
                processes.push(proc);
                info!("Added new process {} to scheduler", new_pid);
            },
            3 => { // Msg command.
                // Expected format: "msg:<message>" or just the message.
                let message = if let Some(msg_part) = msg_str.strip_prefix("msg:") {
                    msg_part.trim()
                } else {
                    msg_str.trim()
                };
                let mut found = false;
                for process in processes.iter_mut() {
                    if process.id == process_id {
                        found = true;
                        // For this example, send the message to FD 0.
                        let mut table = process.data.fd_table.lock().unwrap();
                        if let Some(Some(fd_entry)) = table.entries.get_mut(0) {
                            fd_entry.buffer.extend_from_slice(message.as_bytes());
                            fd_entry.buffer.push(b'\n');
                            info!("Added msg to process {}'s FD 0", process_id);
                        } else {
                            error!("Process {} does not have FD 0 open for msg", process_id);
                        }
                        process.data.cond.notify_all();
                        break;
                    }
                }
                if !found {
                    error!("No process found with ID {} for msg", process_id);
                }
            },
            4 => { // FTP update.
                info!("Received FTP command for process {}: {}", process_id, msg_str);
                // Insert logic here to dispatch the FTP command to the process.
            },
            _ => {
                error!("Unknown message type: {} in message: {}", msg_type, msg_str);
            }
        }
    }
    Ok(())
}

pub fn process_consensus_file(file_path: &str, processes: &mut Vec<process::Process>) -> Result<()> {
    let file = File::open(file_path)?;
    let mut reader = BufReader::new(file);

    // Read the message type (1 byte)
    let mut msg_type_buf = [0u8; 1];
    if reader.read_exact(&mut msg_type_buf).is_err() {
        return Ok(()); // No more data, exit gracefully
    }
    let msg_type = msg_type_buf[0];

    // Read process_id (8 bytes)
    let process_id = match reader.read_u64::<LittleEndian>() {
        Ok(pid) => pid,
        Err(_) => return Ok(()), // End of file
    };

    // Read payload length (2 bytes)
    let payload_len = match reader.read_u16::<LittleEndian>() {
        Ok(sz) => sz as usize,
        Err(_) => return Ok(()), // End of file
    };

    // Read the payload.
    let mut payload = vec![0u8; payload_len];
    if let Err(e) = reader.read_exact(&mut payload) {
        error!("Failed to read message from file: {}", e);
        return Ok(());
    }

    // Convert payload to a string for text-based messages.
    let msg_str = match msg_type {
        0 | 1 | 4 => {
            match String::from_utf8(payload.clone()) {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to decode file message as UTF-8: {}", e);
                    return Ok(());
                }
            }
        },
        2 => String::new(), // For Init command, the payload is binary.
        _ => {
            error!("Unknown message type: {} in file", msg_type);
            return Ok(());
        }
    };

    match msg_type {
        0 => { // Clock update.
            if let Some(delta_str) = msg_str.strip_prefix("clock:") {
                match delta_str.trim().parse::<u64>() {
                    Ok(delta) => {
                        GlobalClock::increment(delta);
                        info!("Global clock incremented by {} (via file)", delta);
                    }
                    Err(e) => error!("Invalid clock increment in file: {}", e),
                }
            } else {
                error!("Invalid clock message format in file: {}", msg_str);
            }
        },
        1 => { // FD update.
            let parts: Vec<&str> = msg_str.split(",body:").collect();
            if parts.len() != 2 {
                error!("Invalid file message format for FD update: {}", msg_str);
                return Ok(());
            }
            let fd: i32 = if let Some(fd_part) = parts[0].strip_prefix("fd:") {
                match fd_part.trim().parse() {
                    Ok(num) => num,
                    Err(_) => {
                        error!("Invalid FD in file message: {}", msg_str);
                        return Ok(());
                    }
                }
            } else {
                error!("Missing FD prefix in file message: {}", msg_str);
                return Ok(());
            };
            let body = parts[1].trim();
            let mut found = false;
            for process in processes.iter_mut() {
                if process.id == process_id {
                    found = true;
                    let mut table = process.data.fd_table.lock().unwrap();
                    if let Some(Some(fd_entry)) = table.entries.get_mut(fd as usize) {
                        fd_entry.buffer.extend_from_slice(body.as_bytes());
                        fd_entry.buffer.push(b'\n');
                        info!(
                            "Added input to process {}'s FD {} (via file)",
                            process_id, fd
                        );
                    } else {
                        error!(
                            "Process {} does not have FD {} open (via file)",
                            process_id, fd
                        );
                    }
                    process.data.cond.notify_all();
                    break;
                }
            }
            if !found {
                error!("No process found with ID {} (via file)", process_id);
            }
        },
        2 => { // Init command.
            info!("Received init command from consensus file.");
            let new_pid = get_next_pid(); // Assumes get_next_pid is public.
            let proc = process::start_process_from_bytes(payload, new_pid)?;
            processes.push(proc);
            info!("Added new process {} to scheduler (via file)", new_pid);
        },
        3 => { // Msg command.
            let message = if let Some(msg_part) = msg_str.strip_prefix("msg:") {
                msg_part.trim()
            } else {
                msg_str.trim()
            };
            let mut found = false;
            for process in processes.iter_mut() {
                if process.id == process_id {
                    found = true;
                    let mut table = process.data.fd_table.lock().unwrap();
                    if let Some(Some(fd_entry)) = table.entries.get_mut(0) {
                        fd_entry.buffer.extend_from_slice(message.as_bytes());
                        fd_entry.buffer.push(b'\n');
                        info!(
                            "Added msg to process {}'s FD 0 (via file)",
                            process_id
                        );
                    } else {
                        error!(
                            "Process {} does not have FD 0 open for msg (via file)",
                            process_id
                        );
                    }
                    process.data.cond.notify_all();
                    break;
                }
            }
            if !found {
                error!("No process found with ID {} for msg (via file)", process_id);
            }
        },
        4 => { // FTP update.
            info!(
                "Received FTP command for process {}: {} (via file)",
                process_id, msg_str
            );
            // Add FTP command dispatch logic here if needed.
        },
        _ => {
            error!("Unknown message type: {} in file message: {}", msg_type, msg_str);
        }
    }
    Ok(())
}
