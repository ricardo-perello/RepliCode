#include <stdio.h>
#include <string.h>

// WASI socket functions
__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_open")))
int sock_open(int domain, int socktype, int protocol, int* sock_fd_out);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_send")))
int sock_send(int sock_fd, const void* si_data, int si_data_len, int si_flags, int* ret_data_len);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_shutdown")))
int sock_shutdown(int sock_fd, int how);

int main() {
    int sock_fd;
    int ret;
    int bytes_sent;
    
    // Open a socket
    ret = sock_open(2, 1, 0, &sock_fd); // AF_INET=2, SOCK_STREAM=1
    if (ret != 0) {
        printf("Failed to open socket\n");
        return 1;
    }
    printf("Socket opened with fd: %d\n", sock_fd);

    // Send a test message using iovec
    const char* message = "Hello from WASM!";
    struct iovec iov = {
        .iov_base = (void*)message,
        .iov_len = strlen(message)
    };
    
    ret = sock_send(sock_fd, &iov, 1, 0, &bytes_sent);
    if (ret != 0) {
        printf("Failed to send message\n");
        return 1;
    }
    printf("Message sent successfully, %d bytes sent\n", bytes_sent);

    // Shutdown the socket (SHUT_RDWR = 3)
    ret = sock_shutdown(sock_fd, 3);
    if (ret != 0) {
        printf("Failed to shutdown socket\n");
        return 1;
    }
    printf("Socket shutdown successfully\n");

    return 0;
} 