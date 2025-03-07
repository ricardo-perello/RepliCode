use anyhow::Result;
use wasmtime::Linker;
use crate::runtime::process::ProcessData;
pub mod fd;
pub mod fs;
pub mod net;
pub mod builtin_yield;
pub fn register(linker: &mut Linker<ProcessData>) -> Result<()> {
    linker.func_wrap("wasi_snapshot_preview1", "fd_fdstat_get", fd::wasi_fd_fdstat_get)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_seek", fd::wasi_fd_seek)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_write", fd::wasi_fd_write)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_read", fd::wasi_fd_read)?;
    linker.func_wrap("wasi_snapshot_preview1", "poll_oneoff", fd::wasi_poll_oneoff)?;
    linker.func_wrap("wasi_snapshot_preview1", "proc_exit", fd::wasi_proc_exit)?;

    linker.func_wrap("env","__builtin_rt_yield",builtin_yield::wasi__builtin_rt_yield)?;

    linker.func_wrap("wasi_snapshot_preview1", "path_open", fs::wasi_path_open)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_readdir", fs::wasi_fd_readdir)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_close", fs::wasi_fd_close)?;


    // If you want them under 'env' or some other module, that is up to you:
    linker.func_wrap("wasi_snapshot_preview1", "sock_open", net::wasi_sock_open)?;
    linker.func_wrap("wasi_snapshot_preview1", "sock_listen", net::wasi_sock_listen)?;
    Ok(())
}