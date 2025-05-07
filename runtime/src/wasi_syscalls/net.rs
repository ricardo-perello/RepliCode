use wasmtime::Caller;
use crate::runtime::process::{BlockReason, ProcessData, ProcessState};
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
    
    // Validate parameters
    if domain != 1 && domain != 2 { // AF_INET (1) or AF_INET6 (2)
        error!("wasi_sock_open: invalid domain {}", domain);
        return 1; // EINVAL
    }
    
    if socktype != 1 && socktype != 2 { // SOCK_STREAM (1) or SOCK_DGRAM (2)
        error!("wasi_sock_open: invalid socktype {}", socktype);
        return 1; // EINVAL
    }
    
    let pid;
    let src_port;
    let fd;
    
    // First handle process data and socket creation
    {
        let process_data = caller.data();
        pid = process_data.id;
        src_port = {
            let mut port = process_data.next_port.lock().unwrap();
            *port += 1;
            *port
        };
        debug!("Allocated port {} for process {}", src_port, pid);

        // Create FD entry for the socket
        let mut table = process_data.fd_table.lock().unwrap();
        fd = table.allocate_fd();
        if fd < 0 {
            error!("wasi_sock_open: no free file descriptors available");
            return 76; // EMFILE
        }
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
            return 1; // EINVAL
        }
    };
    let mem_mut = memory.data_mut(&mut caller);
    let out_ptr = sock_fd_out as usize;
    if out_ptr + 4 > mem_mut.len() {
        error!("sock_open: sock_fd_out pointer out of bounds");
        return 1; // EINVAL
    }
    mem_mut[out_ptr..out_ptr+4].copy_from_slice(&(fd as u32).to_le_bytes());
    debug!("Wrote socket FD {} to memory at offset {}", fd, out_ptr);
    0 // Success
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

pub fn wasi_sock_listen(
    mut caller: Caller<'_, ProcessData>,
    fd: i32,
    backlog: i32,
) -> i32 {
    debug!("wasi_sock_listen called with fd={}, backlog={}", fd, backlog);
    let pid;
    let src_port;
    
    // Get socket FD entry
    {
        let process_data = caller.data();
        pid = process_data.id;
        let table = process_data.fd_table.lock().unwrap();
        if let Some(Some(crate::runtime::fd_table::FDEntry::Socket { local_port, .. })) = table.entries.get(fd as usize) {
            src_port = *local_port;
        } else {
            error!("Invalid socket FD {} for process {}", fd, pid);
            return 1; // Invalid FD
        }
    }
    
    // Queue the listen operation
    {
        let process_data = caller.data();
        let op = NetworkOperation::Listen {
            src_port,
        };
        
        process_data.network_queue.lock().unwrap().push(OutgoingNetworkMessage {
            pid,
            operation: op,
        });
        info!("Queued listen operation for process {}:{}", pid, src_port);
    }
    
    // Block until consensus processes this
    debug!("Blocking process {} for network operation", pid);
    block_process_for_network(&mut caller);
    0 // Success
}

