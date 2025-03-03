use std::io;
use byteorder::{LittleEndian, WriteBytesExt};
use std::io::Write;

/// Encodes a record using the standard batch format:
/// [ process_id: u64 ][ msg_size: u16 ][ msg: [u8; msg_size] ]
/// This version accepts a raw byte slice (which can be either UTF-8 text or binary data).
pub fn write_record_bytes(pid: u64, payload: &[u8]) -> io::Result<Vec<u8>> {
    if payload.len() > (u16::MAX as usize) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Payload too long"));
    }
    let mut record = Vec::with_capacity(8 + 2 + payload.len());
    record.write_u64::<LittleEndian>(pid)?;
    record.write_u16::<LittleEndian>(payload.len() as u16)?;
    record.write_all(payload)?;
    Ok(record)
}

/// Convenience function for text messages.
pub fn write_record(pid: u64, message: &str) -> io::Result<Vec<u8>> {
    write_record_bytes(pid, message.as_bytes())
}
