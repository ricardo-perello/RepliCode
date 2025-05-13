use anyhow::Result;
use wasmtime::Caller;
use crate::runtime::process::ProcessData;

pub fn wasi_args_get(
    mut caller: Caller<ProcessData>,
    argv_ptr: u32,
    argv_buf_ptr: u32,
) -> Result<u32> {
    // Clone args to avoid borrow checker issues
    let args = caller.data().args.clone();
    let memory = match caller.get_export("memory") {
        Some(wasmtime::Extern::Memory(mem)) => mem,
        _ => return Ok(1), // WASI_EINVAL
    };
    let mem = memory.data_mut(&mut caller);
    let mut buf_offset = argv_buf_ptr as usize;
    for (i, arg) in args.iter().enumerate() {
        let ptr_offset = argv_ptr as usize + i * 4;
        let arg_bytes = arg.as_bytes();
        let arg_len = arg_bytes.len();
        // Write pointer to this arg in argv[i]
        let ptr = buf_offset as u32;
        mem[ptr_offset..ptr_offset + 4].copy_from_slice(&ptr.to_le_bytes());
        // Write arg string to argv_buf
        mem[buf_offset..buf_offset + arg_len].copy_from_slice(arg_bytes);
        mem[buf_offset + arg_len] = 0; // null terminator
        buf_offset += arg_len + 1;
    }
    Ok(0)
}

pub fn wasi_args_sizes_get(
    mut caller: Caller<ProcessData>,
    argc_ptr: u32,
    argv_buf_size_ptr: u32,
) -> Result<u32> {
    // Clone args to avoid borrow checker issues
    let args = caller.data().args.clone();
    let argc = args.len() as u32;
    let argv_buf_size: u32 = args.iter().map(|a| a.len() as u32 + 1).sum();
    let memory = match caller.get_export("memory") {
        Some(wasmtime::Extern::Memory(mem)) => mem,
        _ => return Ok(1), // WASI_EINVAL
    };
    let mem = memory.data_mut(&mut caller);
    mem[argc_ptr as usize..(argc_ptr as usize + 4)].copy_from_slice(&argc.to_le_bytes());
    mem[argv_buf_size_ptr as usize..(argv_buf_size_ptr as usize + 4)].copy_from_slice(&argv_buf_size.to_le_bytes());
    Ok(0)
}

pub fn wasi_environ_get(
    _caller: Caller<ProcessData>,
    _environ_ptr: u32,
    _environ_buf_ptr: u32,
) -> Result<u32> {
    // TODO: Implement actual environ handling
    Ok(0)
}

pub fn wasi_environ_sizes_get(
    _caller: Caller<ProcessData>,
    _environ_count_ptr: u32,
    _environ_buf_size_ptr: u32,
) -> Result<u32> {
    // TODO: Implement actual environ size calculation
    Ok(0)
} 