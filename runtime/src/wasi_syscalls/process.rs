use anyhow::Result;
use wasmtime::Caller;
use crate::runtime::process::ProcessData;
use log::info;


pub fn wasi_proc_raise(
    caller: Caller<ProcessData>,
    signal: u32,
) -> Result<u32> {
    info!("wasi_proc_raise: signal={}", signal);
    Ok(0)
}

pub fn wasi_sched_yield(
    caller: Caller<ProcessData>,
) -> Result<u32> {
    info!("wasi_sched_yield");
    Ok(0)
}

pub fn wasi_random_get(
    caller: Caller<ProcessData>,
    buf_ptr: u32,
    buf_len: u32,
) -> Result<u32> {
    info!("wasi_random_get: buf_ptr={}, buf_len={}", buf_ptr, buf_len);
    Ok(0)
} 