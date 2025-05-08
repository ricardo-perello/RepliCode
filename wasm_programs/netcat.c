#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

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

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_listen")))
int sock_listen(int sock_fd, int backlog);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_accept")))
int sock_accept(int sock_fd, int flags, int* conn_fd_out);

#define BUF_SIZE 4096

void usage() {
    fprintf(stderr, "Usage: netcat [-l] <port> or netcat <host> <port>\n");
    fprintf(stderr, "  -l    Listen mode (server)\n");
    exit(1);
}

int main(int argc, char *argv[]) {
    // Print all arguments for debugging
    printf("Netcat received %d arguments:\n", argc);
    for (int i = 0; i < argc; i++) {
        printf("  argv[%d] = '%s'\n", i, argv[i]);
    }
    
    // Parse arguments
    int is_server = 0;
    int port = 0;
    const char *host = NULL;
    
    // Check for -l flag as first arg
    if (argc >= 1 && strcmp(argv[0], "-l") == 0) {
        is_server = 1;
        if (argc < 2) {
            usage();
        }
        port = atoi(argv[1]);
    } 
    // Check for -l flag as second arg (if program name is in argv[0])
    else if (argc >= 2 && strcmp(argv[1], "-l") == 0) {
        is_server = 1;
        if (argc < 3) {
            usage();
        }
        port = atoi(argv[2]);
    }
    // Client mode
    else if (argc == 2) {
        host = argv[0];
        port = atoi(argv[1]);
    }
    else if (argc == 3) {
        host = argv[1];
        port = atoi(argv[2]);
    }
    else {
        usage();
    }
    
    if (port <= 0) {
        printf("Invalid port: %d\n", port);
        usage();
    }

    int sockfd, clientfd;
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

    if (is_server) {
        // Server mode
        printf("Starting server on port %d\n", port);
        
        // Listen on the port (binding happens automatically)
        ret = sock_listen(sockfd, 5);
        if (ret != 0) {
            printf("Failed to listen on port %d\n", port);
            return 1;
        }
        
        printf("Listening on port %d, waiting for connection...\n", port);
        fflush(stdout);
        
        // Accept one connection with retry loop
        while (1) {
            ret = sock_accept(sockfd, 0, &clientfd);
            if (ret == 0) {
                // Successfully accepted a connection
                break;
            } else if (ret == 11) { // EAGAIN
                // No connection available yet, retry
                printf("Waiting for connection...\n");
                continue;
            } else {
                // Some other error occurred
                printf("Failed to accept connection (error: %d)\n", ret);
                return 1;
            }
        }
        
        printf("Client connected! Ready to receive data.\n");
        fflush(stdout);
        
        // Relay data from client to stdout
        while (!done) {
            // Read from socket
            ret = sock_recv(clientfd, buf, BUF_SIZE, 0, &received, NULL);
            if (ret == 0 && received > 0) {
                fwrite(buf, 1, received, stdout);
                fflush(stdout);
            } else if (received == 0) {
                printf("Client disconnected\n");
                done = 1;
            }

            // Read from stdin and send to client
            n = fread(buf, 1, BUF_SIZE, stdin);
            if (n > 0) {
                ret = sock_send(clientfd, buf, n, 0, &sent);
                if (ret != 0 || sent != n) {
                    printf("Failed to send data\n");
                    break;
                }
            } else if (feof(stdin)) {
                sock_shutdown(clientfd, 1); // SHUT_WR
                done = 1;
            }
        }
        
        sock_shutdown(clientfd, 3); // SHUT_RDWR
    } else {
        // Client mode
        printf("Connecting to %s:%d\n", host, port);
        
        // Connect to the server
        ret = sock_connect(sockfd, host, port);
        if (ret != 0) {
            printf("Failed to connect to %s:%d\n", host, port);
            return 2;
        }

        printf("Connected. Type data to send...\n");
        
        // Relay stdin to socket and socket to stdout
        while (!done) {
            // Read from stdin
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
                printf("Server disconnected\n");
                done = 1;
            }
        }
        
        sock_shutdown(sockfd, 3); // SHUT_RDWR
    }

    return 0;
} 