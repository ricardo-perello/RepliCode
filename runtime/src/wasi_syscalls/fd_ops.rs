use anyhow::Result;
use wasmtime::Caller;
use crate::runtime::process::ProcessData;
use log::info;

pub fn wasi_fd_advise(
    caller: Caller<ProcessData>,
    fd: u32,
    offset: u64,
    len: u64,
    advice: u32,
) -> Result<u32> {
    info!("wasi_fd_advise: fd={}, offset={}, len={}, advice={}", fd, offset, len, advice);
    Ok(0)
}

pub fn wasi_fd_allocate(
    caller: Caller<ProcessData>,
    fd: u32,
    offset: u64,
    len: u64,
) -> Result<u32> {
    info!("wasi_fd_allocate: fd={}, offset={}, len={}", fd, offset, len);
    Ok(0)
}

pub fn wasi_fd_datasync(
    caller: Caller<ProcessData>,
    fd: u32,
) -> Result<u32> {
    info!("wasi_fd_datasync: fd={}", fd);
    Ok(0)
}

pub fn wasi_fd_fdstat_set_flags(
    caller: Caller<ProcessData>,
    fd: u32,
    flags: u32,
) -> Result<u32> {
    info!("wasi_fd_fdstat_set_flags: fd={}, flags={}", fd, flags);
    Ok(0)
}

pub fn wasi_fd_fdstat_set_rights(
    caller: Caller<ProcessData>,
    fd: u32,
    fs_rights_base: u64,
    fs_rights_inheriting: u64,
) -> Result<u32> {
    info!("wasi_fd_fdstat_set_rights: fd={}, fs_rights_base={}, fs_rights_inheriting={}", 
        fd, fs_rights_base, fs_rights_inheriting);
    Ok(0)
}

pub fn wasi_fd_filestat_get(
    caller: Caller<ProcessData>,
    fd: u32,
    buf_ptr: u32,
) -> Result<u32> {
    info!("wasi_fd_filestat_get: fd={}, buf_ptr={}", fd, buf_ptr);
    Ok(0)
}

pub fn wasi_fd_filestat_set_size(
    caller: Caller<ProcessData>,
    fd: u32,
    size: u64,
) -> Result<u32> {
    info!("wasi_fd_filestat_set_size: fd={}, size={}", fd, size);
    Ok(0)
}

pub fn wasi_fd_filestat_set_times(
    caller: Caller<ProcessData>,
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
    caller: Caller<ProcessData>,
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
    caller: Caller<ProcessData>,
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
    caller: Caller<ProcessData>,
    from: u32,
    to: u32,
) -> Result<u32> {
    info!("wasi_fd_renumber: from={}, to={}", from, to);
    Ok(0)
}

pub fn wasi_fd_sync(
    caller: Caller<ProcessData>,
    fd: u32,
) -> Result<u32> {
    info!("wasi_fd_sync: fd={}", fd);
    Ok(0)
}

pub fn wasi_fd_tell(
    caller: Caller<ProcessData>,
    fd: u32,
    offset_ptr: u32,
) -> Result<u32> {
    info!("wasi_fd_tell: fd={}, offset_ptr={}", fd, offset_ptr);
    Ok(0)
} 