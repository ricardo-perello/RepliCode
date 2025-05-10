#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

// WASI socket functions
__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_open")))
int sock_open(int domain, int socktype, int protocol, int* sock_fd_out);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_connect")))
int sock_connect(int sock_fd, const char* addr, int port);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_send")))
int sock_send(int sock_fd, const void* si_data, int si_data_len, int si_flags, int* ret_data_len);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_recv")))
int sock_recv(int sock_fd, void* ri_data, int ri_data_len, int ri_flags, int* ro_datalen, int* ro_flags);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_shutdown")))
int sock_shutdown(int sock_fd, int how);

// WASI file functions
__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("path_open")))
int path_open(int dirfd, int dirflags, const char* path, int path_len, int oflags, long fs_rights_base, long fs_rights_inheriting, int fdflags, int* fd_out);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("fd_read")))
int fd_read(int fd, void* iovs, int iovs_len, int* nread);

__attribute__((import_module("env")))
__attribute__((import_name("file_create")))
int file_create(const char* path, int path_len, int* fd_out);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("fd_write")))
int fd_write(int fd, const void* iovs, int iovs_len, int* nwritten);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("fd_close")))
int fd_close(int fd);

#define BUF_SIZE 4096
#define MAX_FILENAME 256

void send_image(int sockfd, const char* filename);
void get_image(int sockfd, const char* filename);

void usage(const char* prog) {
    printf("Usage: %s <host> <port> <SEND|GET> <filename> [localfile]\n", prog);
    exit(1);
}

int main(int argc, char* argv[]) {
    if (argc < 5) usage(argv[0]);
    const char* host = argv[1];
    int port = atoi(argv[2]);
    const char* cmd = argv[3];
    const char* filename = argv[4];
    int sockfd, ret;
    ret = sock_open(2, 1, 0, &sockfd);
    if (ret != 0) {
        printf("Failed to open socket\n");
        return 1;
    }
    ret = sock_connect(sockfd, host, port);
    if (ret != 0) {
        printf("Failed to connect to %s:%d\n", host, port);
        return 2;
    }
    if (strcmp(cmd, "SEND") == 0) {
        if (argc < 6) usage(argv[0]);
        send_image(sockfd, filename);
    } else if (strcmp(cmd, "GET") == 0) {
        get_image(sockfd, filename);
    } else {
        usage(argv[0]);
    }
    sock_shutdown(sockfd, 3);
    return 0;
}

void send_image(int sockfd, const char* filename) {
    // Open local file for reading
    int fd;
    if (path_open(3, 0, filename, strlen(filename), 0, 0x1, 0, 0, &fd) != 0) {
        printf("Failed to open local file %s\n", filename);
        return;
    }
    // Stat file size
    uint8_t buf[BUF_SIZE];
    uint32_t total = 0;
    int nread = 0;
    struct { void* buf; int len; } iov = { buf, BUF_SIZE };
    while (fd_read(fd, &iov, 1, &nread) == 0 && nread > 0) {
        total += nread;
    }
    // Send command
    char cmdline[16 + MAX_FILENAME];
    snprintf(cmdline, sizeof(cmdline), "SEND %s\n", filename);
    int sent;
    sock_send(sockfd, cmdline, strlen(cmdline), 0, &sent);
    // Send size
    uint8_t size_buf[4] = {
        (total>>24)&0xFF, (total>>16)&0xFF, (total>>8)&0xFF, total&0xFF
    };
    sock_send(sockfd, size_buf, 4, 0, &sent);
    // Rewind and send file
    fd_close(fd);
    if (path_open(3, 0, filename, strlen(filename), 0, 0x1, 0, 0, &fd) != 0) return;
    while (fd_read(fd, &iov, 1, &nread) == 0 && nread > 0) {
        sock_send(sockfd, buf, nread, 0, &sent);
    }
    fd_close(fd);
    // Wait for OK
    char ok[8];
    int got = 0;
    while (got < 3) {
        int n = 0;
        int ret = sock_recv(sockfd, ok + got, 3 - got, 0, &n, NULL);
        if (ret != 0 || n <= 0) break;
        got += n;
    }
    ok[got] = 0;
    printf("Server response: %s\n", ok);
}

void get_image(int sockfd, const char* filename) {
    // Send command
    char cmdline[16 + MAX_FILENAME];
    snprintf(cmdline, sizeof(cmdline), "GET %s\n", filename);
    int sent;
    sock_send(sockfd, cmdline, strlen(cmdline), 0, &sent);
    // Receive 4 bytes (size)
    uint8_t size_buf[4];
    int got = 0, n = 0;
    while (got < 4) {
        int ret = sock_recv(sockfd, size_buf + got, 4 - got, 0, &n, NULL);
        if (ret != 0 || n <= 0) return;
        got += n;
    }
    uint32_t img_size = (size_buf[0]<<24) | (size_buf[1]<<16) | (size_buf[2]<<8) | size_buf[3];
    if (img_size == 0) {
        printf("Image not found on server\n");
        return;
    }
    // Create file to save
    int img_fd;
    if (file_create(filename, strlen(filename), &img_fd) != 0) {
        printf("Failed to create output file\n");
        return;
    }
    // Receive and write image data
    uint8_t buf[BUF_SIZE];
    uint32_t left = img_size;
    while (left > 0) {
        int to_read = left > BUF_SIZE ? BUF_SIZE : left;
        int n = 0, got = 0;
        while (got < to_read) {
            int ret = sock_recv(sockfd, buf + got, to_read - got, 0, &n, NULL);
            if (ret != 0 || n <= 0) return;
            got += n;
        }
        struct { void* buf; int len; } iov = { buf, got };
        int nwritten = 0;
        if (fd_write(img_fd, &iov, 1, &nwritten) != 0 || nwritten != got) return;
        left -= got;
    }
    fd_close(img_fd);
    printf("Image %s received and saved.\n", filename);
} 