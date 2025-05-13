use anyhow::Result;
use wasmtime::Caller;
use crate::runtime::process::ProcessData;
use crate::runtime::fd_table::FDEntry;
use log::info;
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
    Ok(0)
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

pub fn wasi_fd_filestat_get(
    mut caller: Caller<ProcessData>,
    fd: u32,
    buf_ptr: u32,
) -> anyhow::Result<u32> {
    info!("wasi_fd_filestat_get: fd={}, buf_ptr={}", fd, buf_ptr);
    let host_path = {
        let process_data = caller.data();
        let table = process_data.fd_table.lock().unwrap();
        if fd as usize >= table.entries.len() {
            return Ok(8); // WASI_EBADF
        }
        match &table.entries[fd as usize] {
            Some(FDEntry::File { host_path: Some(path), .. }) => path.clone(),
            _ => return Ok(8), // WASI_EBADF
        }
    };
    let meta = match fs::metadata(&host_path) {
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
    let memory = caller.get_export("memory").unwrap().into_memory().unwrap();
    let mem = memory.data_mut(&mut caller);
    let ptr = buf_ptr as usize;
    if ptr + 56 > mem.len() {
        return Ok(21); // WASI_EFAULT
    }
    mem[ptr..ptr+56].copy_from_slice(&buf);
    Ok(0)
}

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
    Ok(0)
}

pub fn wasi_fd_tell(
    _caller: Caller<ProcessData>,
    fd: u32,
    offset_ptr: u32,
) -> Result<u32> {
    info!("wasi_fd_tell: fd={}, offset_ptr={}", fd, offset_ptr);
    Ok(0)
} 