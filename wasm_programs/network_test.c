#include <stdio.h>
#include <string.h>
#include <netinet/in.h>

// WASI socket functions
__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_open")))
int sock_open(int domain, int socktype, int protocol, int* sock_fd_out);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_connect")))
int sock_connect(int sock_fd, const struct sockaddr* addr, int addr_len);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_send")))
int sock_send(int sock_fd, const void* si_data, int si_data_len, int si_flags, int* ret_data_len);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_recv")))
int sock_recv(int sock_fd, void* ri_data, int ri_data_len, int ri_flags, int* ro_datalen, int* ro_flags);

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

    // Set up destination address (example: localhost:8080)
    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port = htons(8000);
    addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK); // 127.0.0.1

    // Connect to the destination
    ret = sock_connect(sock_fd, (struct sockaddr*)&addr, sizeof(addr));
    if (ret != 0) {
        printf("Failed to connect socket\n");
        return 1;
    }
    printf("Socket connected successfully\n");

    // Send a test message
    const char* message = "Hello from WASM!";
    ret = sock_send(sock_fd, message, strlen(message), 0, &bytes_sent);
    if (ret != 0) {
        printf("Failed to send message\n");
        return 1;
    }
    printf("Message sent successfully, %d bytes sent\n", bytes_sent);  

    // Read response
    char buffer[1024];
    int bytes_received;
    ret = sock_recv(sock_fd, buffer, sizeof(buffer), 0, &bytes_received, NULL);
    if (ret != 0) {
        printf("Failed to receive response\n");
        return 1;
    }
    printf("Received %d bytes: %.*s\n", bytes_received, bytes_received, buffer);

    // Shutdown the socket (SHUT_RDWR = 3)
    ret = sock_shutdown(sock_fd, 3);
    if (ret != 0) {
        printf("Failed to shutdown socket\n");
        return 1;
    }
    printf("Socket shutdown successfully\n");

    return 0;
} 