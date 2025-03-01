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


        // 2. We'll do one pass over the processes to handle finishing/blocking states.
        let mut found_running = false;  // did we see a process that's *already* Running?
        let mut next_round = Vec::with_capacity(processes.len());

        for process in processes {
            let state_copy = {
                let guard = process.data.state.lock().unwrap();
                *guard
            };

            match state_copy {
                ProcessState::Finished => {
                    // Wait for the process thread to complete.
                    // The thread ended; join and discard it.
                    let _ = process.thread.join();
                }
                ProcessState::Blocked => {
                    // Possibly un-block if FD input or timed out
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
                                found_running = true;
                            }
                        }
                        Some(BlockReason::Timeout { resume_after }) => {
                            if GlobalClock::now() >= resume_after {
                                let mut st = process.data.state.lock().unwrap();
                                *st = ProcessState::Running;
                                *process.data.block_reason.lock().unwrap() = None;
                                process.data.cond.notify_all();
                                found_running = true;
                            }
                        }
                        None => {}
                    }
                    // Keep it for next round whether or not it was unblocked
                    next_round.push(process);
                }
                ProcessState::Running => {
                    // We already have a Running process.
                    found_running = true;
                    next_round.push(process);
                }
                ProcessState::Ready => {
                    // It's Ready but not Running yet. We won't promote it immediately
                    // if we already have something Running. We'll do that after this pass
                    // if we find no Running process at all.
                    next_round.push(process);
                }
            }
        }

        // 3. If we have *no* Running processes but some are Ready, promote exactly one.
        if !found_running {
            for process in &next_round {
                let mut st = process.data.state.lock().unwrap();
                if *st == ProcessState::Ready {
                    *st = ProcessState::Running;
                    process.data.cond.notify_all();
                    println!("Promoting a Ready process to Running");
                    break; // only promote one
                }
            }
        }

        // 4. Next iteration
        processes = next_round;
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