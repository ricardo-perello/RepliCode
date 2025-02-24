use std::fs::File;
use std::io::{BufReader, Read};
use byteorder::{LittleEndian, ReadBytesExt};
use anyhow::Result;

use crate::runtime::process::Process;

/// Reads a consensus input file and updates the FD buffers for the appropriate processes.
///
/// The file is expected to contain a series of records in binary format:
/// [ process_id: u64 ][ msg_size: u16 ][ msg: [u8; msg_size] ]
///
/// The message is assumed to be a UTF-8 encoded string with the following format:
/// "fd:<number>,body:<data>"
/// For example: "fd:0,body:Hello World"
pub fn process_consensus_file(file_path: &str, processes: &mut Vec<Process>) -> Result<()> {
    let file = File::open(file_path)?;
    let mut reader = BufReader::new(file);

    loop {
        // Read process_id (u64)
        let process_id = match reader.read_u64::<LittleEndian>() {
            Ok(pid) => pid,
            Err(_) => break, // EOF reached or error encountered.
        };

        // Read message size (u16)
        let msg_size = match reader.read_u16::<LittleEndian>() {
            Ok(sz) => sz,
            Err(_) => break,
        };

        // Read message bytes
        let mut msg_buf = vec![0u8; msg_size as usize];
        if let Err(e) = reader.read_exact(&mut msg_buf) {
            eprintln!("Failed to read message: {}", e);
            break;
        }

        // Interpret the message as a UTF-8 string.
        let msg_str = match String::from_utf8(msg_buf) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to decode message as UTF-8: {}", e);
                continue;
            }
        };

        // Parse the message.
        // Expected format: "fd:<number>,body:<data>"
        let parts: Vec<&str> = msg_str.split(",body:").collect();
        if parts.len() != 2 {
            eprintln!("Invalid message format: {}", msg_str);
            continue;
        }
        let fd: i32 = if let Some(fd_part) = parts[0].strip_prefix("fd:") {
            match fd_part.trim().parse() {
                Ok(num) => num,
                Err(_) => {
                    eprintln!("Invalid FD in message: {}", msg_str);
                    continue;
                }
            }
        } else {
            eprintln!("Missing FD prefix in message: {}", msg_str);
            continue;
        };
        let body = parts[1].trim();

        // Update the corresponding process's FD table.
        let mut found = false;
        for process in processes.iter_mut() {
            if process.id == process_id {
                found = true;
                let mut table = process.data.fd_table.lock().unwrap();
                if let Some(Some(fd_entry)) = table.entries.get_mut(fd as usize) {
                    fd_entry.buffer.extend_from_slice(body.as_bytes());
                    // Optionally add a newline as a delimiter.
                    fd_entry.buffer.push(b'\n');
                } else {
                    eprintln!("Process {} does not have FD {} open", process_id, fd);
                }
                // Notify the process so that it wakes up if waiting on input.
                process.data.cond.notify_all();
                break;
            }
        }
        if !found {
            eprintln!("No process found with ID {}", process_id);
        }
    }
    Ok(())
}
