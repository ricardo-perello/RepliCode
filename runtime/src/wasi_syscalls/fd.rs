use wasmtime::{Caller, Extern};
use std::io::{self, Write};
use std::convert::TryInto;

// Dummy implementation for fd_close: logs the call.
pub fn wasi_fd_close(_caller: Caller<'_, ()>, fd: i32) -> i32 {
    println!("Called fd_close with fd: {}", fd);
    0
}

// Dummy implementation for fd_fdstat_get: logs the call.
pub fn wasi_fd_fdstat_get(_caller: Caller<'_, ()>, fd: i32, _buf: i32) -> i32 {
    println!("Called fd_fdstat_get with fd: {}", fd);
    0
}

// Dummy implementation for fd_seek: logs the call.
pub fn wasi_fd_seek(_caller: Caller<'_, ()>, fd: i32, offset: i64, whence: i32, _newoffset: i32) -> i32 {
    println!("Called fd_seek with fd: {}, offset: {}, whence: {}", fd, offset, whence);
    0
}
pub fn wasi_fd_read(
    mut caller: Caller<'_, ()>,
    fd: i32,
    iovs: i32,
    iovs_len: i32,
    nread: i32,
) -> i32 {
    use std::io::Read;

    let memory = match caller.get_export("memory") {
        Some(Extern::Memory(mem)) => mem,
        _ => {
            eprintln!("Failed to find memory export");
            return 1;
        }
    };

    let data = memory.data_mut(&mut caller);
    let mut total_read = 0;

    for i in 0..iovs_len {
        let iovec_addr = (iovs as usize) + (i as usize) * 8;
        if iovec_addr + 8 > data.len() {
            eprintln!("iovec out of bounds");
            return 1;
        }
        let offset_bytes: [u8; 4] = data[iovec_addr..iovec_addr + 4].try_into().unwrap();
        let len_bytes: [u8; 4] = data[iovec_addr + 4..iovec_addr + 8].try_into().unwrap();
        let offset = u32::from_le_bytes(offset_bytes) as usize;
        let len = u32::from_le_bytes(len_bytes) as usize;

        if offset + len > data.len() {
            eprintln!("data slice out of bounds");
            return 1;
        }

        let mut buffer = vec![0; len];
        let bytes_read = match fd {
            0 => { // stdin
                io::stdin().read(&mut buffer).unwrap_or(0)
            }
            _ => {
                eprintln!("fd_read called with unsupported fd: {}", fd);
                return 1;
            }
        };

        data[offset..offset + bytes_read].copy_from_slice(&buffer[..bytes_read]);
        total_read += bytes_read;
    }

    let total_read_bytes = (total_read as u32).to_le_bytes();
    let nread_ptr = nread as usize;
    if nread_ptr + 4 > data.len() {
        eprintln!("nread pointer out of bounds");
        return 1;
    }
    data[nread_ptr..nread_ptr + 4].copy_from_slice(&total_read_bytes);

    0
}

use std::thread;
use std::time::Duration;

pub fn wasi_poll_oneoff(
    _caller: Caller<'_, ()>,
    subscriptions_ptr: i32,
    events_ptr: i32,
    nsubscriptions: i32,
    nevents_ptr: i32,
) -> i32 {
    println!(
        "Called poll_oneoff: subscriptions_ptr={}, events_ptr={}, nsubscriptions={}, nevents_ptr={}",
        subscriptions_ptr, events_ptr, nsubscriptions, nevents_ptr
    );

    // Simulate waiting by sleeping for 1ms.
    thread::sleep(Duration::from_millis(1));

    // For now, just return 0, indicating success.
    0
}
// Implementation for fd_write: reads the iovecs from memory, writes data to stdout/stderr,
// writes the total number of bytes written to the provided memory location, and returns 0 on success.
pub fn wasi_fd_write(
    mut caller: Caller<'_, ()>,
    fd: i32,
    iovs: i32,
    iovs_len: i32,
    nwritten: i32,
) -> i32 {
    // Get the module's linear memory.
    let memory = match caller.get_export("memory") {
        Some(Extern::Memory(mem)) => mem,
        _ => {
            eprintln!("Failed to find memory export");
            return 1;
        }
    };

    let data = memory.data(&caller);
    let mut total_written = 0;

    // Each iovec is 8 bytes: 4 bytes for the offset and 4 bytes for the length.
    for i in 0..iovs_len {
        let iovec_addr = (iovs as usize) + (i as usize) * 8;
        if iovec_addr + 8 > data.len() {
            eprintln!("iovec out of bounds");
            return 1;
        }
        let offset_bytes: [u8; 4] = data[iovec_addr..iovec_addr+4].try_into().unwrap();
        let len_bytes: [u8; 4] = data[iovec_addr+4..iovec_addr+8].try_into().unwrap();
        let offset = u32::from_le_bytes(offset_bytes) as usize;
        let len = u32::from_le_bytes(len_bytes) as usize;

        if offset + len > data.len() {
            eprintln!("data slice out of bounds");
            return 1;
        }
        let slice = &data[offset..offset+len];

        match fd {
            1 => {
                io::stdout().write_all(slice).unwrap();
            }
            2 => {
                io::stderr().write_all(slice).unwrap();
            }
            _ => {
                eprintln!("fd_write called with unsupported fd: {}", fd);
                return 1;
            }
        }
        total_written += len;
    }

    // Write the total number of bytes written into memory at address 'nwritten'.
    let total_written_bytes = (total_written as u32).to_le_bytes();
    let nwritten_ptr = nwritten as usize;
    let mem_mut = memory.data_mut(&mut caller);
    if nwritten_ptr + 4 > mem_mut.len() {
        eprintln!("nwritten pointer out of bounds");
        return 1;
    }
    mem_mut[nwritten_ptr..nwritten_ptr+4].copy_from_slice(&total_written_bytes);

    0
}

// Implementation for proc_exit: logs and exits.
pub fn wasi_proc_exit(_caller: Caller<'_, ()>, code: i32) {
    println!("Called proc_exit with code: {}", code);
    std::process::exit(code);
}