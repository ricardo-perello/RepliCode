use anyhow::Result;
use wasmtime::Linker;

pub mod fd;

pub fn register(linker: &mut Linker<()>) -> Result<()> {
    linker.func_wrap("wasi_snapshot_preview1", "fd_close", fd::wasi_fd_close)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_fdstat_get", fd::wasi_fd_fdstat_get)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_seek", fd::wasi_fd_seek)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_write", fd::wasi_fd_write)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_read", fd::wasi_fd_read)?;
    linker.func_wrap("wasi_snapshot_preview1", "poll_oneoff", fd::wasi_poll_oneoff)?;
    linker.func_wrap("wasi_snapshot_preview1", "proc_exit", fd::wasi_proc_exit)?;
    Ok(())
}