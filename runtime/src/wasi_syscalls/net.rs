use wasmtime::Caller;
use crate::runtime::process::{BlockReason, ProcessData, ProcessState};
use std::sync::Arc;
use std::sync::Mutex;
use consensus::commands::NetworkOperation;
use anyhow::Result;
use log::{info, error, debug};

#[derive(Debug, Clone)]
pub struct OutgoingNetworkMessage {
    pub pid: u64,
    pub operation: NetworkOperation,
}

//TODO dummy version, need to talk to gauthier to ensure how it goes through consensus


pub fn wasi_sock_open(
    mut caller: Caller<'_, ProcessData>,
    domain: i32,
    socktype: i32,
    protocol: i32,
    sock_fd_out: i32,
) -> i32 {
    debug!("wasi_sock_open called with domain={}, socktype={}, protocol={}, sock_fd_out={}", 
        domain, socktype, protocol, sock_fd_out);
    let pid;
    let src_port;
    let fd;
    
    // First handle process data and network operations
    {
        let process_data = caller.data();
        pid = process_data.id;
        src_port = {
            let mut port = process_data.next_port.lock().unwrap();
            *port += 1;
            *port
        };
        debug!("Allocated port {} for process {}", src_port, pid);

        // Queue the connect operation
        let op = NetworkOperation::Connect {
            dest_addr: "127.0.0.1".to_string(), // TODO: Get from WASM memory
            dest_port: 8000,                          // TODO: Get from WASM memory
            src_port,
        };
        
        process_data.network_queue.lock().unwrap().push(OutgoingNetworkMessage {
            pid,
            operation: op,
        });
        info!("Queued connect operation for process {}:{}", pid, src_port);
    }
    
    // Block until consensus processes this
    debug!("Blocking process {} for network operation", pid);
    block_process_for_network(&mut caller);
    
    // Create FD entry for the socket
    {
        let process_data = caller.data();
        let mut table = process_data.fd_table.lock().unwrap();
        fd = table.allocate_fd();
        table.entries[fd as usize] = Some(crate::runtime::fd_table::FDEntry::Socket {
            local_port: src_port,
            connected: false,
        });
        info!("Created socket FD {} for process {}:{}", fd, pid, src_port);
    }
    
    // Write FD back to WASM memory
    let memory = match caller.get_export("memory") {
        Some(wasmtime::Extern::Memory(mem)) => mem,
        _ => {
            error!("sock_open: no memory export found");
            return 1;
        }
    };
    let mem_mut = memory.data_mut(&mut caller);
    let out_ptr = sock_fd_out as usize;
    if out_ptr + 4 > mem_mut.len() {
        error!("sock_open: sock_fd_out pointer out of bounds");
        return 1;
    }
    mem_mut[out_ptr..out_ptr+4].copy_from_slice(&(fd as u32).to_le_bytes());
    debug!("Wrote socket FD {} to memory at offset {}", fd, out_ptr);
    0
}

pub fn wasi_sock_send(
    mut caller: Caller<'_, ProcessData>,
    fd: i32,
    si_data: i32,
    si_data_len: i32,
    si_flags: i32,
    ret_data_len: i32,
) -> i32 {
    debug!("wasi_sock_send called with fd={}, si_data={}, si_data_len={}, si_flags={}, ret_data_len={}", 
        fd, si_data, si_data_len, si_flags, ret_data_len);
    let pid;
    let src_port;
    let data;
    
    // First get the memory data
    {
        let memory = match caller.get_export("memory") {
            Some(wasmtime::Extern::Memory(mem)) => mem,
            _ => {
                error!("sock_send: no memory export found");
                return 1;
            }
        };
        let mem = memory.data(&caller);
        data = mem[si_data as usize..(si_data + si_data_len) as usize].to_vec();
        debug!("Read {} bytes from memory for send operation", data.len());
    }

    // Then handle process data
    {
        let process_data = caller.data();
        pid = process_data.id;
        
        // Get socket FD entry
        src_port = {
            let table = process_data.fd_table.lock().unwrap();
            if let Some(Some(crate::runtime::fd_table::FDEntry::Socket { local_port, .. })) = table.entries.get(fd as usize) {
                *local_port
            } else {
                error!("Invalid socket FD {} for process {}", fd, pid);
                return 1; // Invalid FD
            }
        };
        
        // Queue the send operation
        let op = NetworkOperation::Send {
            src_port,
            data: data.clone(),
        };
        
        process_data.network_queue.lock().unwrap().push(OutgoingNetworkMessage {
            pid,
            operation: op,
        });
        info!("Queued send operation for process {}:{} ({} bytes)", pid, src_port, data.len());
    }
    
    // Block until consensus processes this
    debug!("Blocking process {} for network operation", pid);
    block_process_for_network(&mut caller);

    // Write the number of bytes sent back to memory
    {
        let memory = match caller.get_export("memory") {
            Some(wasmtime::Extern::Memory(mem)) => mem,
            _ => {
                error!("sock_send: no memory export found for return value");
                return 1;
            }
        };
        let mem_mut = memory.data_mut(&mut caller);
        let ret_data_len_bytes = (data.len() as u32).to_le_bytes();
        mem_mut[ret_data_len as usize..(ret_data_len + 4) as usize].copy_from_slice(&ret_data_len_bytes);
        debug!("Wrote return value {} to memory at offset {}", data.len(), ret_data_len);
    }
    0
}

