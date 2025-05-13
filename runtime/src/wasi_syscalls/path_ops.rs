use anyhow::Result;
use wasmtime::Caller;
use crate::runtime::process::ProcessData;
use crate::runtime::fd_table::FDEntry;
use log::info;
use std::fs;
use std::os::unix::fs::MetadataExt;

pub fn wasi_path_filestat_get(
    mut caller: Caller<ProcessData>,
    fd: u32,
    _flags: u32,
    path_ptr: u32,
    path_len: u32,
    buf_ptr: u32,
) -> anyhow::Result<u32> {
    info!("wasi_path_filestat_get: fd={}, path_ptr={}, path_len={}, buf_ptr={}", fd, path_ptr, path_len, buf_ptr);
    // Get the base directory from fd
    let dir_path = {
        let process_data = caller.data();
        let table = process_data.fd_table.lock().unwrap();
        if fd as usize >= table.entries.len() {
            return Ok(8); // WASI_EBADF
        }
        match &table.entries[fd as usize] {
            Some(FDEntry::File { host_path: Some(path), is_directory: true, .. }) => path.clone(),
            _ => return Ok(8), // WASI_EBADF
        }
    };
    // Read the path string from WASM memory
    let memory = caller.get_export("memory").unwrap().into_memory().unwrap();
    let mem = memory.data(&caller);
    let start = path_ptr as usize;
    let end = start + path_len as usize;
    if end > mem.len() {
        return Ok(21); // WASI_EFAULT
    }
    let rel_path = match std::str::from_utf8(&mem[start..end]) {
        Ok(s) => s,
        Err(_) => return Ok(28), // WASI_EILSEQ (invalid unicode)
    };
    let full_path = std::path::Path::new(&dir_path).join(rel_path.trim_start_matches('/'));
    let meta = match fs::metadata(&full_path) {
        Ok(m) => m,
        Err(_) => return Ok(2), // WASI_ENOENT
    };
    let filetype = if meta.is_dir() { 3u8 } else { 4u8 }; // 3=directory, 4=regular file
    let mut buf = [0u8; 56];
    buf[0..8].copy_from_slice(&meta.dev().to_le_bytes());
    buf[8..16].copy_from_slice(&meta.ino().to_le_bytes());
    buf[16] = filetype;
    buf[20..24].copy_from_slice(&(meta.nlink() as u32).to_le_bytes());
    buf[24..32].copy_from_slice(&meta.size().to_le_bytes());
    buf[32..40].copy_from_slice(&meta.atime().to_le_bytes());
    buf[40..48].copy_from_slice(&meta.mtime().to_le_bytes());
    buf[48..56].copy_from_slice(&meta.ctime().to_le_bytes());
    let mem_mut = memory.data_mut(&mut caller);
    let ptr = buf_ptr as usize;
    if ptr + 56 > mem_mut.len() {
        return Ok(21); // WASI_EFAULT
    }
    mem_mut[ptr..ptr+56].copy_from_slice(&buf);
    Ok(0)
}

pub fn wasi_path_filestat_set_times(
    _caller: Caller<ProcessData>,
    fd: u32,
    flags: u32,
    path_ptr: u32,
    path_len: u32,
    atim: u64,
    mtim: u64,
    fst_flags: u32,
) -> Result<u32> {
    info!("wasi_path_filestat_set_times: fd={}, flags={}, path_ptr={}, path_len={}, atim={}, mtim={}, fst_flags={}", 
        fd, flags, path_ptr, path_len, atim, mtim, fst_flags);
    Ok(0)
}

pub fn wasi_path_link(
    _caller: Caller<ProcessData>,
    old_fd: u32,
    old_flags: u32,
    old_path_ptr: u32,
    old_path_len: u32,
    new_fd: u32,
    new_path_ptr: u32,
    new_path_len: u32,
) -> Result<u32> {
    info!("wasi_path_link: old_fd={}, old_flags={}, old_path_ptr={}, old_path_len={}, new_fd={}, new_path_ptr={}, new_path_len={}", 
        old_fd, old_flags, old_path_ptr, old_path_len, new_fd, new_path_ptr, new_path_len);
    Ok(0)
}

pub fn wasi_path_readlink(
    _caller: Caller<ProcessData>,
    fd: u32,
    path_ptr: u32,
    path_len: u32,
    buf_ptr: u32,
    buf_len: u32,
    nread_ptr: u32,
) -> Result<u32> {
    info!("wasi_path_readlink: fd={}, path_ptr={}, path_len={}, buf_ptr={}, buf_len={}, nread_ptr={}", 
        fd, path_ptr, path_len, buf_ptr, buf_len, nread_ptr);
    Ok(0)
}

pub fn wasi_path_rename(
    _caller: Caller<ProcessData>,
    old_fd: u32,
    old_path_ptr: u32,
    old_path_len: u32,
    new_fd: u32,
    new_path_ptr: u32,
    new_path_len: u32,
) -> Result<u32> {
    info!("wasi_path_rename: old_fd={}, old_path_ptr={}, old_path_len={}, new_fd={}, new_path_ptr={}, new_path_len={}", 
        old_fd, old_path_ptr, old_path_len, new_fd, new_path_ptr, new_path_len);
    Ok(0)
} 