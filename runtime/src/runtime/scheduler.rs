use anyhow::Result;
use std::sync::MutexGuard;
use crate::runtime::process::{Process, ProcessState};

pub fn run_scheduler(mut processes: Vec<Process>) -> Result<()> {
    while !processes.is_empty() {
        let mut still_running = Vec::new();

        // Sequentially go through them:
        for process in processes {
            // Lock the state to see if it's Blocked, Running, or Finished 

            let process_state = {
                let guard = process.state.lock().unwrap();
                *guard // copy out the enum variant
            };

            match process_state {
                ProcessState::Finished => {
                    let _ = process.thread.join();
                }
                ProcessState::Blocked => {
                    // Move it to the back of the queue
                    still_running.push(process);
                }
                ProcessState::Running => {
                    // TODO make round robin scheduler with fixed time slices.
                    still_running.push(process);
                }
            }
        }

        processes = still_running;

    }

    Ok(())
}
