#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <unistd.h>

int main() {
    int server_fd;
    int client_fd;
    int ret;
    int bytes_received;
    int bytes_sent;
    char buffer[1024];
    struct sockaddr_in server_addr;
    
    // Create a socket
    server_fd = socket(AF_INET, SOCK_STREAM, 0);
    if (server_fd < 0) {
        printf("Failed to open socket\n");
        return 1;
    }
    printf("Server socket opened with fd: %d\n", server_fd);

    // Set up server address
    memset(&server_addr, 0, sizeof(server_addr));
    server_addr.sin_family = AF_INET;
    server_addr.sin_addr.s_addr = INADDR_ANY;
    server_addr.sin_port = htons(7000);

    // Bind the socket
    ret = bind(server_fd, (struct sockaddr*)&server_addr, sizeof(server_addr));
    if (ret < 0) {
        printf("Failed to bind socket\n");
        return 1;
    }

    // Listen on port 7000
    ret = listen(server_fd, 5); // backlog of 5
    if (ret < 0) {
        printf("Failed to listen on socket\n");
        return 1;
    }
    printf("Server listening on port 7000\n");
    fflush(stdout);

    // Accept a connection
    struct sockaddr_in client_addr;
    socklen_t client_len = sizeof(client_addr);
    client_fd = accept(server_fd, (struct sockaddr*)&client_addr, &client_len);
    if (client_fd < 0) {
        printf("Failed to accept connection (error: %d)\n", client_fd);
        return 1;
    }
    printf("Accepted connection with client fd: %d\n", client_fd);

    // Main message loop
    while (1) {
        // Receive data from client
        bytes_received = recv(client_fd, buffer, sizeof(buffer) - 1, 0);
        if (bytes_received < 0) {
            printf("Failed to receive data\n");
            break;
        }
        if (bytes_received == 0) {
            printf("Client disconnected\n");
            break;
        }
        
        // Null terminate the received data
        buffer[bytes_received] = '\0';
        printf("Received %d bytes: %s\n", bytes_received, buffer);

        // Echo back to client
        bytes_sent = send(client_fd, buffer, bytes_received, 0);
        if (bytes_sent < 0) {
            printf("Failed to send data\n");
            break;
        }
        printf("Echoed %d bytes back to client\n", bytes_sent);
    }

    // Close the connection
    ret = close(client_fd);
    if (ret < 0) {
        printf("Failed to close client socket\n");
        return 1;
    }
    printf("Client socket closed successfully\n");

    // Close the server socket
    ret = close(server_fd);
    if (ret < 0) {
        printf("Failed to close server socket\n");
        return 1;
    }
    printf("Server socket closed successfully\n");

    return 0;
} 