#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/stat.h>

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
        int img_fd = open(filename, O_WRONLY | O_CREAT | O_TRUNC, 0666);
        if (img_fd < 0) return;
        // Receive and write image data
        uint8_t buf[BUF_SIZE];
        uint32_t left = img_size;
        while (left > 0) {
            int to_read = left > BUF_SIZE ? BUF_SIZE : left;
            int n = 0, got = 0;
            while (got < to_read) {
                int ret = sock_recv(client_fd, buf + got, to_read - got, 0, &n, NULL);
                if (ret != 0 || n <= 0) { close(img_fd); return; }
                got += n;
            }
            // Write to file
            int nwritten = 0, written_total = 0;
            while (written_total < got) {
                nwritten = write(img_fd, buf + written_total, got - written_total);
                if (nwritten < 0) { close(img_fd); return; }
                written_total += nwritten;
            }
            left -= got;
        }
        close(img_fd);
        // Optionally send OK
        char ok[] = "OK\n";
        int sent;
        sock_send(client_fd, ok, strlen(ok), 0, &sent);
    } else if (strncmp(cmd_buf, "GET ", 4) == 0) {
        char* filename = cmd_buf + 4;
        // Open file
        int img_fd = open(filename, O_RDONLY);
        if (img_fd < 0) {
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
        while ((nread = read(img_fd, buf, BUF_SIZE)) > 0) {
            total += nread;
        }
        lseek(img_fd, 0, SEEK_SET); // rewind
        // Send size
        uint8_t size_buf[4] = {
            (total>>24)&0xFF, (total>>16)&0xFF, (total>>8)&0xFF, total&0xFF
        };
        int sent;
        sock_send(client_fd, size_buf, 4, 0, &sent);
        // Rewind and send file
        lseek(img_fd, 0, SEEK_SET);
        while ((nread = read(img_fd, buf, BUF_SIZE)) > 0) {
            sock_send(client_fd, buf, nread, 0, &sent);
        }
        close(img_fd);
    }
} 