use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use byteorder::{LittleEndian, ReadBytesExt};
use anyhow::Result;
use crate::runtime::process::Process;
use crate::runtime::clock::GlobalClock;
use std::sync::atomic::{AtomicU64, Ordering};
use log::{info, error};

// Global read offset used by the file-based consensus function.
static READ_OFFSET: AtomicU64 = AtomicU64::new(0);

/// Reads new records from the consensus input file for one batch only.
///
/// Record format:
///   [ process_id: u64 ][ msg_size: u16 ][ msg: [u8; msg_size] ]
///
/// Special cases:
/// - For FD updates, use a message like "fd:<number>,body:<data>" with process_id != 0.
/// - For clock updates, use a record with process_id == 0 and a message starting with "clock:".
///   This clock record is treated as the batch end.
pub fn process_consensus_file(file_path: &str, processes: &mut Vec<Process>) -> Result<()> {
    let mut file = File::open(file_path)?;
    let offset = READ_OFFSET.load(Ordering::SeqCst);
    file.seek(SeekFrom::Start(offset))?;
    let mut reader = BufReader::new(file);

    loop {
        // Try to read process_id.
        let process_id = match reader.read_u64::<LittleEndian>() {
            Ok(pid) => pid,
            Err(_) => break, // EOF reached.
        };

        // Read message size.
        let msg_size = match reader.read_u16::<LittleEndian>() {
            Ok(sz) => sz,
            Err(_) => break,
        };

        // Read message bytes.
        let mut msg_buf = vec![0u8; msg_size as usize];
        if let Err(e) = reader.read_exact(&mut msg_buf) {
            error!("Failed to read message: {}", e);
            break;
        }

        // Update our global offset after reading this record.
        let new_offset = reader.stream_position()?;
        READ_OFFSET.store(new_offset, Ordering::SeqCst);

        // Interpret the message.
        let msg_str = match String::from_utf8(msg_buf) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to decode message as UTF-8: {}", e);
                continue;
            }
        };

        // If the record is a clock update, process it and then end this batch.
        if process_id == 0 {
            if let Some(delta_str) = msg_str.strip_prefix("clock:") {
                match delta_str.trim().parse::<u64>() {
                    Ok(delta) => {
                        GlobalClock::increment(delta);
                        info!("Global clock incremented by {}", delta);
                    }
                    Err(e) => error!("Invalid clock increment: {}", e),
                }
            } else {
                error!("Invalid clock message format: {}", msg_str);
            }
            // End of the batch.
            break;
        }

        // Otherwise, process as an FD update.
        // Expected format: "fd:<number>,body:<data>"
        let parts: Vec<&str> = msg_str.split(",body:").collect();
        if parts.len() != 2 {
            error!("Invalid message format: {}", msg_str);
            continue;
        }
        let fd: i32 = if let Some(fd_part) = parts[0].strip_prefix("fd:") {
            match fd_part.trim().parse() {
                Ok(num) => num,
                Err(_) => {
                    error!("Invalid FD in message: {}", msg_str);
                    continue;
                }
            }
        } else {
            error!("Missing FD prefix in message: {}", msg_str);
            continue;
        };
        let body = parts[1].trim();

        // Update the corresponding process's FD.
        let mut found = false;
        for process in processes.iter_mut() {
            if process.id == process_id {
                found = true;
                let mut table = process.data.fd_table.lock().unwrap();
                if let Some(Some(fd_entry)) = table.entries.get_mut(fd as usize) {
                    fd_entry.buffer.extend_from_slice(body.as_bytes());
                    // Optionally add a newline.
                    fd_entry.buffer.push(b'\n');
                    info!("Added input to process {}'s FD {}", process_id, fd);
                } else {
                    error!("Process {} does not have FD {} open", process_id, fd);
                }
                process.data.cond.notify_all();
                break;
            }
        }
        if !found {
            error!("No process found with ID {}", process_id);
        }
    }
    Ok(())
}

/// Reads new records from a live consensus pipe/socket for one batch only.
///
/// Record format:
///   [ process_id: u64 ][ msg_size: u16 ][ msg: [u8; msg_size] ]
///
/// Special cases:
/// - For FD updates, use a message like "fd:<number>,body:<data>" with process_id != 0.
/// - For clock updates, use a record with process_id == 0 and a message starting with "clock:".
///   This clock record is treated as the batch end.
pub fn process_consensus_pipe<R: Read>(consensus_pipe: &mut R, processes: &mut Vec<Process>) -> Result<()> {
    let mut reader = BufReader::new(consensus_pipe);

    loop {
        // Try to read process_id.
        let process_id = match reader.read_u64::<LittleEndian>() {
            Ok(pid) => pid,
            Err(_) => break, // No more complete data available.
        };

        // Read message size.
        let msg_size = match reader.read_u16::<LittleEndian>() {
            Ok(sz) => sz,
            Err(_) => break,
        };

        // Read message bytes.
        let mut msg_buf = vec![0u8; msg_size as usize];
        if let Err(e) = reader.read_exact(&mut msg_buf) {
            error!("Failed to read message from pipe: {}", e);
            break;
        }

        // Interpret the message.
        let msg_str = match String::from_utf8(msg_buf) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to decode pipe message as UTF-8: {}", e);
                continue;
            }
        };

        // If the record is a clock update, process it and then end this batch.
        if process_id == 0 {
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
            // End of the batch.
            break;
        }

        // Otherwise, process as an FD update.
        // Expected format: "fd:<number>,body:<data>"
        let parts: Vec<&str> = msg_str.split(",body:").collect();
        if parts.len() != 2 {
            error!("Invalid pipe message format: {}", msg_str);
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

        // Update the corresponding process's FD.
        let mut found = false;
        for process in processes.iter_mut() {
            if process.id == process_id {
                found = true;
                let mut table = process.data.fd_table.lock().unwrap();
                if let Some(Some(fd_entry)) = table.entries.get_mut(fd as usize) {
                    fd_entry.buffer.extend_from_slice(body.as_bytes());
                    // Optionally add a newline.
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
    }
    Ok(())
}
