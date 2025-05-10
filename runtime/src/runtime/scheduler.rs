use anyhow::Result;
use crate::{
    consensus_input:: {process_consensus_file, process_consensus_pipe},
    runtime::{
        clock::GlobalClock,
        process::{BlockReason, Process, ProcessState},
    }, wasi_syscalls::fs::flush_write_buffer_for_scheduler,
};
use std::{collections::VecDeque, fs};
use std::io::{Read, Write};
use log::{debug, error, info};
use std::thread;
use std::time::Duration;
use crate::wasi_syscalls::net::OutgoingNetworkMessage;
use crate::runtime::fd_table::FDEntry;

struct BatchCollector {
    outgoing_messages: Vec<OutgoingNetworkMessage>,
    batch_start_time: u64,
}

impl BatchCollector {
    fn new() -> Self {
        BatchCollector {
            outgoing_messages: Vec::new(),
            batch_start_time: GlobalClock::now(),
        }
    }

    fn collect_network_messages(&mut self, processes: &[Process]) {
        for process in processes {
            let mut queue = process.data.network_queue.lock().unwrap();
            while let Some(msg) = queue.pop() {
                self.outgoing_messages.push(msg);
            }
        }
    }
}

/// A dynamic scheduler that runs indefinitely and uses a generic consensus function.
/// The consensus function receives a mutable vector of processes (which may be new or blocked)
/// and updates their state based on external input.
pub fn run_scheduler_dynamic<F>(processes: Vec<Process>, mut consensus_input: F) -> Result<()>
where
    F: FnMut(&mut Vec<Process>, Vec<OutgoingNetworkMessage>) -> Result<bool>,
{
    let mut ready_queue: VecDeque<Process> = processes.into();
    let mut blocked_queue: VecDeque<Process> = VecDeque::new();
    let mut has_more_input = true;
    let mut batch_collector = BatchCollector::new();

    debug!(
        "Dynamic scheduler running on thread: {}",
        thread::current().name().unwrap_or("scheduler")
    );

    while has_more_input || !ready_queue.is_empty() || !blocked_queue.is_empty() {
        // Process all ready processes.
        while let Some(proc) = ready_queue.pop_front() {
            {
                // Set process state to Running and notify.
                let mut st = proc.data.state.lock().unwrap();
                *st = ProcessState::Running;
                proc.data.cond.notify_all();
                info!(
                    "Process {} set to Running on thread: {}",
                    proc.id,
                    thread::current().name().unwrap_or("scheduler")
                );
            }

            // Wait until the process is no longer Running.
            {
                let mut st = proc.data.state.lock().unwrap();
                while *st == ProcessState::Running {
                    debug!(
                        "Dynamic scheduler waiting for process {} (state: {:?})",
                        proc.id, *st
                    );
                    st = proc.data.cond.wait(st).unwrap();
                }
            }

            // Check new state and decide where to enqueue.
            let current_state = { *proc.data.state.lock().unwrap() };
            match current_state {
                ProcessState::Finished => {
                    let _ = proc.thread.join();
                    if let Err(e) = fs::remove_dir_all(&proc.data.root_path) {
                        error!("Failed to remove dir for process {}: {}", proc.id, e);
                    }
                    info!("Process {} finished and joined.", proc.id);
                }
                ProcessState::Ready => {
                    info!("Process {} yielded; moving it to Ready queue.", proc.id);
                    ready_queue.push_back(proc);
                }
                ProcessState::Blocked => {
                    info!("Process {} blocked; moving it to Blocked queue.", proc.id);
                    blocked_queue.push_back(proc);
                }
                ProcessState::Running => {
                    error!("Process {} still Running unexpectedly.", proc.id);
                }
            }
        }

        // When no process is ready, try to update states via the consensus input.
        if ready_queue.is_empty() {
            if blocked_queue.is_empty() {
                debug!("No processes in queue; waiting for consensus input.");
                let mut new_processes = Vec::new();
                batch_collector.collect_network_messages(&new_processes);
                has_more_input = consensus_input(&mut new_processes, batch_collector.outgoing_messages.drain(..).collect())?;
                ready_queue.extend(new_processes);

                if ready_queue.is_empty() && !has_more_input {
                    info!("All processes finished and no more consensus input. Exiting scheduler.");
                    break;
                }

                if ready_queue.is_empty() && has_more_input {
                    // We have more input but no processes - sleep briefly
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
            } else {
                // Combine blocked processes and update their states.
                let mut all_processes: Vec<Process> = blocked_queue.drain(..).collect();
                batch_collector.collect_network_messages(&all_processes);
                has_more_input = consensus_input(&mut all_processes, batch_collector.outgoing_messages.drain(..).collect())?;
                info!("All processes blocked; consensus input updated process states.");

                // Re-split processes based on new state.
                for proc in all_processes.into_iter() {
                    let state = { *proc.data.state.lock().unwrap() };
                    match state {
                        ProcessState::Ready => ready_queue.push_back(proc),
                        ProcessState::Blocked => blocked_queue.push_back(proc),
                        ProcessState::Finished => {
                            if let Err(e) = fs::remove_dir_all(&proc.data.root_path) {
                                if e.kind() != std::io::ErrorKind::NotFound {
                                    error!("Failed to remove dir for process {}: {}", proc.id, e);
                                }
                            }
                            let _ = proc.thread.join();
                            info!("Process {} finished and joined.", proc.id);
                        }
                        ProcessState::Running => {
                            error!("Process {} still Running unexpectedly after consensus input.", proc.id);
                        }
                    }
                }

                // Try to unblock processes based on their block reasons.
                let mut still_blocked = VecDeque::new();
                while let Some(proc) = blocked_queue.pop_front() {
                    let unblocked = {
                        let reason = proc.data.block_reason.lock().unwrap().clone();
                        match reason {
                            Some(BlockReason::StdinRead) => {
                                let fd_has_input = {
                                    let fd_table = proc.data.fd_table.lock().unwrap();
                                    fd_table.has_pending_input(0)
                                };
                                fd_has_input
                            }
                            Some(BlockReason::WriteIO(ref path)) => {
                                match flush_write_buffer_for_scheduler(&proc.data, path) {
                                    Ok(_bytes) => true,  // Flushed successfully: unblock the process.
                                    Err(_errno) => false // If flush fails, keep the process blocked.
                                }
                            }
                            Some(BlockReason::Timeout { resume_after }) => GlobalClock::now() >= resume_after,
                            Some(BlockReason::NetworkIO) => {
                                let nat_table = proc.data.nat_table.lock().unwrap();
                                let fd_table = proc.data.fd_table.lock().unwrap();
                                
                                let mut should_block = false;
                                for entry in fd_table.entries.iter() {
                                    if let Some(FDEntry::Socket { local_port, .. }) = entry {
                                        if nat_table.is_waiting_for_accept(proc.id, *local_port) || 
                                           nat_table.is_waiting_for_recv(proc.id, *local_port) {
                                            should_block = true;
                                        break;
                                        }
                                    }
                                }
                                !should_block
                            },
                            _ => false,
                        }
                    };

                    if unblocked {
                        {
                            let mut st = proc.data.state.lock().unwrap();
                            *st = ProcessState::Ready;
                        }
                        {
                            let mut reason = proc.data.block_reason.lock().unwrap();
                            *reason = None;
                        }
                        proc.data.cond.notify_all();
                        info!("Process {} unblocked and moved to Ready queue.", proc.id);
                        ready_queue.push_back(proc);
                    } else {
                        still_blocked.push_back(proc);
                    }
                }
                blocked_queue = still_blocked;

                if ready_queue.is_empty() && blocked_queue.is_empty() && !has_more_input {
                    info!("All processes finished and no more consensus input. Exiting scheduler.");
                    break;
                }

                if ready_queue.is_empty() {
                    debug!("No processes unblocked; scheduler sleeping briefly.");
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }

    info!("Scheduler exiting: no more processes to run and no more input.");
    Ok(())
}


pub fn run_scheduler_with_file(processes: Vec<Process>, consensus_file: &str) -> Result<()> {
    run_scheduler_dynamic(processes, |processes, _| {
        // Use the existing process_consensus_file function.
        process_consensus_file(consensus_file, processes)
    })
}

// // /// Wrapper for interactive mode using a live consensus pipe/socket.
pub fn run_scheduler_interactive<R: Read + Write>(processes: Vec<Process>, consensus_pipe: &mut R) -> Result<()> {
    run_scheduler_dynamic(processes, |processes, outgoing_messages| {
        // Process pipe should keep running indefinitely
        process_consensus_pipe(consensus_pipe, processes, outgoing_messages)?;
        Ok(true) // Always return true for pipe mode to keep scheduler running
    })
}