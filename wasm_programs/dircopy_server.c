#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/stat.h>
#include <dirent.h>

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
    if (mkdir(dst, 0777) != 0) {
        // Might already exist, that's ok
    }
    // Open source directory
    DIR* src_dir = opendir(src);
    if (!src_dir) return -1;
    struct dirent* entry;
    while ((entry = readdir(src_dir)) != NULL) {
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) continue;
        char src_path[MAX_PATH], dst_path[MAX_PATH];
        snprintf(src_path, MAX_PATH, "%s/%s", src, entry->d_name);
        snprintf(dst_path, MAX_PATH, "%s/%s", dst, entry->d_name);
        struct stat st;
        if (stat(src_path, &st) == 0) {
            if (S_ISDIR(st.st_mode)) {
                copy_dir(src_path, dst_path);
            } else if (S_ISREG(st.st_mode)) {
                copy_file(src_path, dst_path);
            }
        }
    }
    closedir(src_dir);
    return 0;
}

// Copy a single file
int copy_file(const char* src, const char* dst) {
    int src_fd = open(src, O_RDONLY);
    if (src_fd < 0) return -1;
    int dst_fd = open(dst, O_WRONLY | O_CREAT | O_TRUNC, 0666);
    if (dst_fd < 0) { close(src_fd); return -1; }
    char buf[BUF_SIZE];
    ssize_t nread;
    while ((nread = read(src_fd, buf, BUF_SIZE)) > 0) {
        ssize_t nwritten = 0, written_total = 0;
        while (written_total < nread) {
            nwritten = write(dst_fd, buf + written_total, nread - written_total);
            if (nwritten < 0) { close(src_fd); close(dst_fd); return -1; }
            written_total += nwritten;
        }
    }
    close(src_fd);
    close(dst_fd);
    return 0;
} 