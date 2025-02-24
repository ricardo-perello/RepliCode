use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use byteorder::{LittleEndian, ReadBytesExt};
use anyhow::Result;
use crate::runtime::process::Process;
use crate::runtime::clock::GlobalClock;

// Global read offset.
use std::sync::atomic::{AtomicU64, Ordering};
static READ_OFFSET: AtomicU64 = AtomicU64::new(0);

/// Reads a consensus input file from the last read offset and updates the FD buffers
/// or global clock based on the records found.
/// 
/// Record format:
///   [ process_id: u64 ][ msg_size: u16 ][ msg: [u8; msg_size] ]
/// 
/// For FD updates, the message is: "fd:<number>,body:<data>"
/// For clock updates, the record must have process_id==0 and the message is: "clock:<delta>"
pub fn process_consensus_file(file_path: &str, processes: &mut Vec<Process>) -> Result<()> {
    // Open the file and seek to the last read offset.
    let mut file = File::open(file_path)?;
    let offset = READ_OFFSET.load(Ordering::SeqCst);
    file.seek(SeekFrom::Start(offset))?;
    let mut reader = BufReader::new(file);

    loop {
        // Attempt to read the process_id.
        let process_id = match reader.read_u64::<LittleEndian>() {
            Ok(pid) => pid,
            Err(_) => break, // EOF or error encountered.
        };

        // Read the message size.
        let msg_size = match reader.read_u16::<LittleEndian>() {
            Ok(sz) => sz,
            Err(_) => break,
        };

        // Read the message bytes.
        let mut msg_buf = vec![0u8; msg_size as usize];
        if let Err(e) = reader.read_exact(&mut msg_buf) {
            eprintln!("Failed to read message: {}", e);
            break;
        }

        // Update our global offset after successfully reading a record.
        let new_offset = reader.stream_position()?;
        READ_OFFSET.store(new_offset, Ordering::SeqCst);

        // Interpret the message as UTF-8.
        let msg_str = match String::from_utf8(msg_buf) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to decode message as UTF-8: {}", e);
                continue;
            }
        };

        // Process a clock update if process_id is 0.
        if process_id == 0 {
            // Expected format: "clock:<delta>"
            if let Some(delta_str) = msg_str.strip_prefix("clock:") {
                match delta_str.trim().parse::<u64>() {
                    Ok(delta) => {
                        GlobalClock::increment(delta);
                        println!("Global clock incremented by {}", delta);
                    }
                    Err(e) => eprintln!("Invalid clock increment: {}", e),
                }
            } else {
                eprintln!("Invalid clock message format: {}", msg_str);
            }
            continue; // Move on to the next record.
        }

        // Otherwise, process it as an FD message.
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

        // Find the target process and update its FD.
        let mut found = false;
        for process in processes.iter_mut() {
            if process.id == process_id {
                found = true;
                let mut table = process.data.fd_table.lock().unwrap();
                if let Some(Some(fd_entry)) = table.entries.get_mut(fd as usize) {
                    fd_entry.buffer.extend_from_slice(body.as_bytes());
                    // Optionally add a newline as a delimiter.
                    fd_entry.buffer.push(b'\n');
                    println!("Added input to process {}'s FD {}", process_id, fd);
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
    //println!("Finished processing consensus file");
    Ok(())
}
