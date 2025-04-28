use anyhow::Result;
use wasmtime::Linker;
use crate::runtime::process::ProcessData;

pub mod fd;
pub mod fs;
pub mod net;
pub mod builtin_yield;
pub mod args;
pub mod clock;
pub mod process;
pub mod fd_ops;
pub mod path_ops;

pub fn register(linker: &mut Linker<ProcessData>) -> Result<()> {
    // Arguments and Environment
    linker.func_wrap("wasi_snapshot_preview1", "args_get", args::wasi_args_get)?;
    linker.func_wrap("wasi_snapshot_preview1", "args_sizes_get", args::wasi_args_sizes_get)?;
    linker.func_wrap("wasi_snapshot_preview1", "environ_get", args::wasi_environ_get)?;
    linker.func_wrap("wasi_snapshot_preview1", "environ_sizes_get", args::wasi_environ_sizes_get)?;

    // Clock
    linker.func_wrap("wasi_snapshot_preview1", "clock_res_get", clock::wasi_clock_res_get)?;
    linker.func_wrap("wasi_snapshot_preview1", "clock_time_get", clock::wasi_clock_time_get)?;

    // Process and Random
    linker.func_wrap("wasi_snapshot_preview1", "proc_raise", process::wasi_proc_raise)?;
    linker.func_wrap("wasi_snapshot_preview1", "sched_yield", process::wasi_sched_yield)?;
    linker.func_wrap("wasi_snapshot_preview1", "random_get", process::wasi_random_get)?;

    // File Descriptor Operations
    linker.func_wrap("wasi_snapshot_preview1", "fd_advise", fd_ops::wasi_fd_advise)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_allocate", fd_ops::wasi_fd_allocate)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_datasync", fd_ops::wasi_fd_datasync)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_fdstat_set_flags", fd_ops::wasi_fd_fdstat_set_flags)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_fdstat_set_rights", fd_ops::wasi_fd_fdstat_set_rights)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_filestat_get", fd_ops::wasi_fd_filestat_get)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_filestat_set_size", fd_ops::wasi_fd_filestat_set_size)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_filestat_set_times", fd_ops::wasi_fd_filestat_set_times)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_pread", fd_ops::wasi_fd_pread)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_pwrite", fd_ops::wasi_fd_pwrite)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_renumber", fd_ops::wasi_fd_renumber)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_sync", fd_ops::wasi_fd_sync)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_tell", fd_ops::wasi_fd_tell)?;

    // Path Operations
    linker.func_wrap("wasi_snapshot_preview1", "path_filestat_get", path_ops::wasi_path_filestat_get)?;
    linker.func_wrap("wasi_snapshot_preview1", "path_filestat_set_times", path_ops::wasi_path_filestat_set_times)?;
    linker.func_wrap("wasi_snapshot_preview1", "path_link", path_ops::wasi_path_link)?;
    linker.func_wrap("wasi_snapshot_preview1", "path_readlink", path_ops::wasi_path_readlink)?;
    linker.func_wrap("wasi_snapshot_preview1", "path_rename", path_ops::wasi_path_rename)?;

    // Existing registrations
    linker.func_wrap("wasi_snapshot_preview1", "fd_fdstat_get", fd::wasi_fd_fdstat_get)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_seek", fd::wasi_fd_seek)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_read", fd::wasi_fd_read)?;
    linker.func_wrap("wasi_snapshot_preview1", "poll_oneoff", fd::wasi_poll_oneoff)?;
    linker.func_wrap("wasi_snapshot_preview1", "proc_exit", fd::wasi_proc_exit)?;

    linker.func_wrap("env","__builtin_rt_yield",builtin_yield::wasi__builtin_rt_yield)?;

    linker.func_wrap("wasi_snapshot_preview1", "path_open", fs::wasi_path_open)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_readdir", fs::wasi_fd_readdir)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_close", fs::wasi_fd_close)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_prestat_get", fd::wasi_fd_prestat_get)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_prestat_dir_name", fd::wasi_fd_prestat_dir_name)?;
    linker.func_wrap("wasi_snapshot_preview1", "path_create_directory", fs::wasi_path_create_directory)?;
    linker.func_wrap("wasi_snapshot_preview1", "path_remove_directory", fs::wasi_path_remove_directory)?;
    linker.func_wrap("wasi_snapshot_preview1", "path_unlink_file", fs::wasi_path_unlink_file)?;
    linker.func_wrap("wasi_snapshot_preview1", "path_symlink", fs::wasi_path_symlink)?;
    linker.func_wrap("wasi_snapshot_preview1", "fd_write", fs::wasi_fd_write)?;
    linker.func_wrap("env", "file_create", fs::wasi_file_create)?;

    // Socket Operations
    linker.func_wrap("wasi_snapshot_preview1", "sock_open", net::wasi_sock_open)?;
    linker.func_wrap("wasi_snapshot_preview1", "sock_connect", net::wasi_sock_connect)?;
    linker.func_wrap("wasi_snapshot_preview1", "sock_listen", net::wasi_sock_listen)?;
    linker.func_wrap("wasi_snapshot_preview1", "sock_accept", net::wasi_sock_accept)?;
    linker.func_wrap("wasi_snapshot_preview1", "sock_recv", net::wasi_sock_recv)?;
    linker.func_wrap("wasi_snapshot_preview1", "sock_send", net::wasi_sock_send)?;
    linker.func_wrap("wasi_snapshot_preview1", "sock_shutdown", net::wasi_sock_shutdown)?;

    Ok(())
}