pub fn wasi_sock_close(
    mut caller: Caller<'_, ProcessData>,
    fd: i32,
) -> i32 {
    debug!("wasi_sock_close called with fd={}", fd);
    let process_data = caller.data();
    let pid = process_data.id;
    
    // Get socket FD entry
    let src_port = {
        let table = process_data.fd_table.lock().unwrap();
        if let Some(Some(crate::runtime::fd_table::FDEntry::Socket { local_port, .. })) = table.entries.get(fd as usize) {
            *local_port
        } else {
            error!("Invalid socket FD {} for process {}", fd, pid);
            return 1; // Invalid FD
        }
    };
    
    // Queue the close operation
    let op = NetworkOperation::Close {
        src_port,
    };
    
    process_data.network_queue.lock().unwrap().push(OutgoingNetworkMessage {
        pid,
        operation: op,
    });
    info!("Queued close operation for process {}:{}", pid, src_port);
    
    // Block until consensus processes this
    debug!("Blocking process {} for network operation", pid);
    block_process_for_network(&mut caller);
    0
}

/// Example stub for socket listen: 'wasi_sock_listen'
/// This is also not official WASI but helps illustrate how you can block.
pub fn wasi_sock_listen(
    mut caller: Caller<'_, ProcessData>,
    fd: i32,
    backlog: i32,
) -> i32 {
    println!("Called sock_listen on fd={}, backlog={}", fd, backlog);
    // If we want to block (e.g. we cannot listen yet?), do so:
    let can_listen_now = true; // pretend we can listen
    if !can_listen_now {
        block_process_for_network(&mut caller);
        return 0;
    }

    // Otherwise, just print success.
    println!("Socket is now listening!");
    0
}

pub fn wasi_sock_accept(
    mut caller: Caller<ProcessData>,
    fd: u32,
    flags: u32,
    fd_ptr: u32,
) -> Result<u32> {
    info!("wasi_sock_accept: fd={}, flags={}, fd_ptr={}", fd, flags, fd_ptr);
    Ok(0)
}

pub fn wasi_sock_recv(
    mut caller: Caller<ProcessData>,
    fd: u32,
    ri_data_ptr: u32,
    ri_data_len: u32,
    ri_flags: u32,
    ro_datalen_ptr: u32,
    ro_flags_ptr: u32,
) -> Result<u32> {
    info!("wasi_sock_recv: fd={}, ri_data_ptr={}, ri_data_len={}, ri_flags={}, ro_datalen_ptr={}, ro_flags_ptr={}", 
        fd, ri_data_ptr, ri_data_len, ri_flags, ro_datalen_ptr, ro_flags_ptr);
    Ok(0)
}

pub fn wasi_sock_shutdown(
    mut caller: Caller<ProcessData>,
    fd: u32,
    how: u32,
) -> Result<u32> {
    info!("wasi_sock_shutdown: fd={}, how={}", fd, how);
    Ok(0)
}

fn block_process_for_network(caller: &mut Caller<'_, ProcessData>) {
    {
        let mut state = caller.data().state.lock().unwrap();
        if *state == ProcessState::Running {
            debug!("Setting process state to Blocked for network operation");
            *state = ProcessState::Blocked;
        }
        let mut reason = caller.data().block_reason.lock().unwrap();
        *reason = Some(BlockReason::NetworkIO);
        caller.data().cond.notify_all();
    }

    let mut state = caller.data().state.lock().unwrap();
    while *state != ProcessState::Running {
        debug!("Process waiting for network operation to complete");
        state = caller.data().cond.wait(state).unwrap();
    }
    debug!("Process resumed after network operation");
}
