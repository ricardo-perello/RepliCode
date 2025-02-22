use wasmtime::{Caller, Extern};
use std::io::{self, Write};
use std::convert::TryInto;
use crate::runtime::process::{ProcessData, ProcessState};
use std::cmp;
use once_cell::sync::Lazy;
use std::sync::{Mutex, Condvar};




// Define and export the global input buffer.
pub static GLOBAL_INPUT: Lazy<(Mutex<Vec<u8>>, Condvar)> = Lazy::new(|| {
    (Mutex::new(Vec::new()), Condvar::new())
});

/// Dummy implementation for fd_close: simply logs the call.
pub fn wasi_fd_close(_caller: Caller<'_, ProcessData>, fd: i32) -> i32 {
    println!("Called fd_close with fd: {}", fd);
    0
}

/// Dummy implementation for fd_fdstat_get: logs the call.
pub fn wasi_fd_fdstat_get(_caller: Caller<'_, ProcessData>, fd: i32, _buf: i32) -> i32 {
    println!("Called fd_fdstat_get with fd: {}", fd);
    0
}

/// Dummy implementation for fd_seek: logs the call.
pub fn wasi_fd_seek(_caller: Caller<'_, ProcessData>, fd: i32, offset: i64, whence: i32, _newoffset: i32) -> i32 {
    println!("Called fd_seek with fd: {}, offset: {}, whence: {}", fd, offset, whence);
    0
}

pub fn wasi_fd_read(
    mut caller: Caller<'_, ProcessData>,
    fd: i32,
    iovs: i32,
    iovs_len: i32,
    nread: i32,
) -> i32 {
    if fd == 0 {
        // Access the global input buffer.
        let (global_lock, global_cond) = &*GLOBAL_INPUT;
        let mut global_buf = global_lock.lock().unwrap();

        // If no input is available, block this process.
        if global_buf.is_empty() {
            let mut state = caller.data().state.lock().unwrap();
            if *state == ProcessState::Running {
                println!("Blocking process from fd_read (waiting for input)...");
                *state = ProcessState::Blocked;
                caller.data().cond.notify_all();
            }
            drop(state); // Drop process state lock before waiting on global input.
            // Wait until data is available in the global input buffer.
            global_buf = global_cond.wait_while(global_buf, |buf| buf.is_empty()).unwrap();
        }
        

        // At this point, global_buf has some data.
        // Copy as much as possible into WASM memory.

        println!("processState is {:?}", *caller.data().state.lock().unwrap());
        let input = global_buf.clone();
        // Clear the global buffer after copying.
        global_buf.clear();

        let mut total_read = 0;
        let memory = match caller.get_export("memory") {
            Some(Extern::Memory(mem)) => mem,
            _ => {
                eprintln!("Failed to find memory export");
                return 1;
            }
        };
        let data = memory.data_mut(&mut caller);
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
            let to_copy = cmp::min(len, input.len() - total_read);
            data[offset..offset + to_copy].copy_from_slice(&input[total_read..total_read + to_copy]);
            total_read += to_copy;
            if total_read >= input.len() {
                break;
            }
        }
        let total_read_bytes = (total_read as u32).to_le_bytes();
        let nread_ptr = nread as usize;
        if nread_ptr + 4 > data.len() {
            eprintln!("nread pointer out of bounds");
            return 1;
        }
        data[nread_ptr..nread_ptr + 4].copy_from_slice(&total_read_bytes);

        // After reading input, mark process as Running (i.e. unblocked).
        {
            println!("Unblocking process from fd_read after input received");
            let mut s = caller.data().state.lock().unwrap();
            *s = ProcessState::Running;
        }
        caller.data().cond.notify_all();
        0
    } else {
        // For non-stdin fds, return an error.
        eprintln!("fd_read called with unsupported fd: {}", fd);
        1
    }
}
use std::thread;
use std::time::Duration;

