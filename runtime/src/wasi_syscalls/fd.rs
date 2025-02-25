use wasmtime::{Caller, Extern};
use std::io::{self, Write};
use std::convert::TryInto;
use crate::runtime::process::{BlockReason, ProcessData, ProcessState};
use crate::runtime::clock::GlobalClock;


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
    loop {
        // Lock the FD table and try to extract data.
        let (data_to_read, bytes_to_advance) = {
            let process_data = caller.data();
            let mut table = process_data.fd_table.lock().unwrap();
            let fd_entry = match table.get_fd_entry_mut(fd) {
                Some(entry) => entry,
                None => {
                    eprintln!("fd_read called with invalid FD: {}", fd);
                    return 1;
                }
            };

            // If no data is available, then we want to block.
            if fd_entry.read_ptr >= fd_entry.buffer.len() {
                // Drop the lock and wait until input is available.
                drop(table);
                block_process_for_stdin(&mut caller);
                continue;
            }
            // Otherwise, clone available data.
            let available_data = &fd_entry.buffer[fd_entry.read_ptr..];
            (available_data.to_vec(), available_data.len())
        };

        // At this point, data is available, so proceed to copy it into the WASM memory.
        let memory = match caller.get_export("memory") {
            Some(Extern::Memory(mem)) => mem,
            _ => {
                eprintln!("fd_read: Failed to find memory export");
                return 1;
            }
        };

        {
            // First, determine how many bytes will be read using the iovecs.
            let mut total_read = 0;
            {
                let data = memory.data(&caller);
                for i in 0..iovs_len {
                    let iovec_addr = (iovs as usize) + (i as usize) * 8;
                    if iovec_addr + 8 > data.len() {
                        eprintln!("iovec out of bounds");
                        return 1;
                    }
                    let offset_bytes: [u8; 4] =
                        data[iovec_addr..iovec_addr + 4].try_into().unwrap();
                    let len_bytes: [u8; 4] =
                        data[iovec_addr + 4..iovec_addr + 8].try_into().unwrap();
                    let offset = u32::from_le_bytes(offset_bytes) as usize;
                    let len = u32::from_le_bytes(len_bytes) as usize;
                    if offset + len > data.len() {
                        eprintln!("data slice out of bounds");
                        return 1;
                    }
                    let to_copy = std::cmp::min(len, data_to_read.len() - total_read);
                    if to_copy == 0 {
                        break;
                    }
                    total_read += to_copy;
                    if total_read >= data_to_read.len() {
                        break;
                    }
                }
            }

            // Now actually write the data into the WASM memory.
            let data_mut = memory.data_mut(&mut caller);
            let mut total_read = 0;
            for i in 0..iovs_len {
                let iovec_addr = (iovs as usize) + (i as usize) * 8;
                if iovec_addr + 8 > data_mut.len() {
                    eprintln!("iovec out of bounds");
                    return 1;
                }
                let offset_bytes: [u8; 4] =
                    data_mut[iovec_addr..iovec_addr + 4].try_into().unwrap();
                let len_bytes: [u8; 4] =
                    data_mut[iovec_addr + 4..iovec_addr + 8].try_into().unwrap();
                let offset = u32::from_le_bytes(offset_bytes) as usize;
                let len = u32::from_le_bytes(len_bytes) as usize;
                if offset + len > data_mut.len() {
                    eprintln!("data slice out of bounds");
                    return 1;
                }
                let to_copy = std::cmp::min(len, data_to_read.len() - total_read);
                if to_copy == 0 {
                    break;
                }
                data_mut[offset..offset + to_copy]
                    .copy_from_slice(&data_to_read[total_read..total_read + to_copy]);
                total_read += to_copy;
                if total_read >= data_to_read.len() {
                    break;
                }
            }
            // Write the total number of bytes read into memory.
            let total_read_bytes = (total_read as u32).to_le_bytes();
            let nread_ptr = nread as usize;
            if nread_ptr + 4 > data_mut.len() {
                eprintln!("nread pointer out of bounds");
                return 1;
            }
            data_mut[nread_ptr..nread_ptr + 4].copy_from_slice(&total_read_bytes);
        }

        // After reading, update the FD's read pointer.
        {
            let process_data = caller.data();
            let mut table = process_data.fd_table.lock().unwrap();
            let fd_entry = table.get_fd_entry_mut(fd).unwrap();
            fd_entry.read_ptr += bytes_to_advance;
        }
        return 0;
    }
}


/// Blocks the process, telling the scheduler we're waiting on stdin.
fn block_process_for_stdin(caller: &mut Caller<'_, ProcessData>) {
    {
        let mut st = caller.data().state.lock().unwrap();
        if *st == ProcessState::Running {
            println!("fd_read: Setting process state to Blocked");
            *st = ProcessState::Blocked;
        }
        let mut reason = caller.data().block_reason.lock().unwrap();
        *reason = Some(BlockReason::StdinRead);
        // Notify the scheduler that we’re now waiting.
        caller.data().cond.notify_all();
    }

    // Now wait until the state changes.
    let mut state = caller.data().state.lock().unwrap();
    while *state == ProcessState::Blocked {
        // This call drops the lock while waiting and reacquires it when notified.
        state = caller.data().cond.wait(state).unwrap();
    }
}


