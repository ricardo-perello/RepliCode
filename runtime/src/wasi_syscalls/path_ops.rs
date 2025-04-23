use anyhow::Result;
use wasmtime::Caller;
use crate::runtime::process::ProcessData;
use log::{info, error};

pub fn wasi_path_filestat_get(
    mut caller: Caller<ProcessData>,
    fd: u32,
    flags: u32,
    path_ptr: u32,
    path_len: u32,
    buf_ptr: u32,
) -> Result<u32> {
    info!("wasi_path_filestat_get: fd={}, flags={}, path_ptr={}, path_len={}, buf_ptr={}", 
        fd, flags, path_ptr, path_len, buf_ptr);
    Ok(0)
}

pub fn wasi_path_filestat_set_times(
    mut caller: Caller<ProcessData>,
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
    mut caller: Caller<ProcessData>,
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
    mut caller: Caller<ProcessData>,
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
    mut caller: Caller<ProcessData>,
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