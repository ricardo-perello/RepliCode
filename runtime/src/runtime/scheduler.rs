// runtime/src/runtime/scheduler.rs

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

pub fn run_scheduler<F>(mut processes: Vec<Process>, mut consensus_input: F) -> Result<()>
where
    F: FnMut(&mut Vec<Process>) -> Result<()>,
{
    // Log the scheduler thread name (if available)
    debug!(
        "Scheduler running on thread: {}",
        thread::current().name().unwrap_or("scheduler")
    );

    while !processes.is_empty() {
        // Count the processes that are not blocked.
        let unblocked = processes
            .iter()
            .filter(|p| {
                let state = p.data.state.lock().unwrap();
                *state != ProcessState::Blocked
            })
            .count();
        debug!("Number of unblocked processes: {}", unblocked);

        if unblocked == 0 {
            // All processes are blocked. Process the next batch of consensus input.
            debug!("All processes are blocked. Processing the next batch of consensus input.");
            if let Err(e) = consensus_input(&mut processes) {
                error!("Error processing consensus input: {:?}", e);
                return Err(e);
            }
        }

        // 2. Do one pass over the processes to handle finishing/blocking states.
        let mut found_running = false; // Did we see a process that's already Running?
        let mut next_round = Vec::with_capacity(processes.len());
        for process in processes {
            let state_copy = {
                let guard = process.data.state.lock().unwrap();
                *guard
            };
            match state_copy {
                ProcessState::Finished => {
                    // Wait for the process thread to complete and discard it.
                    let _ = process.thread.join();
                    info!("Process {} finished and joined.", process.id);
                }
                ProcessState::Blocked => {
                    // Possibly unblock if FD input is available or timeout expired.
                    let reason = {
                        let r = process.data.block_reason.lock().unwrap();
                        r.clone()
                    };
                    match reason {
                        Some(BlockReason::StdinRead) => {
                            let fd_has_input = {
                                let fd_table = process.data.fd_table.lock().unwrap();
                                fd_table.has_pending_input(0)
                            };
                            if fd_has_input {
                                let mut st = process.data.state.lock().unwrap();
                                *st = ProcessState::Running;
                                *process.data.block_reason.lock().unwrap() = None;
                                process.data.cond.notify_all();
                                info!(
                                    "Process {} unblocked (stdin read) on thread: {}",
                                    process.id,
                                    thread::current().name().unwrap_or("scheduler")
                                );
                                found_running = true;
                            }
                        }
                        Some(BlockReason::Timeout { resume_after }) => {
                            if GlobalClock::now() >= resume_after {
                                let mut st = process.data.state.lock().unwrap();
                                *st = ProcessState::Running;
                                *process.data.block_reason.lock().unwrap() = None;
                                process.data.cond.notify_all();
                                info!(
                                    "Process {} unblocked (timeout) on thread: {}",
                                    process.id,
                                    thread::current().name().unwrap_or("scheduler")
                                );
                                found_running = true;
                            }
                        }
                        Some(BlockReason::FileIO) => {
                            let file_is_ready = true; // Simplified
                            if file_is_ready {
                                let mut st = process.data.state.lock().unwrap();
                                *st = ProcessState::Running;
                                *process.data.block_reason.lock().unwrap() = None;
                                process.data.cond.notify_all();
                                found_running = true;
                            }
                        }
                        Some(BlockReason::NetworkIO) => {
                            let net_is_ready = true; // Simplified
                            if net_is_ready {
                                let mut st = process.data.state.lock().unwrap();
                                *st = ProcessState::Running;
                                *process.data.block_reason.lock().unwrap() = None;
                                process.data.cond.notify_all();
                                info!(
                                    "Process {} unblocked (network IO) on thread: {}",
                                    process.id,
                                    thread::current().name().unwrap_or("scheduler")
                                );
                                found_running = true;
                            }
                        }
                        // Some(BlockReason::FileIO) => {
                        //     let file_is_ready = true; // or parse from your input
                        //     if file_is_ready {
                        //         let mut st = process.data.state.lock().unwrap();
                        //         *st = ProcessState::Running;
                        //         *process.data.block_reason.lock().unwrap() = None;
                        //         process.data.cond.notify_all();
                        //         found_running = true;
                        //     }
                        // }
                        // Some(BlockReason::NetworkIO) => {
                        //     let net_is_ready = true; // or check from your consensus input
                        //     if net_is_ready {
                        //         let mut st = process.data.state.lock().unwrap();
                        //         *st = ProcessState::Running;
                        //         *process.data.block_reason.lock().unwrap() = None;
                        //         process.data.cond.notify_all();
                        //         found_running = true;
                        //     }
                        // }
                        None => {}
                    }
                    // Keep it for the next round.
                    next_round.push(process);
                }
                ProcessState::Running => {
                    // Already Running.
                    found_running = true;
                    next_round.push(process);
                }
                ProcessState::Ready => {
                    // It's Ready, not Running yet.
                    next_round.push(process);
                }
            }
        }

        // 3. If no process is Running but some are Ready, promote exactly one.
        if !found_running {
            for process in &next_round {
                let mut st = process.data.state.lock().unwrap();
                if *st == ProcessState::Ready {
                    *st = ProcessState::Running;
                    process.data.cond.notify_all();
                    info!(
                        "Promoting process {} from Ready to Running on thread: {}",
                        process.id,
                        thread::current().name().unwrap_or("scheduler")
                    );
                    break; // Only promote one.
                }
            }
        }

        // 4. Next iteration.
        processes = next_round;
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
