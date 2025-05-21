#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <errno.h>
#include <time.h>
#include <arpa/inet.h>
#include <limits.h>  // For PATH_MAX

#define BUF_SIZE 4096
#define MAX_FILENAME 256
#define MAX_CMD_SIZE 1024
#define PORT 7001

// Helper function to trim whitespace and newlines from the end of a string
void trim_end(char *str) {
    int len = strlen(str);
    while (len > 0 && (str[len-1] == ' ' || str[len-1] == '\n' || str[len-1] == '\r' || str[len-1] == '\t')) {
        str[len-1] = '\0';
        len--;
    }
    // Also trim from the beginning
    while (str[0] == ' ' || str[0] == '\n' || str[0] == '\r' || str[0] == '\t') {
        memmove(str, str + 1, len);
        len--;
    }
    printf("[SERVER] After trimming: length=%d, bytes: ", len);
    for(int i = 0; i < len; i++) {
        printf("%02x ", (unsigned char)str[i]);
    }
    printf("\n");
    fflush(stdout);
}

// Helper function to get current timestamp in milliseconds
long get_timestamp_ms() {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (ts.tv_sec * 1000) + (ts.tv_nsec / 1000000);
}

void handle_client(int client_fd) {
    char cmd_buf[MAX_CMD_SIZE];
    int bytes_received;
    int bytes_sent;
    long start_time, end_time;
    char cwd[PATH_MAX];
    char abs_path[PATH_MAX];
    
    // Print current working directory
    if (getcwd(cwd, sizeof(cwd)) != NULL) {
        printf("[SERVER] Current working directory: %s\n", cwd);
        fflush(stdout);
    }
    
    printf("[SERVER] New client connection on fd %d\n", client_fd);
    fflush(stdout);
    
    // Receive command and filename
    int total_received = 0;
    while (total_received < MAX_CMD_SIZE - 1) {
        bytes_received = recv(client_fd, cmd_buf + total_received, 1, 0);
        if (bytes_received <= 0) {
            printf("[SERVER] Failed to receive command or client disconnected (bytes=%d)\n", bytes_received);
            return;
        }
        total_received += bytes_received;
        if (cmd_buf[total_received - 1] == '\n' || total_received >= MAX_CMD_SIZE - 1) {
            break;
        }
    }
    
    cmd_buf[total_received] = '\0';
    printf("[SERVER] Received command (%d bytes): %s", total_received, cmd_buf);
    fflush(stdout);

    // Parse command
    char* cmd = strtok(cmd_buf, " ");
    if (cmd == NULL) {
        printf("[SERVER] Invalid command format - no command found\n");
        fflush(stdout);
        return;
    }
    
    if (strcmp(cmd, "SEND") == 0) {
        // Handle SEND command
        char* filename = strtok(NULL, " ");
        if (filename == NULL) {
            printf("[SERVER] Missing filename for SEND command\n");
            fflush(stdout);
            return;
        }
        trim_end(filename);
        printf("[SERVER] Raw filename length: %zu\n", strlen(filename));
        printf("[SERVER] Raw filename bytes: ");
        for(size_t i = 0; i < strlen(filename); i++) {
            printf("%02x ", (unsigned char)filename[i]);
        }
        printf("\n");
        fflush(stdout);
        
        // Get absolute path
        if (realpath(filename, abs_path) != NULL) {
            printf("[SERVER] Absolute path for SEND: %s\n", abs_path);
        } else {
            printf("[SERVER] Could not resolve absolute path for: %s (errno=%d, strerror=%s)\n", 
                   filename, errno, strerror(errno));
        }
        fflush(stdout);
        
        printf("[SERVER] Processing SEND request for file: %s\n", filename);
        fflush(stdout);
        
        // Receive file size
        uint32_t file_size;
        bytes_received = recv(client_fd, &file_size, sizeof(file_size), 0);
        if (bytes_received != sizeof(file_size)) {
            printf("[SERVER] Failed to receive file size (bytes=%d)\n", bytes_received);
            fflush(stdout);
            return;
        }
        file_size = ntohl(file_size);
        printf("[SERVER] Expecting to receive %u bytes for file %s\n", file_size, filename);
        fflush(stdout);
        
        // Create file
        int fd = open(filename, O_WRONLY | O_CREAT | O_TRUNC, 0666);
        if (fd < 0) {
            printf("[SERVER] Failed to create file %s (errno=%d, strerror=%s)\n", filename, errno, strerror(errno));
            fflush(stdout);
            return;
        }
        printf("[SERVER] Opened file %s for writing (fd=%d)\n", filename, fd);
        fflush(stdout);
        
        // Receive and write file data
        char buffer[BUF_SIZE];
        uint32_t remaining = file_size;
        uint32_t total_written = 0;
        start_time = get_timestamp_ms();
        
        while (remaining > 0) {
            int to_read = remaining > BUF_SIZE ? BUF_SIZE : remaining;
            bytes_received = recv(client_fd, buffer, to_read, 0);
            if (bytes_received <= 0) {
                printf("[SERVER] Error or disconnect while receiving file data (bytes=%d)\n", bytes_received);
                fflush(stdout);
                close(fd);
                return;
            }
            int written = write(fd, buffer, bytes_received);
            if (written != bytes_received) {
                printf("[SERVER] Failed to write all data to file (written=%d, expected=%d)\n", written, bytes_received);
                fflush(stdout);
                close(fd);
                return;
            }
            remaining -= bytes_received;
            total_written += bytes_received;
            printf("[SERVER] Received %d bytes, %u bytes remaining (total written: %u)\n", 
                   bytes_received, remaining, total_written);
            fflush(stdout);
        }
        
        end_time = get_timestamp_ms();
        close(fd);
        printf("[SERVER] Finished writing file %s (%u bytes total) in %ld ms\n", 
               filename, total_written, end_time - start_time);
        fflush(stdout);
        
        // Verify file exists and check permissions
        struct stat st;
        if (stat(filename, &st) == 0) {
            printf("[SERVER] File verification: %s exists, size=%ld, permissions=%o\n", 
                   filename, (long)st.st_size, st.st_mode & 0777);
        } else {
            printf("[SERVER] File verification failed: %s (errno=%d, strerror=%s)\n", 
                   filename, errno, strerror(errno));
        }
        fflush(stdout);
        
        // Send success response
        const char* response = "OK\n";
        bytes_sent = send(client_fd, response, strlen(response), 0);
        if (bytes_sent != strlen(response)) {
            printf("[SERVER] Failed to send response (bytes=%d)\n", bytes_sent);
            fflush(stdout);
            return;
        }
        printf("[SERVER] Sent response: %s", response);
        fflush(stdout);
        
        // Verify file still exists after sending response
        if (stat(filename, &st) == 0) {
            printf("[SERVER] Post-SEND verification: File still exists, size=%ld, permissions=%o, inode=%ld\n", 
                   (long)st.st_size, st.st_mode & 0777, (long)st.st_ino);
        } else {
            printf("[SERVER] Post-SEND verification: File no longer exists (errno=%d, strerror=%s)\n", 
                   errno, strerror(errno));
        }
        fflush(stdout);
        
        // Close client connection
        close(client_fd);
        printf("[SERVER] Client connection closed\n");
        fflush(stdout);
        
    } else if (strcmp(cmd, "GET") == 0) {
        // Handle GET command
        char* filename = strtok(NULL, " ");
        if (filename == NULL) {
            printf("[SERVER] Missing filename for GET command\n");
            fflush(stdout);
            return;
        }
        trim_end(filename);
        printf("[SERVER] Raw filename length: %zu\n", strlen(filename));
        printf("[SERVER] Raw filename bytes: ");
        for(size_t i = 0; i < strlen(filename); i++) {
            printf("%02x ", (unsigned char)filename[i]);
        }
        printf("\n");
        fflush(stdout);
        
        // Get absolute path
        if (realpath(filename, abs_path) != NULL) {
            printf("[SERVER] Absolute path for GET: %s\n", abs_path);
        } else {
            printf("[SERVER] Could not resolve absolute path for: %s (errno=%d, strerror=%s)\n", 
                   filename, errno, strerror(errno));
        }
        fflush(stdout);
        
        printf("[SERVER] Processing GET request for file: %s\n", filename);
        fflush(stdout);
        
        // Check file existence and permissions before opening
        struct stat st;
        if (stat(filename, &st) == 0) {
            printf("[SERVER] Pre-open check: File exists, size=%ld, permissions=%o, inode=%ld\n", 
                   (long)st.st_size, st.st_mode & 0777, (long)st.st_ino);
        } else {
            printf("[SERVER] Pre-open check: File not found (errno=%d, strerror=%s)\n", 
                   errno, strerror(errno));
        }
        fflush(stdout);
        
        // Open file
        int fd = open(filename, O_RDONLY);
        if (fd < 0) {
            printf("[SERVER] File not found: %s (errno=%d, strerror=%s)\n", filename, errno, strerror(errno));
            fflush(stdout);
            const char* response = "ERROR: File not found\n";
            send(client_fd, response, strlen(response), 0);
            return;
        }
        printf("[SERVER] Successfully opened file %s for reading (fd=%d)\n", filename, fd);
        fflush(stdout);
        
        // Get file size
        fstat(fd, &st);
        uint32_t file_size = st.st_size;
        
        printf("[SERVER] Sending file %s of size %u bytes\n", filename, file_size);
        fflush(stdout);
        
        // Send file size in network byte order
        uint32_t net_size = htonl(file_size);
        bytes_sent = send(client_fd, &net_size, sizeof(net_size), 0);
        if (bytes_sent != sizeof(net_size)) {
            printf("[SERVER] Failed to send file size (bytes=%d)\n", bytes_sent);
            fflush(stdout);
            close(fd);
            return;
        }
        
        // Send file data
        char buffer[BUF_SIZE];
        uint32_t remaining = file_size;
        uint32_t total_sent = 0;
        start_time = get_timestamp_ms();
        
        while (remaining > 0) {
            size_t to_read = remaining < BUF_SIZE ? remaining : BUF_SIZE;
            ssize_t nread = read(fd, buffer, to_read);
            if (nread < 0) {
                printf("[SERVER] read error (errno=%d)\n", errno);
                fflush(stdout);
                break;
            }
            if (nread == 0) {
                printf("[SERVER] unexpected EOF after %u/%u bytes\n", total_sent, file_size);
                fflush(stdout);
                break;
            }

            bytes_sent = send(client_fd, buffer, nread, 0);
            if (bytes_sent != nread) {
                printf("[SERVER] send failed (sent=%d/%zd)\n", bytes_sent, nread);
                fflush(stdout);
                break;
            }

            remaining -= bytes_sent;
            total_sent += bytes_sent;
            printf("[SERVER] Sent %d bytes, total %u/%u\n", bytes_sent, total_sent, file_size);
            fflush(stdout);
        }
        
        end_time = get_timestamp_ms();
        close(fd);
        printf("[SERVER] Finished sending file %s (%u bytes total) in %ld ms\n", 
               filename, total_sent, end_time - start_time);
        fflush(stdout);
        
        // Wait for client acknowledgment
        char ack[3];  // "OK\n"
        int ack_bytes;
        printf("[SERVER] Waiting for client acknowledgment...\n");
        fflush(stdout);
        
        ack_bytes = recv(client_fd, ack, sizeof(ack), 0);
        if (ack_bytes == sizeof(ack) && memcmp(ack, "OK\n", 3) == 0) {
            printf("[SERVER] Received client acknowledgment\n");
        } else {
            printf("[SERVER] No acknowledgment received from client (ret=%d)\n", ack_bytes);
        }
        fflush(stdout);
        
        // Close the client connection
        close(client_fd);
        printf("[SERVER] Client connection closed\n");
        fflush(stdout);
        return;
        
    } else {
        printf("[SERVER] Unknown command: %s\n", cmd);
        fflush(stdout);
        const char* response = "ERROR: Unknown command\n";
        send(client_fd, response, strlen(response), 0);
    }
}

