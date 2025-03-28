use anyhow::Result;
use crate::{
    consensus_input:: {process_consensus_pipe, process_consensus_file},
    runtime::{
        clock::GlobalClock,
        process::{BlockReason, Process, ProcessState},
    },
};
use std::{collections::VecDeque, fs};
use std::io::Read;
use log::{debug, error, info};
use std::thread;
use std::time::Duration;

/// A dynamic scheduler that runs indefinitely and uses a generic consensus function.
/// The consensus function receives a mutable vector of processes (which may be new or blocked)
/// and updates their state based on external input.
pub fn run_scheduler_dynamic<F>(processes: Vec<Process>, mut consensus_input: F) -> Result<()>
where
    F: FnMut(&mut Vec<Process>) -> Result<bool>,
{
    let mut ready_queue: VecDeque<Process> = processes.into();
    let mut blocked_queue: VecDeque<Process> = VecDeque::new();
    let mut has_more_input = true;

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
                has_more_input = consensus_input(&mut new_processes)?;
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
                has_more_input = consensus_input(&mut all_processes)?;
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
                            Some(BlockReason::Timeout { resume_after }) => GlobalClock::now() >= resume_after,
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
    run_scheduler_dynamic(processes, |processes| {
        // Use the existing process_consensus_file function.
        process_consensus_file(consensus_file, processes)
    })
}

// // /// Wrapper for interactive mode using a live consensus pipe/socket.
pub fn run_scheduler_interactive<R: Read>(processes: Vec<Process>, consensus_pipe: &mut R) -> Result<()> {
    run_scheduler_dynamic(processes, |processes| {
        // Process pipe should keep running indefinitely
        process_consensus_pipe(consensus_pipe, processes)?;
        Ok(true) // Always return true for pipe mode to keep scheduler running
    })
}