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
use consensus::commands::NetworkOperation;

// Use an AtomicU64 for generating unique process IDs.
static NEXT_PID: AtomicU64 = AtomicU64::new(1);
// Track file position for consensus file
static FILE_POSITION: AtomicU64 = AtomicU64::new(0);
static OUTGOING_BATCH_NUMBER: AtomicU64 = AtomicU64::new(1);

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

    // First, send any outgoing network messages as a batch
    if !outgoing_messages.is_empty() {
        let batch_number = OUTGOING_BATCH_NUMBER.fetch_add(1, Ordering::SeqCst);
        let direction = 1u8; // Outgoing
        let mut batch_data = Vec::new();
        for msg in outgoing_messages {
            debug!("Sending outgoing network message for process {}: {:?}", msg.pid, msg.operation);
            // Write message type (NetworkOut = 5)
            batch_data.push(5);
            // Write process ID
            batch_data.extend_from_slice(&msg.pid.to_le_bytes());
            // Serialize and write the network operation
            let op_bytes = bincode::serialize(&msg.operation)?;
            batch_data.extend_from_slice(&(op_bytes.len() as u32).to_le_bytes());
            batch_data.extend_from_slice(&op_bytes);
        }
        // Write batch header
        reader.get_mut().write_all(&batch_number.to_le_bytes())?;
        reader.get_mut().write_all(&[direction])?;
        reader.get_mut().write_all(&(batch_data.len() as u64).to_le_bytes())?;
        // Write batch data
        reader.get_mut().write_all(&batch_data)?;
        debug!("Sent outgoing batch {} ({} bytes)", batch_number, batch_data.len());
    }

    // Read batch header (8 bytes for batch number, 1 byte for direction)
    let mut batch_header = [0u8; 9];
    if reader.read_exact(&mut batch_header).is_err() {
        debug!("No batch header in consensus pipe");
        return Ok(false);
    }

    let batch_number = u64::from_le_bytes(batch_header[0..8].try_into().unwrap());
    let direction = batch_header[8];
    debug!("Received batch {} with direction {}", batch_number, direction);

    // Read batch data length (8 bytes)
    let mut data_len_buf = [0u8; 8];
    if reader.read_exact(&mut data_len_buf).is_err() {
        error!("Failed to read batch data length");
        return Ok(false);
    }
    let data_len = u64::from_le_bytes(data_len_buf) as usize;
    debug!("Batch {} data length: {} bytes", batch_number, data_len);

    // Read the batch data
    let mut batch_data = vec![0u8; data_len];
    if reader.read_exact(&mut batch_data).is_err() {
        error!("Failed to read batch data");
        return Ok(false);
    }

    // Process the batch data as a series of records
    let mut data_reader = std::io::Cursor::new(batch_data);
    loop {
        // Read the message type (1 byte)
        let mut msg_type_buf = [0u8; 1];
        if data_reader.read_exact(&mut msg_type_buf).is_err() {
            debug!("No more records in batch {}", batch_number);
            break; // No more data.
        }
        let msg_type = msg_type_buf[0];
        debug!("Processing record type {} in batch {}", msg_type, batch_number);

        // Read process_id (8 bytes)
        let process_id = match data_reader.read_u64::<LittleEndian>() {
            Ok(pid) => pid,
            Err(_) => break,
        };

        // Read payload length (4 bytes)
        let payload_len = match data_reader.read_u32::<LittleEndian>() {
            Ok(sz) => sz as usize,
            Err(_) => break,
        };

        debug!("Reading payload of {} bytes for process {} in batch {}", payload_len, process_id, batch_number);

        // Read the payload.
        let mut payload = vec![0u8; payload_len];
        if let Err(e) = data_reader.read_exact(&mut payload) {
            error!("Failed to read message from batch {}: {}", batch_number, e);
            break;
        }

        match msg_type {
            0 => { // Clock update.
                let msg_str = String::from_utf8_lossy(&payload);
                debug!("Processing clock update in batch {}: {}", batch_number, msg_str);
                if let Some(delta_str) = msg_str.strip_prefix("clock:") {
                    match delta_str.trim().parse::<u64>() {
                        Ok(delta) => {
                            GlobalClock::increment(delta);
                            info!("Global clock incremented by {} in batch {}", delta, batch_number);
                        }
                        Err(e) => error!("Invalid clock increment in batch {}: {}", batch_number, e),
                    }
                } else {
                    error!("Invalid clock message format in batch {}: {}", batch_number, msg_str);
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
                
                debug!("Received {} bytes from network for process {} port {}: {}", data.len(), process_id, dest_port, String::from_utf8_lossy(data));
                
                let mut found = false;
                for process in processes.iter_mut() {
                    if process.id == process_id {
                        found = true;
                        // If this is a success status message (port 0)
                        if dest_port == 0 && data.len() >= 5 {  // Now we expect at least 5 bytes
                            let status = data[0];
                            let src_port = (data[1] as u16) | ((data[2] as u16) << 8);
                            let new_port = (data[3] as u16) | ((data[4] as u16) << 8);
                            match status {
                                1 => { // Success
                                    info!("Network operation succeeded for process {}:{}", process_id, src_port);
                                    // Update the runtime's NAT table to match consensus
                                    let mut nat_table = process.data.nat_table.lock().unwrap();
                                    if new_port != 0 {  // This is an accept operation
                                        debug!("Processing accept success for process {}:{} -> {}", process_id, src_port, new_port);
                                        // Add mapping for the new port
                                        nat_table.add_port_mapping(process_id, new_port);
                                        // Mark the socket as connected
                                        let mut table = process.data.fd_table.lock().unwrap();
                                        debug!("Looking for socket with port {} in FD table (size: {})", new_port, table.entries.len());
                                        // Find the socket with matching port
                                        let mut found = false;
                                        for (fd, entry) in table.entries.iter_mut().enumerate() {
                                            if let Some(FDEntry::Socket { local_port, connected, .. }) = entry {
                                                if *local_port == new_port {
                                                    *connected = true;
                                                    debug!("Marked socket FD {} as connected for process {}:{}", fd, process_id, new_port);
                                                    found = true;
                                                    break;
                                                }
                                            }
                                        }
                                        if !found {
                                            error!("Could not find socket with port {} in FD table for process {}", new_port, process_id);
                                            // Debug: Print all socket entries
                                            for (fd, entry) in table.entries.iter().enumerate() {
                                                if let Some(FDEntry::Socket { local_port, is_listener, connected, .. }) = entry {
                                                    debug!("FD {}: port={}, is_listener={}, connected={}", fd, local_port, is_listener, connected);
                                                }
                                            }
                                        }
                                    } else {
                                        // Regular operation, just add mapping for src_port
                                        nat_table.add_port_mapping(process_id, src_port);
                                    }
                                    // Clear the waiting state
                                    nat_table.clear_waiting_accept(process_id, src_port);
                                }
                                2 => { // Still waiting
                                    debug!("Network operation still waiting for process {}:{}", process_id, src_port);
                                    // Keep the process blocked
                                    let mut nat_table = process.data.nat_table.lock().unwrap();
                                    nat_table.set_waiting_accept(process_id, src_port);
                                }
                                _ => { // Failure
                                    error!("Network operation failed for process {}:{}", process_id, src_port);
                                    // Clear both waiting states to ensure process unblocks
                                    let mut nat_table = process.data.nat_table.lock().unwrap();
                                    nat_table.clear_waiting_accept(process_id, src_port);
                                    nat_table.clear_waiting_recv(process_id, src_port);
                                    debug!("Cleared waiting states for process {}:{} due to failure", process_id, src_port);
                                    
                                    // Also mark any connected sockets as disconnected
                                    let mut table = process.data.fd_table.lock().unwrap();
                                    for (fd, entry) in table.entries.iter_mut().enumerate() {
                                        if let Some(FDEntry::Socket { local_port, connected, .. }) = entry {
                                            if *local_port == src_port && *connected {
                                                *connected = false;
                                                debug!("Marked socket FD {} as disconnected for process {}:{}", 
                                                      fd, process_id, src_port);
                                            }
                                        }
                                    }
                                }
                            }
                            // Notify the process that the operation completed (or is still waiting)
                            process.data.cond.notify_all();
                            break;
                        }
                        
                        // Find socket with matching port
                        let mut matching_fd = None;
                        {
                            let table = process.data.fd_table.lock().unwrap();
                            // First look for an accepted connection socket
                            for (fd, entry) in table.entries.iter().enumerate() {
                                if let Some(FDEntry::Socket { local_port, is_listener, .. }) = entry {
                                    if *local_port == dest_port && !*is_listener {
                                        matching_fd = Some(fd);
                                        break;
                                    }
                                }
                            }
                            // If no accepted connection found, look for a listening socket
                            if matching_fd.is_none() {
                                for (fd, entry) in table.entries.iter().enumerate() {
                                    if let Some(FDEntry::Socket { local_port, is_listener, .. }) = entry {
                                        if *local_port == dest_port && *is_listener {
                                            matching_fd = Some(fd);
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        
                        // If we found a matching socket, update it with the data
                        if let Some(fd) = matching_fd {
                            let mut table = process.data.fd_table.lock().unwrap();
                            if let Some(Some(FDEntry::Socket { buffer, .. })) = table.entries.get_mut(fd) {
                                buffer.extend_from_slice(data);
                                // Clear waiting state since we have data
                                let mut nat_table = process.data.nat_table.lock().unwrap();
                                nat_table.clear_waiting_recv(process_id, dest_port);
                                info!("Added NetworkIn data to process {}'s socket FD {} ({} bytes)", 
                                     process_id, fd, data.len());
                            }
                        } else {
                            error!("No matching socket found for process {} port {}", process_id, dest_port);
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