int main() {
    int server_fd, client_fd;
    struct sockaddr_in server_addr, client_addr;
    socklen_t client_len = sizeof(client_addr);
    
    printf("[SERVER] Starting native image server...\n");
    fflush(stdout);
    
    // Create socket
    server_fd = socket(AF_INET, SOCK_STREAM, 0);
    if (server_fd < 0) {
        perror("Failed to create socket");
        return 1;
    }
    
    // Set socket options to reuse address
    int opt = 1;
    if (setsockopt(server_fd, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt)) < 0) {
        perror("Failed to set socket options");
        return 1;
    }
    
    // Bind socket
    memset(&server_addr, 0, sizeof(server_addr));
    server_addr.sin_family = AF_INET;
    server_addr.sin_addr.s_addr = INADDR_ANY;
    server_addr.sin_port = htons(PORT);
    
    if (bind(server_fd, (struct sockaddr*)&server_addr, sizeof(server_addr)) < 0) {
        perror("Failed to bind socket");
        return 1;
    }
    
    // Listen for connections
    if (listen(server_fd, 5) < 0) {
        perror("Failed to listen on socket");
        return 1;
    }
    
    printf("[SERVER] Server listening on port %d\n", PORT);
    fflush(stdout);
    
    // Main server loop
    while (1) {
        // Accept connection
        client_fd = accept(server_fd, (struct sockaddr*)&client_addr, &client_len);
        if (client_fd < 0) {
            perror("Failed to accept connection");
            continue;
        }
        
        printf("[SERVER] Accepted connection from %s:%d\n", 
               inet_ntoa(client_addr.sin_addr), ntohs(client_addr.sin_port));
        fflush(stdout);
        
        // Handle client
        handle_client(client_fd);
        
        // Note: Client connection is now closed in handle_client
        printf("[SERVER] Ready for next connection\n");
        fflush(stdout);
    }
    
    return 0;
} 