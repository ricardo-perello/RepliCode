#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

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

// WASI file functions
__attribute__((import_module("env")))
__attribute__((import_name("file_create")))
int file_create(const char* path, int path_len, int* fd_out);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("fd_write")))
int fd_write(int fd, const void* iovs, int iovs_len, int* nwritten);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("path_open")))
int path_open(int dirfd, int dirflags, const char* path, int path_len, int oflags, long fs_rights_base, long fs_rights_inheriting, int fdflags, int* fd_out);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("fd_read")))
int fd_read(int fd, void* iovs, int iovs_len, int* nread);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("fd_close")))
int fd_close(int fd);

#define BUF_SIZE 4096
#define MAX_FILENAME 256

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
    printf("Image server listening on port 7000\n");
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
    char cmd_buf[16 + MAX_FILENAME];
    int received = 0;
    // Receive command line
    received = 0;
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
    if (strncmp(cmd_buf, "SEND ", 5) == 0) {
        char* filename = cmd_buf + 5;
        // Receive 4 bytes (image size)
        uint8_t size_buf[4];
        int n = 0, got = 0;
        while (got < 4) {
            int ret = sock_recv(client_fd, size_buf + got, 4 - got, 0, &n, NULL);
            if (ret != 0 || n <= 0) return;
            got += n;
        }
        uint32_t img_size = (size_buf[0]<<24) | (size_buf[1]<<16) | (size_buf[2]<<8) | size_buf[3];
        if (img_size > 10*1024*1024) return; // 10MB limit
        // Create file
        int img_fd;
        if (file_create(filename, strlen(filename), &img_fd) != 0) return;
        // Receive and write image data
        uint8_t buf[BUF_SIZE];
        uint32_t left = img_size;
        while (left > 0) {
            int to_read = left > BUF_SIZE ? BUF_SIZE : left;
            int n = 0, got = 0;
            while (got < to_read) {
                int ret = sock_recv(client_fd, buf + got, to_read - got, 0, &n, NULL);
                if (ret != 0 || n <= 0) return;
                got += n;
            }
            // Write to file
            struct { void* buf; int len; } iov = { buf, got };
            int nwritten = 0;
            if (fd_write(img_fd, &iov, 1, &nwritten) != 0 || nwritten != got) return;
            left -= got;
        }
        fd_close(img_fd);
        // Optionally send OK
        char ok[] = "OK\n";
        int sent;
        sock_send(client_fd, ok, strlen(ok), 0, &sent);
    } else if (strncmp(cmd_buf, "GET ", 4) == 0) {
        char* filename = cmd_buf + 4;
        // Open file
        int img_fd;
        int oflags = 0; // read only
        if (path_open(3, 0, filename, strlen(filename), oflags, 0x1, 0, 0, &img_fd) != 0) {
            // Not found
            uint8_t size_buf[4] = {0,0,0,0};
            int sent;
            sock_send(client_fd, size_buf, 4, 0, &sent);
            return;
        }
        // Stat file size (read whole file)
        uint8_t buf[BUF_SIZE];
        uint32_t total = 0;
        int nread = 0;
        struct { void* buf; int len; } iov = { buf, BUF_SIZE };
        while (fd_read(img_fd, &iov, 1, &nread) == 0 && nread > 0) {
            total += nread;
        }
        // Send size
        uint8_t size_buf[4] = {
            (total>>24)&0xFF, (total>>16)&0xFF, (total>>8)&0xFF, total&0xFF
        };
        int sent;
        sock_send(client_fd, size_buf, 4, 0, &sent);
        // Rewind and send file
        fd_close(img_fd);
        if (path_open(3, 0, filename, strlen(filename), oflags, 0x1, 0, 0, &img_fd) != 0) return;
        while (fd_read(img_fd, &iov, 1, &nread) == 0 && nread > 0) {
            sock_send(client_fd, buf, nread, 0, &sent);
        }
        fd_close(img_fd);
    }
} 