use std::io;
use byteorder::{LittleEndian, WriteBytesExt};
use std::io::Write;
use crate::commands::Command;

/// Write a binary record for a given command.
///
/// Record layout:
/// [ 1 byte: msg_type ][ 8 bytes: process_id ][ 2 bytes: payload_length ][ payload ]
pub fn write_record(cmd: &Command) -> io::Result<Vec<u8>> {
    let (msg_type, pid, payload) = match cmd {
        Command::Clock(delta) => {
            // Type 0; payload is the 8-byte little-endian representation.
            (0u8, 0u64, delta.to_le_bytes().to_vec())
        },
        Command::Init(wasm_bytes) => (2u8, u64::MAX, wasm_bytes.clone()),
        Command::FDMsg(pid, data) => (1u8, *pid, data.clone()),
        Command::NetMsg(net_msg) => {
            // Type 3; payload packs destination (8 bytes) then payload.
            let mut payload = Vec::with_capacity(8 + net_msg.payload.len());
            payload.write_u64::<LittleEndian>(net_msg.dst)?;
            payload.extend(&net_msg.payload);
            (3u8, net_msg.src, payload)
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
