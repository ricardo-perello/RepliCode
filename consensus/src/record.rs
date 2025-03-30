use std::io;
use byteorder::{LittleEndian, WriteBytesExt};
use std::io::Write;
use crate::commands::Command;

/// Write a binary record for a given command.
/// New record layout:
/// [ 1 byte msg_type ][ 8 bytes process_id ][ 2 bytes payload_length ][ payload ]
pub fn write_record(cmd: &Command) -> io::Result<Vec<u8>> {
    let (msg_type, pid, payload) = match cmd {
        Command::Clock(delta) => {
            // Type 0; payload is "clock:<delta>"
            (0u8, 0u64, format!("clock:{}", delta).as_bytes().to_vec())
        },
        Command::Init(wasm_bytes, dir_path) => {
            // For Init, we'll prepend the directory path if present
            let mut payload = Vec::new();
            if let Some(dir) = dir_path {
                payload.extend(format!("dir:{}", dir).as_bytes());
                payload.push(0); // Null terminator between dir and wasm //TODO: Make sure this wont cause issues with the wasm file data
            }
            payload.extend(wasm_bytes);
            (2u8, u64::MAX, payload)
        },
        Command::FDMsg(pid, data) => (1u8, *pid, data.clone()),
        Command::Ftp(pid, ftp_cmd) => {
            // Type 4 for FTP operations; include the provided pid.
            (4u8, *pid, ftp_cmd.as_bytes().to_vec())
        },
    };

    if payload.len() > (u16::MAX as usize) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Payload too long"));
    }
    let mut record = Vec::with_capacity(1 + 8 + 2 + payload.len());
    record.push(msg_type);
    record.write_u64::<LittleEndian>(pid)?;
    record.write_u16::<LittleEndian>(payload.len() as u16)?;
    record.write_all(&payload)?;
    Ok(record)
}