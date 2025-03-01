use anyhow::Result;
use wasmtime::Linker;
use crate::runtime::process::ProcessData;
pub mod fd;
pub mod builtin_yield;
pub fn register(linker: &mut Linker<ProcessData>) -> Result<()> {
    linker.func_wrap("wasi_snapshot_preview1", "fd_close", fd::wasi_fd_close)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_fdstat_get", fd::wasi_fd_fdstat_get)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_seek", fd::wasi_fd_seek)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_write", fd::wasi_fd_write)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_read", fd::wasi_fd_read)?;
    linker.func_wrap("wasi_snapshot_preview1", "poll_oneoff", fd::wasi_poll_oneoff)?;
    linker.func_wrap("wasi_snapshot_preview1", "proc_exit", fd::wasi_proc_exit)?;
    linker.func_wrap("env","_builtin_rt_yield",builtin_yield::wasi_builtin_rt_yield)?;
    Ok(())
}