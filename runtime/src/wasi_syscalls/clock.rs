use anyhow::Result;
use wasmtime::Caller;
use crate::runtime::process::ProcessData;
use crate::runtime::clock::GlobalClock;

// WASI clock IDs
const CLOCK_REALTIME: u32 = 0;
const CLOCK_MONOTONIC: u32 = 1;
const CLOCK_PROCESS_CPUTIME_ID: u32 = 2;
const CLOCK_THREAD_CPUTIME_ID: u32 = 3;

pub fn wasi_clock_res_get(
    mut caller: Caller<ProcessData>,
    clock_id: u32,
    resolution_ptr: u32,
) -> Result<u32> {
    // For deterministic behavior, we'll use a fixed resolution of 1ms
    let resolution: u64 = 1_000_000; // 1ms in nanoseconds
    
    // Write resolution to memory
    let memory = match caller.get_export("memory") {
        Some(wasmtime::Extern::Memory(mem)) => mem,
        _ => return Ok(1), // EINVAL
    };
    
    let mem_mut = memory.data_mut(&mut caller);
    let out_ptr = resolution_ptr as usize;
    if out_ptr + 8 > mem_mut.len() {
        return Ok(1); // EINVAL
    }
    
    // Write resolution as u64 in little-endian
    mem_mut[out_ptr..out_ptr+8].copy_from_slice(&resolution.to_le_bytes());
    
    Ok(0)
}

pub fn wasi_clock_time_get(
    mut caller: Caller<ProcessData>,
    clock_id: u32,
    _precision: u64,
    time_ptr: u32,
) -> Result<u32> {
    // Get current time from our deterministic clock
    let current_time = GlobalClock::now();
    
    // Write time to memory
    let memory = match caller.get_export("memory") {
        Some(wasmtime::Extern::Memory(mem)) => mem,
        _ => return Ok(1), // EINVAL
    };
    
    let mem_mut = memory.data_mut(&mut caller);
    let out_ptr = time_ptr as usize;
    if out_ptr + 8 > mem_mut.len() {
        return Ok(1); // EINVAL
    }
    
    // Write time as u64 in little-endian
    mem_mut[out_ptr..out_ptr+8].copy_from_slice(&current_time.to_le_bytes());
    
    Ok(0)
} 