pub fn wasi_sock_accept(
    mut caller: Caller<'_, ProcessData>,
    fd: i32,
    flags: i32,
    fd_out: i32,
) -> i32 {
    debug!("wasi_sock_accept called with fd={}, flags={}, fd_out={}", fd, flags, fd_out);
    let pid;
    let src_port;
    
    // Get socket FD entry
    {
        let process_data = caller.data();
        pid = process_data.id;
        let table = process_data.fd_table.lock().unwrap();
        if let Some(Some(crate::runtime::fd_table::FDEntry::Socket { local_port, .. })) = table.entries.get(fd as usize) {
            src_port = *local_port;
        } else {
            error!("Invalid socket FD {} for process {}", fd, pid);
            return 1; // Invalid FD
        }
    }
    
    // Queue the accept operation
    {
        let process_data = caller.data();
        let op = NetworkOperation::Accept {
            src_port,
        };
        
        process_data.network_queue.lock().unwrap().push(OutgoingNetworkMessage {
            pid,
            operation: op,
        });
        info!("Queued accept operation for process {}:{}", pid, src_port);
    }
    
    // Block until consensus processes this
    debug!("Blocking process {} for network operation", pid);
    block_process_for_network(&mut caller);
    
    // Check if we got a connection
    let has_connection = {
        let process_data = caller.data();
        process_data.nat_table.lock().unwrap().has_pending_accept(pid, src_port)
    };

    if has_connection {
        // Create new FD for the accepted connection
        let new_fd = {
            let process_data = caller.data();
            let mut table = process_data.fd_table.lock().unwrap();
            let new_fd = table.allocate_fd();
            if new_fd < 0 {
                error!("No free file descriptors available for accepted connection");
                return 76; // EMFILE
            }
            table.entries[new_fd as usize] = Some(crate::runtime::fd_table::FDEntry::Socket {
                local_port: src_port,
                connected: true,
            });
            new_fd
        };

        // Write the new FD back to WASM memory
        let memory = match caller.get_export("memory") {
            Some(wasmtime::Extern::Memory(mem)) => mem,
            _ => {
                error!("sock_accept: no memory export found");
                return 1; // EINVAL
            }
        };
        let mem_mut = memory.data_mut(&mut caller);
        let out_ptr = fd_out as usize;
        if out_ptr + 4 > mem_mut.len() {
            error!("sock_accept: fd_out pointer out of bounds");
            return 1; // EINVAL
        }
        mem_mut[out_ptr..out_ptr+4].copy_from_slice(&(new_fd as u32).to_le_bytes());

        // Clear the pending accept
        {
            let process_data = caller.data();
            process_data.nat_table.lock().unwrap().clear_pending_accept(pid, src_port);
        }

        info!("Created new socket FD {} for accepted connection on process {}:{}", new_fd, pid, src_port);
        0 // Success
    } else {
        debug!("No connection available yet for process {}:{}, will retry", pid, src_port);
        11 // EAGAIN - Resource temporarily unavailable
    }
}

pub fn wasi_sock_recv(
    caller: Caller<ProcessData>,
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
    caller: Caller<ProcessData>,
    fd: u32,
    how: u32,
) -> Result<u32> {
    info!("wasi_sock_shutdown: fd={}, how={}", fd, how);
    Ok(0)
}

pub fn wasi_sock_connect(
    mut caller: Caller<'_, ProcessData>,
    fd: i32,
    addr: i32,
    addr_len: i32,
) -> i32 {
    debug!("wasi_sock_connect called with fd={}, addr={}, addr_len={}", 
        fd, addr, addr_len);
    
    let pid;
    let src_port;
    let dest_addr;
    let dest_port;
    
    // First get the memory data for address
    {
        let memory = match caller.get_export("memory") {
            Some(wasmtime::Extern::Memory(mem)) => mem,
            _ => {
                error!("sock_connect: no memory export found");
                return 1; // EINVAL
            }
        };
        let mem = memory.data(&caller);
        if addr as usize + addr_len as usize > mem.len() {
            error!("sock_connect: address out of bounds");
            return 1; // EINVAL
        }
        
        // Parse sockaddr_in structure (assuming IPv4 for now)
        // struct sockaddr_in {
        //     sa_family_t sin_family;  // 2 bytes
        //     in_port_t sin_port;      // 2 bytes
        //     struct in_addr sin_addr; // 4 bytes
        //     char sin_zero[8];        // 8 bytes
        // }
        let addr_bytes = &mem[addr as usize..(addr + addr_len) as usize];
        if addr_bytes.len() < 16 {
            error!("sock_connect: address too short");
            return 1; // EINVAL
        }
        
        // Parse port (network byte order)
        let port_bytes: [u8; 2] = [addr_bytes[2], addr_bytes[3]];
        dest_port = u16::from_be_bytes(port_bytes);
        
        // Parse address (network byte order)
        let addr_bytes: [u8; 4] = [addr_bytes[4], addr_bytes[5], addr_bytes[6], addr_bytes[7]];
        dest_addr = format!("{}.{}.{}.{}", addr_bytes[0], addr_bytes[1], addr_bytes[2], addr_bytes[3]);
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
                return 1; // EINVAL
            }
        };
        
        // Queue the connect operation
        let op = NetworkOperation::Connect {
            dest_addr: dest_addr.clone(),
            dest_port,
            src_port,
        };
        
        process_data.network_queue.lock().unwrap().push(OutgoingNetworkMessage {
            pid,
            operation: op,
        });
        info!("Queued connect operation for process {}:{} -> {}:{}", pid, src_port, dest_addr, dest_port);
    }
    
    // Block until consensus processes this
    debug!("Blocking process {} for network operation", pid);
    block_process_for_network(&mut caller);
    0 // Success
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
