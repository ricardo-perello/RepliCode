use anyhow::Result;
use wasmtime::Caller;
use crate::runtime::process::ProcessData;
use crate::runtime::fd_table::FDEntry;
use log::{info, debug};
use std::fs;
use std::os::unix::fs::MetadataExt;

pub fn wasi_fd_advise(
    _caller: Caller<ProcessData>,
    fd: u32,
    offset: u64,
    len: u64,
    advice: u32,
) -> Result<u32> {
    info!("wasi_fd_advise: fd={}, offset={}, len={}, advice={}", fd, offset, len, advice);
    Ok(0)
}

pub fn wasi_fd_allocate(
    _caller: Caller<ProcessData>,
    fd: u32,
    offset: u64,
    len: u64,
) -> Result<u32> {
    info!("wasi_fd_allocate: fd={}, offset={}, len={}", fd, offset, len);
    Ok(0)
}

pub fn wasi_fd_datasync(
    _caller: Caller<ProcessData>,
    fd: u32,
) -> Result<u32> {
    info!("wasi_fd_datasync: fd={}", fd);
    
    // Check if fd is valid
    let process_data = _caller.data();
    let table = process_data.fd_table.lock().unwrap();
    if fd as usize >= table.entries.len() {
        return Ok(8); // WASI_EBADF
    }
    match &table.entries[fd as usize] {
        Some(_) => Ok(0), // Success - no-op since we're working with in-memory files
        None => Ok(8), // WASI_EBADF
    }
}

pub fn wasi_fd_fdstat_set_flags(
    _caller: Caller<ProcessData>,
    fd: u32,
    flags: u32,
) -> Result<u32> {
    info!("wasi_fd_fdstat_set_flags: fd={}, flags={}", fd, flags);
    Ok(0)
}

pub fn wasi_fd_fdstat_set_rights(
    _caller: Caller<ProcessData>,
    fd: u32,
    fs_rights_base: u64,
    fs_rights_inheriting: u64,
) -> Result<u32> {
    info!("wasi_fd_fdstat_set_rights: fd={}, fs_rights_base={}, fs_rights_inheriting={}", 
        fd, fs_rights_base, fs_rights_inheriting);
    Ok(0)
}

// pub fn wasi_fd_fdstat_get(
//     mut caller: Caller<ProcessData>,
//     fd: u32,
//     stat_ptr: u32,
// ) -> Result<u32> {
//     info!("wasi_fd_fdstat_get: fd={}, stat_ptr={}", fd, stat_ptr);
    
//     // Get file descriptor info
//     let (fs_filetype, fs_flags, fs_rights_base, fs_rights_inheriting) = {
//         let process_data = caller.data();
//         let table = process_data.fd_table.lock().unwrap();
//         if fd as usize >= table.entries.len() {
//             return Ok(8); // WASI_EBADF
//         }
//         match &table.entries[fd as usize] {
//             Some(FDEntry::File { .. }) => (
//                 4u8,  // WASI_FILETYPE_REGULAR_FILE
//                 0u16,  // No flags
//                 0x1u64 | 0x2 | 0x4 | 0x8 | 0x10 | 0x20 | 0x40 | 0x80 | 0x100 | 0x200 | 0x400 | 0x800 | 0x1000 | 0x2000 | 0x4000 | 0x8000 | 0x10000 | 0x20000 | 0x40000 | 0x80000 | 0x100000 | 0x200000 | 0x400000 | 0x800000 | 0x1000000 | 0x2000000 | 0x4000000 | 0x8000000 | 0x10000000 | 0x20000000 | 0x40000000 | 0x80000000,  // All rights
//                 0u64,  // No inheriting rights
//             ),
//             Some(FDEntry::Socket { .. }) => (
//                 7u8,  // WASI_FILETYPE_SOCKET_STREAM
//                 0u16,  // No flags
//                 0x1u64 | 0x2 | 0x4 | 0x8 | 0x10 | 0x20 | 0x40 | 0x80 | 0x100 | 0x200 | 0x400 | 0x800 | 0x1000 | 0x2000 | 0x4000 | 0x8000 | 0x10000 | 0x20000 | 0x40000 | 0x80000 | 0x100000 | 0x200000 | 0x400000 | 0x800000 | 0x1000000 | 0x2000000 | 0x4000000 | 0x8000000 | 0x10000000 | 0x20000000 | 0x40000000 | 0x80000000,  // All rights
//                 0u64,  // No inheriting rights
//             ),
//             None => return Ok(8), // WASI_EBADF
//         }
//     };