pub fn wasi_poll_oneoff(
    mut caller: Caller<'_, ProcessData>,
    subscriptions_ptr: i32,
    events_ptr: i32,
    nsubscriptions: i32,
    nevents_ptr: i32,
) -> i32 {
    // Get the memory export.
    let memory = match caller.get_export("memory") {
        Some(Extern::Memory(mem)) => mem,
        _ => {
            eprintln!("poll_oneoff: Failed to find memory export");
            return 1;
        }
    };

    // Read subscription data (assuming a single subscription for simplicity).
    let mem_data = memory.data(&caller);
    let subscription_size = 48;
    let sub_addr = subscriptions_ptr as usize;
    if sub_addr + subscription_size > mem_data.len() {
        eprintln!("poll_oneoff: Subscription out of bounds");
        return 1;
    }
    // Extract userdata (u64), type (u16) and timeout (u64) from the subscription.
    let userdata_bytes = &mem_data[sub_addr..sub_addr + 8];
    let userdata = u64::from_le_bytes(userdata_bytes.try_into().unwrap());
    let type_bytes = &mem_data[sub_addr + 8..sub_addr + 10];
    let sub_type = u16::from_le_bytes(type_bytes.try_into().unwrap());
    // Instead of sub_addr + 16..sub_addr + 24
    let timeout_bytes = &mem_data[sub_addr + 24..sub_addr + 32];
    let timeout_nanos = u64::from_le_bytes(timeout_bytes.try_into().unwrap());

    // Instead of sleeping, set the process to block until the clock reaches wake_time.
    let sleep_nanos = if timeout_nanos == 0 { 1_000_000_000 } else { timeout_nanos };
    println!("poll_oneoff: Blocking process for {} nanoseconds", sleep_nanos);
    let wake_time = GlobalClock::now() + sleep_nanos;

    {
        let process_data = caller.data();
        let mut state = process_data.state.lock().unwrap();
        let mut reason = process_data.block_reason.lock().unwrap();
        *reason = Some(BlockReason::Timeout { resume_after: wake_time });
        *state = ProcessState::Blocked;
        process_data.cond.notify_all();
    }

    // Wait until the scheduler unblocks the process.
    {
        let mut state = caller.data().state.lock().unwrap();
        while *state == ProcessState::Blocked {
            state = caller.data().cond.wait(state).unwrap();
        }
    } // The lock on state is dropped here.

    // Once unblocked, write a dummy event back to WASM memory.
    {
        let event_size = 32;
        let events_addr = events_ptr as usize;
        let mem_mut = memory.data_mut(&mut caller);
        if events_addr + event_size > mem_mut.len() {
            eprintln!("poll_oneoff: Events area out of bounds");
            return 1;
        }
        // Write userdata.
        mem_mut[events_addr..events_addr + 8].copy_from_slice(&userdata.to_le_bytes());
        // Write error code (0 for success) as u16.
        mem_mut[events_addr + 8..events_addr + 10].copy_from_slice(&0u16.to_le_bytes());
        // Write the event type.
        mem_mut[events_addr + 10..events_addr + 12].copy_from_slice(&sub_type.to_le_bytes());
        // Zero the remaining bytes.
        for byte in &mut mem_mut[events_addr + 12..events_addr + event_size] {
            *byte = 0;
        }
        // Write the number of events (1) to nevents_ptr.
        let nevents_addr = nevents_ptr as usize;
        if nevents_addr + 8 > mem_mut.len() {
            eprintln!("poll_oneoff: nevents pointer out of bounds");
            return 1;
        }
        mem_mut[nevents_addr..nevents_addr + 8].copy_from_slice(&1u64.to_le_bytes());
    }
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
            eprintln!("fd_write: Failed to find memory export");
            return 1;
        }
    };

    // First, use an immutable borrow to read the iovec information.
    let total_written = {
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
        total_written
    };

    // Then use a mutable borrow to write the nwritten value back into memory.
    {
        let total_written_bytes = (total_written as u32).to_le_bytes();
        let nwritten_ptr = nwritten as usize;
        let mem_mut = memory.data_mut(&mut caller);
        if nwritten_ptr + 4 > mem_mut.len() {
            eprintln!("nwritten pointer out of bounds");
            return 1;
        }
        mem_mut[nwritten_ptr..nwritten_ptr + 4].copy_from_slice(&total_written_bytes);
    }
    0
}

/// Implementation for proc_exit: logs and terminates the process.
pub fn wasi_proc_exit(_caller: Caller<'_, ProcessData>, code: i32) {
    println!("Called proc_exit with code: {}", code);
    std::process::exit(code);
}
