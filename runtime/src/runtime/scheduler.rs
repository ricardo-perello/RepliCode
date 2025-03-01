use anyhow::Result;
use crate::{
    consensus_input::process_consensus_file,
    runtime::process::{Process, ProcessState, BlockReason},
    runtime::clock::GlobalClock,
};
use std::io::Read;

/// The main scheduler loop now accepts a consensus input closure that only
/// needs a mutable reference to the process list.
pub fn run_scheduler<F>(mut processes: Vec<Process>, mut consensus_input: F) -> Result<()>
where
    F: FnMut(&mut Vec<Process>) -> Result<()>,
{
    while !processes.is_empty() {
        // Count the processes that are not blocked.
        let unblocked = processes.iter().filter(|p| {
            let state = p.data.state.lock().unwrap();
            *state != ProcessState::Blocked
        }).count();

        if unblocked == 0 {
            // All processes are blocked.
            // Process the next batch of consensus input.
            consensus_input(&mut processes)?;
        }

        let mut still_running = Vec::new();
        for process in processes {
            let state_copy = {
                let guard = process.data.state.lock().unwrap();
                *guard
            };
            match state_copy {
                ProcessState::Finished => {
                    // Wait for the process thread to complete.
                    let _ = process.thread.join();
                }
                ProcessState::Blocked => {
                    let reason = {
                        let reason_guard = process.data.block_reason.lock().unwrap();
                        reason_guard.clone()
                    };

                    // If the process is blocked on stdin, check if there's new input.
                    if let Some(BlockReason::StdinRead) = reason {
                        let fd_has_input = {
                            let fd_table = process.data.fd_table.lock().unwrap();
                            fd_table.has_pending_input(0) // FD 0 (stdin) check
                        };

                        if fd_has_input {
                            let mut st = process.data.state.lock().unwrap();
                            *st = ProcessState::Running;
                            let mut reason = process.data.block_reason.lock().unwrap();
                            *reason = None;
                            process.data.cond.notify_all();
                        }
                    } else if let Some(BlockReason::Timeout { resume_after }) = reason {
                        if GlobalClock::now() >= resume_after {
                            let mut st = process.data.state.lock().unwrap();
                            *st = ProcessState::Running;
                            let mut reason = process.data.block_reason.lock().unwrap();
                            *reason = None;
                            process.data.cond.notify_all();
                        }
                    }
                    still_running.push(process);
                }
                ProcessState::Running => {
                    still_running.push(process);
                }
                ProcessState::Ready => {
                    {
                        let mut guard = process.data.state.lock().unwrap();
                        *guard = ProcessState::Running;
                    }
                    process.data.cond.notify_all();
                    still_running.push(process);
                }
            }
        }
        processes = still_running;
    }
    Ok(())
}

/// Wrapper for benchmark mode using a file as consensus input.
pub fn run_scheduler_with_file(processes: Vec<Process>, consensus_file: &str) -> Result<()> {
    run_scheduler(processes, |processes| {
        // Use the existing process_consensus_file function.
        process_consensus_file(consensus_file, processes)
    })
}

/// Wrapper for interactive mode using a live consensus pipe/socket.
pub fn run_scheduler_interactive<R: Read>(processes: Vec<Process>, consensus_pipe: &mut R) -> Result<()> {
    run_scheduler(processes, |processes| {
        let mut buffer = [0u8; 1024];
        let n = consensus_pipe.read(&mut buffer)?;
        if n > 0 {
            println!("Received {} bytes from consensus pipe", n);
            // Here you would process the input from the pipe as needed.
        }
        Ok(())
    })
}