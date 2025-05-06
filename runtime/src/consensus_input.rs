use anyhow::Result;
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::fs::File;
use byteorder::{LittleEndian, ReadBytesExt};
use log::{info, error, debug};
use std::sync::atomic::{AtomicU64, Ordering};
use crate::runtime::clock::GlobalClock;
use crate::runtime::process;
use crate::wasi_syscalls::net::OutgoingNetworkMessage;
use crate::runtime::fd_table::FDEntry;
use bincode;

// Use an AtomicU64 for generating unique process IDs.
static NEXT_PID: AtomicU64 = AtomicU64::new(1);
// Track file position for consensus file
static FILE_POSITION: AtomicU64 = AtomicU64::new(0);

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
/// - **5**: NetworkIn. The payload is expected to be a network message.
pub fn process_consensus_pipe<R: Read + Write>(
    consensus_pipe: &mut R, 
    processes: &mut Vec<process::Process>,
    outgoing_messages: Vec<OutgoingNetworkMessage>,
) -> Result<bool> {
    debug!("Processing consensus pipe with {} outgoing messages", outgoing_messages.len());
    let mut reader = BufReader::new(consensus_pipe);

    // First, send any outgoing network messages
    for msg in outgoing_messages {
        debug!("Sending outgoing network message for process {}: {:?}", msg.pid, msg.operation);
        // Write message type (NetworkOut = 5)
        reader.get_mut().write_all(&[5])?;
        
        // Write process ID
        reader.get_mut().write_all(&msg.pid.to_le_bytes())?;
        
        // Serialize and write the network operation
        let op_bytes = bincode::serialize(&msg.operation)?;
        reader.get_mut().write_all(&(op_bytes.len() as u32).to_le_bytes())?;
        reader.get_mut().write_all(&op_bytes)?;
    }

    loop {
        // Read the message type (1 byte)
        let mut msg_type_buf = [0u8; 1];
        if reader.read_exact(&mut msg_type_buf).is_err() {
            debug!("No more data in consensus pipe");
            break; // No more data.
        }
        let msg_type = msg_type_buf[0];
        debug!("Received message type {} from consensus pipe", msg_type);

        // Read process_id (8 bytes)
        let process_id = match reader.read_u64::<LittleEndian>() {
            Ok(pid) => pid,
            Err(_) => break,
        };

        // Read payload length (4 bytes)
        let payload_len = match reader.read_u32::<LittleEndian>() {
            Ok(sz) => sz as usize,
            Err(_) => break,
        };

        debug!("Reading payload of {} bytes for process {}", payload_len, process_id);

        // Read the payload.
        let mut payload = vec![0u8; payload_len];
        if let Err(e) = reader.read_exact(&mut payload) {
            error!("Failed to read message from pipe: {}", e);
            break;
        }

        match msg_type {
            0 => { // Clock update.
                let msg_str = String::from_utf8_lossy(&payload);
                debug!("Processing clock update: {}", msg_str);
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
            1 => { // FD update.
                let msg_str = String::from_utf8_lossy(&payload);
                debug!("Processing FD update for process {}: {}", process_id, msg_str);
                let parts: Vec<&str> = msg_str.split(",body:").collect();
                if parts.len() != 2 {
                    error!("Invalid FD update format for process {}: {}", process_id, msg_str);
                    continue;
                }
                let fd: i32 = if let Some(fd_part) = parts[0].strip_prefix("fd:") {
                    match fd_part.trim().parse() {
                        Ok(num) => num,
                        Err(_) => {
                            error!("Invalid FD in FD update for process {}: {}", process_id, msg_str);
                            continue;
                        }
                    }
                } else {
                    error!("Missing FD prefix in FD update for process {}: {}", process_id, msg_str);
                    continue;
                };
                let body = parts[1].trim();
                let mut found = false;
                for process in processes.iter_mut() {
                    if process.id == process_id {
                        found = true;
                        let mut table = process.data.fd_table.lock().unwrap();
                        if let Some(Some(FDEntry::File { buffer, .. })) = table.entries.get_mut(fd as usize) {
                            buffer.extend_from_slice(body.as_bytes());
                            buffer.push(b'\n');
                            info!("Added FD update to process {}'s FD {} ({} bytes)", process_id, fd, body.len());
                        } else {
                            error!("Process {} does not have FD {} open for FD update", process_id, fd);
                        }
                        process.data.cond.notify_all();
                        break;
                    }
                }
                if !found {
                    error!("No process found with ID {} for FD update", process_id);
                }
            },
            2 => { // Init command.
                debug!("Processing init command for new process");
                let new_pid = get_next_pid();
                match process::start_process_from_bytes(payload, new_pid) {
                    Ok(proc) => {
                        processes.push(proc);
                        info!("Added new process {} to scheduler", new_pid);
                    }
                    Err(e) => {
                        error!("Failed to create new process {}: {}", new_pid, e);
                    }
                }
            },
            3 => { // NetworkIn
                debug!("Processing NetworkIn for process {}", process_id);
                // The payload already contains the port + data
                // First 2 bytes are the destination port
                if payload.len() < 2 {
                    error!("NetworkIn payload too short for process {}", process_id);
                    continue;
                }
                
                let dest_port = (payload[0] as u16) | ((payload[1] as u16) << 8);
                let data = &payload[2..];
                
                debug!("Received {} bytes from network for process {} port {}", data.len(), process_id, dest_port);
                
                let mut found = false;
                for process in processes.iter_mut() {
                    if process.id == process_id {
                        found = true;
                        // Find socket with matching port
                        let mut matching_fd = None;
                        {
                            let table = process.data.fd_table.lock().unwrap();
                            for (fd, entry) in table.entries.iter().enumerate() {
                                if let Some(FDEntry::Socket { local_port, .. }) = entry {
                                    if *local_port == dest_port {
                                        matching_fd = Some(fd);
                                        break;
                                    }
                                }
                            }
                        }
                        
                        // If we found a matching socket, update it with the data
                        if let Some(fd) = matching_fd {
                            let mut table = process.data.fd_table.lock().unwrap();
                            if fd < table.entries.len() {
                                let buffer_entry = table.entries.get_mut(fd).unwrap();
                                if let Some(FDEntry::Socket { .. }) = buffer_entry {
                                    // Create a temporary file buffer for this fd to store the data
                                    *buffer_entry = Some(FDEntry::File {
                                        host_path: None,
                                        buffer: data.to_vec(),
                                        read_ptr: 0,
                                        is_directory: false,
                                        is_preopen: false,
                                    });
                                    info!("Added NetworkIn data to process {}'s FD {} ({} bytes)", 
                                         process_id, fd, data.len());
                                }
                            }
                        }
                        
                        // Notify waiting process
                        process.data.cond.notify_all();
                        break;
                    }
                }
                
                if !found {
                    error!("No process found with ID {} for NetworkIn", process_id);
                }
            },
            _ => {
                error!("Unknown message type: {} in message", msg_type);
            }
        }
    }
    Ok(true) // For pipe mode, we always return true to keep scheduler running
}

pub fn process_consensus_file(file_path: &str, processes: &mut Vec<process::Process>) -> Result<bool> {
    debug!("Processing consensus file: {}", file_path);
    let file = File::open(file_path)?;
    let mut reader = BufReader::new(file);
    
    // Seek to the current position
    let current_pos = FILE_POSITION.load(Ordering::SeqCst);
    debug!("Seeking to position {} in consensus file", current_pos);
    reader.seek(SeekFrom::Start(current_pos))?;
    
    let mut processed_something = false;

    loop {
        // Read the message type (1 byte)
        let mut msg_type_buf = [0u8; 1];
        if reader.read_exact(&mut msg_type_buf).is_err() {
            // End of file reached
            // Return true if we processed at least one command in this batch
            // Return false if we reached EOF without processing anything
            return Ok(processed_something);
        }
        let msg_type = msg_type_buf[0];

        // Read process_id (8 bytes)
        let process_id = match reader.read_u64::<LittleEndian>() {
            Ok(pid) => pid,
            Err(_) => return Ok(processed_something), // End of file
        };

        // Read payload length (4 bytes)
        let payload_len = match reader.read_u32::<LittleEndian>() {
            Ok(sz) => sz as usize,
            Err(_) => return Ok(processed_something), // End of file
        };

        // Read the payload.
        let mut payload = vec![0u8; payload_len];
        if let Err(e) = reader.read_exact(&mut payload) {
            error!("Failed to read message from file: {}", e);
            return Ok(processed_something);
        }

        // Save the current position after reading this record
        let current_pos = reader.stream_position()?;
        FILE_POSITION.store(current_pos, Ordering::SeqCst);

        processed_something = true;

        // Convert payload to a string for text-based messages.
        let msg_str = match msg_type {
            0 | 1 | 4 => {
                match String::from_utf8(payload.clone()) {
                    Ok(s) => s,
                    Err(e) => {
                        error!("Failed to decode file message as UTF-8: {}", e);
                        continue; // Try to process next command in batch
                    }
                }
            },
            2 => String::new(), // For Init command, the payload is binary.
            _ => {
                error!("Unknown message type: {} in file", msg_type);
                continue; // Try to process next command in batch
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
                // Clock command marks the end of a batch, so return
                return Ok(true);
            },
            1 => { // FD update.
                debug!("Processing FD update for process {}: {}", process_id, msg_str);
                let parts: Vec<&str> = msg_str.split(",body:").collect();
                if parts.len() != 2 {
                    error!("Invalid file message format for FD update: {}", msg_str);
                    continue; // Try to process next command in batch
                }
                let fd: i32 = if let Some(fd_part) = parts[0].strip_prefix("fd:") {
                    match fd_part.trim().parse() {
                        Ok(num) => num,
                        Err(_) => {
                            error!("Invalid FD in file message: {}", msg_str);
                            continue; // Try to process next command in batch
                        }
                    }
                } else {
                    error!("Missing FD prefix in file message: {}", msg_str);
                    continue; // Try to process next command in batch
                };
                let body = parts[1].trim();
                let mut found = false;
                for process in processes.iter_mut() {
                    if process.id == process_id {
                        found = true;
                        let mut table = process.data.fd_table.lock().unwrap();
                        if let Some(Some(FDEntry::File { buffer, .. })) = table.entries.get_mut(fd as usize) {
                            buffer.extend_from_slice(body.as_bytes());
                            buffer.push(b'\n');
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
                info!("Received init command from consensus file");
                let new_pid = get_next_pid();
                match process::start_process_from_bytes(payload, new_pid) {
                    Ok(proc) => {
                        processes.push(proc);
                        info!("Added new process {} to scheduler (via file)", new_pid);
                    }
                    Err(e) => {
                        error!("Failed to create new process {}: {}", new_pid, e);
                    }
                }
            },
            3 => { // Msg command.
                debug!("Processing message command for process {}: {}", process_id, msg_str);
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
                        if let Some(Some(FDEntry::File { buffer, .. })) = table.entries.get_mut(0) {
                            buffer.extend_from_slice(message.as_bytes());
                            buffer.push(b'\n');
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
                info!("Received FTP command for process {}: {} (via file)", process_id, msg_str);
                // Add FTP command dispatch logic here if needed.
            },
            _ => {
                error!("Unknown message type: {} in file message: {}", msg_type, msg_str);
            }
        }
    }
}
