#include <stdio.h>
#include <string.h>
#include <netinet/in.h>

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

int main() {
    int server_fd;
    int client_fd;
    int ret;
    int bytes_received;
    int bytes_sent;
    char buffer[1024];
    
    // Open a socket
    ret = sock_open(2, 1, 0, &server_fd); // AF_INET=2, SOCK_STREAM=1
    if (ret != 0) {
        printf("Failed to open socket\n");
        return 1;
    }
    printf("Server socket opened with fd: %d\n", server_fd);

    // Listen on port 7000
    ret = sock_listen(server_fd, 5); // backlog of 5
    if (ret != 0) {
        printf("Failed to listen on socket\n");
        return 1;
    }
    printf("Server listening on port 7000\n");

    // Accept a connection with retry loop
    while (1) {
        ret = sock_accept(server_fd, 0, &client_fd);
        if (ret == 0) {
            // Successfully accepted a connection
            break;
        } else if (ret == 11) { // EAGAIN
            // No connection available yet, retry
            continue;
        } else {
            // Some other error occurred
            printf("Failed to accept connection (error: %d)\n", ret);
            return 1;
        }
    }
    printf("Accepted connection with client fd: %d\n", client_fd);

    // // Receive data from client
    // ret = sock_recv(client_fd, buffer, sizeof(buffer), 0, &bytes_received, NULL);
    // if (ret != 0) {
    //     printf("Failed to receive data\n");
    //     return 1;
    // }
    // printf("Received %d bytes: %.*s\n", bytes_received, bytes_received, buffer);

    // Echo back to client
    char* message = "Hello, client!";
    ret = sock_send(client_fd, message, strlen(message), 0, &bytes_sent);
    if (ret != 0) {
        printf("Failed to send data\n");
        return 1;
    }
    printf("Sent %d bytes back to client\n", bytes_sent);

    // Shutdown the connection
    ret = sock_shutdown(client_fd, 3); // SHUT_RDWR = 3
    if (ret != 0) {
        printf("Failed to shutdown client socket\n");
        return 1;
    }
    printf("Client socket shutdown successfully\n");

    printf("Server socket shutdown successfully\n");

    return 0;
} 