/// Real implementation for poll_oneoff.
/// This function reads the first subscription (assumed to be a clock/sleep subscription),
/// extracts the timeout (in nanoseconds), sleeps for that duration,
/// then writes a dummy event back into WASM memory and sets the number of events to 1.
pub fn wasi_poll_oneoff(
    mut caller: Caller<'_, ProcessData>,
    subscriptions_ptr: i32,
    events_ptr: i32,
    nsubscriptions: i32,
    nevents_ptr: i32,
) -> i32 {
    // Get the WASM memory.
    let memory = match caller.get_export("memory") {
        Some(Extern::Memory(mem)) => mem,
        _ => {
            eprintln!("poll_oneoff: Failed to find memory export");
            return 1;
        }
    };
    
    // Get a reference to the memory data for reading.
    let mem_data = memory.data(&caller);
    
    // For simplicity, assume each subscription is 48 bytes.
    let subscription_size = 48;
    let sub_addr = subscriptions_ptr as usize;
    if sub_addr + subscription_size > mem_data.len() {
        eprintln!("poll_oneoff: Subscription out of bounds");
        return 1;
    }
    
    // Read userdata (u64) from offset 0.
    let userdata_bytes = &mem_data[sub_addr..sub_addr+8];
    let userdata = u64::from_le_bytes(userdata_bytes.try_into().unwrap());
    
    // Read the subscription type from offset 8 (u16).
    let type_bytes = &mem_data[sub_addr+8..sub_addr+10];
    let sub_type = u16::from_le_bytes(type_bytes.try_into().unwrap());
    
    // Read the timeout (u64) from offset 16.
    let timeout_bytes = &mem_data[sub_addr+16..sub_addr+24];
    let timeout_nanos = u64::from_le_bytes(timeout_bytes.try_into().unwrap());
    let sleep_nanos = if timeout_nanos == 0 { 9_000_000_000 } else { timeout_nanos }; // TODO fix this part
    let sleep_duration = Duration::from_nanos(sleep_nanos);
    println!("poll_oneoff: Sleeping for {:?}", sleep_duration);
    thread::sleep(sleep_duration);
    println!("poll_oneoff: userdata = {}, type = {}, timeout_nanos = {}", userdata, sub_type, timeout_nanos);
    
    
    
    // Now write an event result back into WASM memory.
    // We assume an event struct of 32 bytes.
    let event_size = 32;
    let events_addr = events_ptr as usize;
    let mut mem_mut = memory.data_mut(&mut caller);
    if events_addr + event_size > mem_mut.len() {
        eprintln!("poll_oneoff: Events area out of bounds");
        return 1;
    }
    
    // Write userdata (copying from the subscription).
    mem_mut[events_addr..events_addr+8].copy_from_slice(&userdata.to_le_bytes());
    // Write error (0) as u16 at offset 8.
    mem_mut[events_addr+8..events_addr+10].copy_from_slice(&0u16.to_le_bytes());
    // Write event type (using the subscription type) as u16 at offset 10.
    mem_mut[events_addr+10..events_addr+12].copy_from_slice(&sub_type.to_le_bytes());
    // Zero the remaining bytes of the event.
    for byte in &mut mem_mut[events_addr+12..events_addr+event_size] {
        *byte = 0;
    }
    
    // Write the number of events (1) as a u64 at the address pointed to by nevents_ptr.
    let nevents_addr = nevents_ptr as usize;
    if nevents_addr + 8 > mem_mut.len() {
        eprintln!("poll_oneoff: nevents pointer out of bounds");
        return 1;
    }
    mem_mut[nevents_addr..nevents_addr+8].copy_from_slice(&1u64.to_le_bytes());
    
    0
}

/// Implementation for fd_write: writes to stdout/stderr.
pub fn wasi_fd_write(
    mut caller: Caller<'_, ProcessData>,
    fd: i32,
    iovs: i32,
    iovs_len: i32,
    nwritten: i32,
) -> i32 {
    let memory = match caller.get_export("memory") {
        Some(Extern::Memory(mem)) => mem,
        _ => {
            eprintln!("Failed to find memory export");
            return 1;
        }
    };
    let data = memory.data(&caller);
    let mut total_written = 0;
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
        let slice = &data[offset..offset + len];
        match fd {
            1 => { io::stdout().write_all(slice).unwrap(); },
            2 => { io::stderr().write_all(slice).unwrap(); },
            _ => {
                eprintln!("fd_write called with unsupported fd: {}", fd);
                return 1;
            }
        }
        total_written += len;
    }
    let total_written_bytes = (total_written as u32).to_le_bytes();
    let nwritten_ptr = nwritten as usize;
    let mem_mut = memory.data_mut(&mut caller);
    if nwritten_ptr + 4 > mem_mut.len() {
        eprintln!("nwritten pointer out of bounds");
        return 1;
    }
    mem_mut[nwritten_ptr..nwritten_ptr + 4].copy_from_slice(&total_written_bytes);
    0
}

/// Implementation for proc_exit: logs and terminates the process.
pub fn wasi_proc_exit(_caller: Caller<'_, ProcessData>, code: i32) {
    println!("Called proc_exit with code: {}", code);
    std::process::exit(code);
}