//     // Get memory and write fdstat
//     let memory = caller.get_export("memory").unwrap().into_memory().unwrap();
//     let mem = memory.data_mut(&mut caller);
//     let ptr = stat_ptr as usize;
//     if ptr + 24 > mem.len() {
//         return Ok(21); // WASI_EFAULT
//     }

//     // Write fdstat to memory
//     mem[ptr..ptr+4].copy_from_slice(&fs_filetype.to_le_bytes());
//     mem[ptr+4..ptr+8].copy_from_slice(&fs_flags.to_le_bytes());
//     mem[ptr+8..ptr+16].copy_from_slice(&fs_rights_base.to_le_bytes());
//     mem[ptr+16..ptr+24].copy_from_slice(&fs_rights_inheriting.to_le_bytes());
    
//     Ok(0)
// }

// pub fn wasi_fd_filestat_get(
//     mut caller: Caller<ProcessData>,
//     fd: u32,
//     buf_ptr: u32,
// ) -> anyhow::Result<u32> {
//     info!("wasi_fd_filestat_get: fd={}, buf_ptr={}", fd, buf_ptr);
    
//     // Get FD entry
//     let (size, filetype) = {
//         let process_data = caller.data();
//         let table = process_data.fd_table.lock().unwrap();
//         if fd as usize >= table.entries.len() {
//             return Ok(8); // WASI_EBADF
//         }
//         match &table.entries[fd as usize] {
//             Some(FDEntry::File { buffer, is_directory, host_path, .. }) => {
//                 debug!("DEBUG: entry buffer.len() = {}  host_path = {:?}", buffer.len(), host_path);
//                 let size = if !buffer.is_empty() {
//                     buffer.len() as u64
//                 } else {
//                     match host_path {
//                         Some(path) => match std::fs::metadata(path) {
//                             Ok(metadata) => metadata.len(),
//                             Err(_) => return Ok(8), // WASI_EBADF
//                         },
//                         None => 0,
//                     }
//                 };
//                 (size, if *is_directory { 3u8 } else { 4u8 })
//             }
//             Some(FDEntry::Socket { .. }) => {
//                 (0, 5u8) // Socket type
//             }
//             None => return Ok(8), // WASI_EBADF
//         }
//     };

//     // Create filestat buffer (64 bytes)
//     let mut buf = [0u8; 64];
    
//     // device (8 bytes) - set to 0
//     buf[0..8].copy_from_slice(&0u64.to_le_bytes());
    
//     // inode (8 bytes) - set to 0
//     buf[8..16].copy_from_slice(&0u64.to_le_bytes());
    
//     // filetype (1 byte)
//     buf[16] = filetype;
    
//     // nlink (4 bytes) - set to 1
//     buf[20..24].copy_from_slice(&1u32.to_le_bytes());
    
//     // size (8 bytes)
//     buf[24..32].copy_from_slice(&size.to_le_bytes());
    
//     // atim (8 bytes) - set to 0
//     buf[32..40].copy_from_slice(&0u64.to_le_bytes());
    
//     // mtim (8 bytes) - set to 0
//     buf[40..48].copy_from_slice(&0u64.to_le_bytes());
    
//     // ctim (8 bytes) - set to 0
//     buf[48..56].copy_from_slice(&0u64.to_le_bytes());

