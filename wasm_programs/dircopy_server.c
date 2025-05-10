#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>

// WASI socket functions
__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_open")))
int sock_open(int domain, int socktype, int protocol, int* sock_fd_out);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_listen")))
int sock_listen(int sock_fd, int backlog);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_accept")))
int sock_accept(int sock_fd, int flags, int* sock_fd_out);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_recv")))
int sock_recv(int sock_fd, void* ri_data, int ri_data_len, int ri_flags, int* ro_datalen, int* ro_flags);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_send")))
int sock_send(int sock_fd, const void* si_data, int si_data_len, int si_flags, int* ret_data_len);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_shutdown")))
int sock_shutdown(int sock_fd, int how);

// WASI file/directory syscalls
__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("path_open")))
int path_open(int dirfd, int dirflags, const char* path, int path_len, int oflags, long fs_rights_base, long fs_rights_inheriting, int fdflags, int* fd_out);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("fd_readdir")))
int fd_readdir(int fd, void* buf, int buf_len, long cookie, int bufused_out);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("fd_read")))
int fd_read(int fd, void* iovs, int iovs_len, int* nread);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("fd_write")))
int fd_write(int fd, const void* iovs, int iovs_len, int* nwritten);

__attribute__((import_module("env")))
__attribute__((import_name("file_create")))
int file_create(const char* path, int path_len, int* fd_out);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("path_create_directory")))
int path_create_directory(int dirfd, const char* path, int path_len);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("fd_close")))
int fd_close(int fd);

#define BUF_SIZE 4096
#define MAX_PATH 256

int copy_file(const char* src, const char* dst);
int copy_dir(const char* src, const char* dst);
void handle_client(int client_fd);

int main() {
    int server_fd, client_fd, ret;
    ret = sock_open(2, 1, 0, &server_fd); // AF_INET=2, SOCK_STREAM=1
    if (ret != 0) {
        printf("Failed to open socket\n");
        return 1;
    }
    ret = sock_listen(server_fd, 5);
    if (ret != 0) {
        printf("Failed to listen on socket\n");
        return 1;
    }
    printf("DirCopy server listening on port 7000\n");
    fflush(stdout);
    while (1) {
        ret = sock_accept(server_fd, 0, &client_fd);
        if (ret == 0) {
            handle_client(client_fd);
            sock_shutdown(client_fd, 3);
        }
    }
    return 0;
}

void handle_client(int client_fd) {
    char cmd_buf[2*MAX_PATH+16];
    int received = 0;
    // Receive command line
    while (received < sizeof(cmd_buf) - 1) {
        int n = 0;
        int ret = sock_recv(client_fd, cmd_buf + received, 1, 0, &n, NULL);
        if (ret != 0 || n <= 0) break;
        if (cmd_buf[received] == '\n') {
            cmd_buf[received] = 0;
            break;
        }
        received++;
    }
    if (strncmp(cmd_buf, "COPY ", 5) == 0) {
        char src[MAX_PATH], dst[MAX_PATH];
        if (sscanf(cmd_buf+5, "%255s %255s", src, dst) != 2) {
            char err[] = "ERR Invalid arguments\n";
            int sent;
            sock_send(client_fd, err, strlen(err), 0, &sent);
            return;
        }
        int res = copy_dir(src, dst);
        if (res == 0) {
            char ok[] = "OK\n";
            int sent;
            sock_send(client_fd, ok, strlen(ok), 0, &sent);
        } else {
            char err[] = "ERR Copy failed\n";
            int sent;
            sock_send(client_fd, err, strlen(err), 0, &sent);
        }
    }
}

// Recursively copy a directory
int copy_dir(const char* src, const char* dst) {
    // Create destination directory
    if (path_create_directory(3, dst, strlen(dst)) != 0) {
        // Might already exist, that's ok
    }
    // Open source directory
    int src_fd;
    if (path_open(3, 0, src, strlen(src), 0, 0x1, 0, 0, &src_fd) != 0) return -1;
    char buf[BUF_SIZE];
    int bufused = 0;
    struct { void* buf; int len; } iov = { buf, BUF_SIZE };
    // Read directory entries (simple: one per line)
    while (1) {
        int ret = fd_read(src_fd, &iov, 1, &bufused);
        if (ret != 0 || bufused <= 0) break;
        int start = 0;
        for (int i = 0; i < bufused; ++i) {
            if (buf[i] == '\n') {
                buf[i] = 0;
                char* entry = buf + start;
                if (strcmp(entry, ".") != 0 && strcmp(entry, "..") != 0) {
                    char src_path[MAX_PATH], dst_path[MAX_PATH];
                    snprintf(src_path, MAX_PATH, "%s/%s", src, entry);
                    snprintf(dst_path, MAX_PATH, "%s/%s", dst, entry);
                    // Check if entry is a directory or file
                    int entry_fd;
                    if (path_open(3, 0, src_path, strlen(src_path), 0, 0x1, 0, 0, &entry_fd) == 0) {
                        // Try reading as directory
                        int test_bufused = 0;
                        struct { void* buf; int len; } test_iov = { buf, BUF_SIZE };
                        int is_dir = (fd_read(entry_fd, &test_iov, 1, &test_bufused) == 0 && test_bufused > 0);
                        fd_close(entry_fd);
                        if (is_dir) {
                            copy_dir(src_path, dst_path);
                        } else {
                            copy_file(src_path, dst_path);
                        }
                    }
                }
                start = i+1;
            }
        }
    }
    fd_close(src_fd);
    return 0;
}

// Copy a single file
int copy_file(const char* src, const char* dst) {
    int src_fd;
    if (path_open(3, 0, src, strlen(src), 0, 0x1, 0, 0, &src_fd) != 0) return -1;
    int dst_fd;
    if (file_create(dst, strlen(dst), &dst_fd) != 0) { fd_close(src_fd); return -1; }
    char buf[BUF_SIZE];
    int nread = 0;
    struct { void* buf; int len; } iov = { buf, BUF_SIZE };
    while (fd_read(src_fd, &iov, 1, &nread) == 0 && nread > 0) {
        struct { void* buf; int len; } wiov = { buf, nread };
        int nwritten = 0;
        if (fd_write(dst_fd, &wiov, 1, &nwritten) != 0 || nwritten != nread) {
            fd_close(src_fd); fd_close(dst_fd); return -1;
        }
    }
    fd_close(src_fd);
    fd_close(dst_fd);
    return 0;
} 