#include <stdio.h>
#include <stdlib.h>
#include <string.h>

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

#define BUF_SIZE 4096

void usage(const char *prog) {
    fprintf(stderr, "Usage: %s <host> <port>\n", prog);
    exit(1);
}

int main(int argc, char *argv[]) {
    if (argc != 3) usage(argv[0]);

    const char *host = argv[1];
    int port = atoi(argv[2]);
    int sockfd;
    int ret;
    char buf[BUF_SIZE];
    int n, sent, received;
    int done = 0;

    // Open a socket (AF_INET=2, SOCK_STREAM=1)
    ret = sock_open(2, 1, 0, &sockfd);
    if (ret != 0) {
        printf("Failed to open socket\n");
        return 1;
    }

    // Connect to the server
    ret = sock_connect(sockfd, host, port);
    if (ret != 0) {
        printf("Failed to connect to %s:%d\n", host, port);
        return 2;
    }

    // Relay stdin to socket and socket to stdout
    while (!done) {
        // Read from stdin (non-blocking would be better, but keep it simple)
        n = fread(buf, 1, BUF_SIZE, stdin);
        if (n > 0) {
            ret = sock_send(sockfd, buf, n, 0, &sent);
            if (ret != 0 || sent != n) {
                printf("Failed to send data\n");
                break;
            }
        } else if (feof(stdin)) {
            sock_shutdown(sockfd, 1); // SHUT_WR
            done = 1;
        }

        // Read from socket
        ret = sock_recv(sockfd, buf, BUF_SIZE, 0, &received, NULL);
        if (ret == 0 && received > 0) {
            fwrite(buf, 1, received, stdout);
            fflush(stdout);
        } else if (received == 0) {
            done = 1;
        }
    }

    sock_shutdown(sockfd, 3); // SHUT_RDWR
    return 0;
} 