//     // Write to memory
//     let memory = caller.get_export("memory").unwrap().into_memory().unwrap();
//     let mem = memory.data_mut(&mut caller);
//     let ptr = buf_ptr as usize;
//     if ptr + 64 > mem.len() {
//         return Ok(21); // WASI_EFAULT
//     }
//     mem[ptr..ptr+64].copy_from_slice(&buf);
    
//     Ok(0)
// }

pub fn wasi_fd_filestat_set_size(
    _caller: Caller<ProcessData>,
    fd: u32,
    size: u64,
) -> Result<u32> {
    info!("wasi_fd_filestat_set_size: fd={}, size={}", fd, size);
    Ok(0)
}

pub fn wasi_fd_filestat_set_times(
    _caller: Caller<ProcessData>,
    fd: u32,
    atim: u64,
    mtim: u64,
    fst_flags: u32,
) -> Result<u32> {
    info!("wasi_fd_filestat_set_times: fd={}, atim={}, mtim={}, fst_flags={}", 
        fd, atim, mtim, fst_flags);
    Ok(0)
}

pub fn wasi_fd_pread(
    _caller: Caller<ProcessData>,
    fd: u32,
    iovs_ptr: u32,
    iovs_len: u32,
    offset: u64,
    nread_ptr: u32,
) -> Result<u32> {
    info!("wasi_fd_pread: fd={}, iovs_ptr={}, iovs_len={}, offset={}, nread_ptr={}", 
        fd, iovs_ptr, iovs_len, offset, nread_ptr);
    Ok(0)
}

pub fn wasi_fd_pwrite(
    _caller: Caller<ProcessData>,
    fd: u32,
    iovs_ptr: u32,
    iovs_len: u32,
    offset: u64,
    nwritten_ptr: u32,
) -> Result<u32> {
    info!("wasi_fd_pwrite: fd={}, iovs_ptr={}, iovs_len={}, offset={}, nwritten_ptr={}", 
        fd, iovs_ptr, iovs_len, offset, nwritten_ptr);
    Ok(0)
}

pub fn wasi_fd_renumber(
    _caller: Caller<ProcessData>,
    from: u32,
    to: u32,
) -> Result<u32> {
    info!("wasi_fd_renumber: from={}, to={}", from, to);
    Ok(0)
}

pub fn wasi_fd_sync(
    _caller: Caller<ProcessData>,
    fd: u32,
) -> Result<u32> {
    info!("wasi_fd_sync: fd={}", fd);
    
    // Check if fd is valid
    let process_data = _caller.data();
    let table = process_data.fd_table.lock().unwrap();
    if fd as usize >= table.entries.len() {
        return Ok(8); // WASI_EBADF
    }
    match &table.entries[fd as usize] {
        Some(_) => Ok(0), // Success - no-op since we're working with in-memory files
        None => Ok(8), // WASI_EBADF
    }
}

pub fn wasi_fd_tell(
    mut caller: Caller<ProcessData>,
    fd: u32,
    offset_ptr: u32,
) -> Result<u32> {
    info!("wasi_fd_tell: fd={}, offset_ptr={}", fd, offset_ptr);
    
    // Get current position
    let current_pos = {
        let process_data = caller.data();
        let table = process_data.fd_table.lock().unwrap();
        if fd as usize >= table.entries.len() {
            return Ok(8); // WASI_EBADF
        }
        match &table.entries[fd as usize] {
            Some(FDEntry::File { read_ptr, .. }) => *read_ptr as u64,
            _ => return Ok(8), // WASI_EBADF
        }
    };

    // Write position to memory
    let memory = caller.get_export("memory").unwrap().into_memory().unwrap();
    let mem = memory.data_mut(&mut caller);
    let ptr = offset_ptr as usize;
    if ptr + 8 > mem.len() {
        return Ok(21); // WASI_EFAULT
    }
    mem[ptr..ptr+8].copy_from_slice(&current_pos.to_le_bytes());
    
    Ok(0)
} 