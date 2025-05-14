#include <stdio.h>
#include <string.h>
#include <netinet/in.h>
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
#define MAX_CMD_SIZE 1024

// Helper function to trim whitespace and newlines from the end of a string
void trim_end(char *str) {
    int len = strlen(str);
    while (len > 0 && (str[len-1] == ' ' || str[len-1] == '\n' || str[len-1] == '\r')) {
        str[len-1] = '\0';
        len--;
    }
}

// Helper function to convert network byte order to host byte order
uint32_t ntohl(uint32_t netlong) {
    return ((netlong & 0xFF) << 24) |
           ((netlong & 0xFF00) << 8) |
           ((netlong & 0xFF0000) >> 8) |
           ((netlong & 0xFF000000) >> 24);
}

void handle_client(int client_fd) {
    char cmd_buf[MAX_CMD_SIZE];
    int bytes_received;
    int bytes_sent;
    int ret;
    
    // Receive command and filename
    int total_received = 0;
    while (total_received < MAX_CMD_SIZE - 1) {
        ret = sock_recv(client_fd, cmd_buf + total_received, 1, 0, &bytes_received, NULL);
        if (ret != 0 || bytes_received == 0) {
            printf("Failed to receive command or client disconnected\n");
            fflush(stdout);
            return;
        }
        total_received += bytes_received;
        // Stop at newline or if we've received enough data
        if (cmd_buf[total_received - 1] == '\n' || total_received >= MAX_CMD_SIZE - 1) {
            break;
        }
    }
    
    // Null terminate the command
    cmd_buf[total_received] = '\0';
    printf("Received command: %s\n", cmd_buf);
    fflush(stdout);

    // Parse command
    char* cmd = strtok(cmd_buf, " ");
    if (cmd == NULL) {
        printf("Invalid command format\n");
        fflush(stdout);
        return;
    }
    
    if (strcmp(cmd, "SEND") == 0) {
        // Handle SEND command
        char* filename = strtok(NULL, " ");
        if (filename == NULL) {
            printf("Missing filename for SEND command\n");
            fflush(stdout);
            return;
        }
        // Trim any whitespace or newlines from the filename
        trim_end(filename);
        
        // Receive file size
        uint32_t file_size;
        ret = sock_recv(client_fd, &file_size, sizeof(file_size), 0, &bytes_received, NULL);
        if (ret != 0 || bytes_received != sizeof(file_size)) {
            printf("Failed to receive file size\n");
            fflush(stdout);
            return;
        }
        // Convert from network byte order to host byte order
        file_size = ntohl(file_size);
        printf("[SERVER] Expecting to receive %u bytes for file %s\n", file_size, filename);
        
        // Create file
        int fd = open(filename, O_WRONLY | O_CREAT | O_TRUNC, 0666);
        if (fd < 0) {
            printf("Failed to create file %s\n", filename);
            fflush(stdout);
            return;
        }
        printf("[SERVER] Opened file %s for writing\n", filename);
        fflush(stdout);
        
        // Receive and write file data
        char buffer[BUF_SIZE];
        uint32_t remaining = file_size;
        while (remaining > 0) {
            int to_read = remaining > BUF_SIZE ? BUF_SIZE : remaining;
            ret = sock_recv(client_fd, buffer, to_read, 0, &bytes_received, NULL);
            if (ret != 0 || bytes_received <= 0) {
                printf("[SERVER] Error or disconnect while receiving file data\n");
                fflush(stdout);
                close(fd);
                return;
            }
            write(fd, buffer, bytes_received);
            remaining -= bytes_received;
            printf("[SERVER] Received %d bytes, %u bytes remaining\n", bytes_received, remaining);
            fflush(stdout);
        }
        close(fd);
        printf("[SERVER] Finished writing file %s\n", filename);
        fflush(stdout);
        
        // Send success response
        const char* response = "OK\n";  // Just send OK
        char response_buf[4];  // Buffer for response
        strcpy(response_buf, response);  // Copy response to separate buffer
        ret = sock_send(client_fd, response_buf, strlen(response_buf), 0, &bytes_sent);
        if (ret != 0 || bytes_sent != strlen(response_buf)) {
            printf("Failed to send response\n");
            fflush(stdout);
            return;
        }
        printf("[SERVER] Sent response: %s", response);
        fflush(stdout);
        
        // Shutdown the write side of the socket to signal EOF
        ret = sock_shutdown(client_fd, 1);  // SHUT_WR = 1
        if (ret != 0) {
            printf("Failed to shutdown socket\n");
            fflush(stdout);
            return;
        }
        
    } else if (strcmp(cmd, "GET") == 0) {
        // Handle GET command
        char* filename = strtok(NULL, " ");
        if (filename == NULL) {
            printf("Missing filename for GET command\n");
            fflush(stdout);
            return;
        }
        
        // Open file
        int fd = open(filename, O_RDONLY);
        if (fd < 0) {
            printf("File not found: %s\n", filename);
            fflush(stdout);
            const char* response = "ERROR: File not found\n";
            sock_send(client_fd, response, strlen(response), 0, &bytes_sent);
            return;
        }
        
        // Get file size
        struct stat st;
        fstat(fd, &st);
        uint32_t file_size = st.st_size;
        
        // Send file size
        sock_send(client_fd, &file_size, sizeof(file_size), 0, &bytes_sent);
        
        // Send file data
        char buffer[BUF_SIZE];
        while (1) {
            int nread = read(fd, buffer, BUF_SIZE);
            if (nread <= 0) break;
            sock_send(client_fd, buffer, nread, 0, &bytes_sent);
        }
        close(fd);
        
    } else {
        printf("Unknown command: %s\n", cmd);
        fflush(stdout);
        const char* response = "ERROR: Unknown command\n";
        sock_send(client_fd, response, strlen(response), 0, &bytes_sent);
    }
}

int main() {
    int server_fd;
    int client_fd;
    int ret;
    
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
    fflush(stdout);

    // Main server loop
    while (1) {
        // Accept a connection
        ret = sock_accept(server_fd, 0, &client_fd);
        if (ret != 0) {
            printf("Failed to accept connection (error: %d)\n", ret);
            continue;
        }
        printf("Accepted connection with client fd: %d\n", client_fd);
        fflush(stdout);
        // Handle client
        handle_client(client_fd);

        // Shutdown the connection
        sock_shutdown(client_fd, 3); // SHUT_RDWR = 3
        printf("Client connection closed\n");
    }

    return 0;
} 