use anyhow::Result;
use crate::{
    consensus_input::{process_consensus_file, process_consensus_pipe},
    runtime::{
        clock::GlobalClock,
        process::{BlockReason, Process, ProcessState},
    },
};
use std::io::Read;
use log::{debug, error, info};
use std::thread;
use std::time::Duration;

pub fn run_scheduler<F>(processes: Vec<Process>, mut consensus_input: F) -> Result<()>
where
    F: FnMut(&mut Vec<Process>) -> Result<()>,
{
    // Initialize the queues. Initially, all processes are in the ready queue.
    let mut ready_queue: Vec<Process> = processes;
    let mut blocked_queue: Vec<Process> = Vec::new();

    debug!(
        "Scheduler running on thread: {}",
        thread::current().name().unwrap_or("scheduler")
    );

    // Main scheduling loop: continue until both queues are empty.
    while !ready_queue.is_empty() || !blocked_queue.is_empty() {
        // Process the ready queue.
        while let Some(proc) = ready_queue.pop() {
            {
                // Set the process state to Running.
                let mut st = proc.data.state.lock().unwrap();
                *st = ProcessState::Running;
                proc.data.cond.notify_all();
                info!(
                    "Process {} marked as Running on thread: {}",
                    proc.id,
                    thread::current().name().unwrap_or("scheduler")
                );
            }

            // Wait until the process is no longer Running.
            {
                let mut st = proc.data.state.lock().unwrap();
                while *st == ProcessState::Running {
                    debug!(
                        "Scheduler waiting for process {} (current state: {:?})",
                        proc.id, *st
                    );
                    st = proc.data.cond.wait(st).unwrap();
                }
            }

            // Now, decide where to put the process based on its new state.
            let current_state = {
                let st = proc.data.state.lock().unwrap();
                *st
            };

            match current_state {
                ProcessState::Finished => {
                    let _ = proc.thread.join();
                    info!("Process {} finished and joined.", proc.id);
                }
                ProcessState::Ready => {
                    // Process yielded: push it at the back of the ready queue.
                    info!("Process {} yielded; moving it to Ready queue.", proc.id);
                    ready_queue.push(proc);
                }
                ProcessState::Blocked => {
                    // Process is blocked: push it into the blocked queue.
                    info!("Process {} blocked; moving it to Blocked queue.", proc.id);
                    blocked_queue.push(proc);
                }
                ProcessState::Running => {
                    // This should never happen because we waited until it was no longer Running.
                    error!("Process {} still Running unexpectedly.", proc.id);
                }
            }
        }

        // If there are no ready processes, call consensus input to update process states
        // and then check whether blocked processes can be unblocked.
        if ready_queue.is_empty() && !blocked_queue.is_empty() {
            // First, call consensus input on all processes.
            let mut all_processes: Vec<Process> = ready_queue.drain(..)
                .chain(blocked_queue.drain(..))
                .collect();
            consensus_input(&mut all_processes)?;
            info!("No ready processes; consensus input updated process states.");

            // Re-split the processes into ready and blocked queues.
            for proc in all_processes.into_iter() {
                let state = { *proc.data.state.lock().unwrap() };
                match state {
                    ProcessState::Ready => ready_queue.push(proc),
                    ProcessState::Blocked => blocked_queue.push(proc),
                    ProcessState::Finished => {
                        let _ = proc.thread.join();
                        info!("Process {} finished and joined.", proc.id);
                    }
                    ProcessState::Running => {
                        error!("Process {} still Running unexpectedly after consensus input.", proc.id);
                    }
                }
            }

            // Next, examine each process in the blocked queue to see if it can be unblocked.
            let mut still_blocked = Vec::new();
            for proc in blocked_queue.drain(..) {
                let unblocked = {
                    let reason = proc.data.block_reason.lock().unwrap().clone();
                    match reason {
                        Some(BlockReason::StdinRead) => {
                            // Check if FD 0 has input.
                            let fd_has_input = {
                                let fd_table = proc.data.fd_table.lock().unwrap();
                                fd_table.has_pending_input(0)
                            };
                            fd_has_input
                        }
                        Some(BlockReason::Timeout { resume_after }) => GlobalClock::now() >= resume_after,
                        // Add additional conditions here if needed.
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
                    info!(
                        "Process {} unblocked and moved to Ready queue on thread: {}",
                        proc.id,
                        thread::current().name().unwrap_or("scheduler")
                    );
                    ready_queue.push(proc);
                } else {
                    still_blocked.push(proc);
                }
            }
            blocked_queue = still_blocked;

            // If still no process is ready, sleep briefly before the next check.
            if ready_queue.is_empty() {
                debug!("No processes unblocked; scheduler sleeping briefly.");
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
    Ok(())
}

pub fn run_scheduler_with_file(processes: Vec<Process>, consensus_file: &str) -> Result<()> {
    run_scheduler(processes, |processes| {
        process_consensus_file(consensus_file, processes)
    })
}

/// Wrapper for interactive mode using a live consensus pipe/socket.
pub fn run_scheduler_interactive<R: Read>(processes: Vec<Process>, consensus_pipe: &mut R) -> Result<()> {
    run_scheduler(processes, |processes| {
        process_consensus_pipe(consensus_pipe, processes)
    })
}