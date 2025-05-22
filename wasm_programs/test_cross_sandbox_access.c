// test_cross_sandbox_access.c
// This program attempts to access another process's sandbox
// by using relative paths like "../pid_X/"
// WARNING: If it accesses its own sandbox, it will still declare a breach even though it's allowed. This is a limitation of the current implementation.

#include <stdio.h>
#include <fcntl.h>
#include <unistd.h>
#include <string.h>
#include <errno.h>
#include <dirent.h>
#include <sys/stat.h>

int main(void) {
    printf("Starting cross-sandbox access test...\n");
    
    // First create our own test file to verify we have write access
    const char *our_file = "our_test_file.txt";
    const char *content = "This is our test file content";
    
    int fd = open(our_file, O_WRONLY | O_CREAT, 0666);
    if (fd < 0) {
        printf("Failed to create our own test file: %s (errno: %d)\n", our_file, errno);
        return 1;
    }
    
    write(fd, content, strlen(content));
    close(fd);
    printf("Successfully created our own test file: %s\n", our_file);
    
    // Now try to access other process sandboxes
    // We'll try PIDs 1-10 assuming one of them might exist
    for (int pid = 1; pid <= 10; pid++) {
        char path[100];
        char invasion_file[120];
        
        // Try direct parent directory access
        snprintf(path, sizeof(path), "../pid_%d", pid);
        printf("\nAttempting to access directory: %s\n", path);
        
        // Try to open the directory
        DIR *dir = opendir(path);
        if (dir != NULL) {
            printf("SECURITY BREACH! Successfully opened directory %s\n", path);
            
            // List directory contents
            printf("Directory contents:\n");
            struct dirent *entry;
            while ((entry = readdir(dir)) != NULL) {
                printf("  %s\n", entry->d_name);
            }
            closedir(dir);
            
            // Try to create a file in that directory
            snprintf(invasion_file, sizeof(invasion_file), "%s/INVASION.txt", path);
            printf("Attempting to create file: %s\n", invasion_file);
            
            fd = open(invasion_file, O_WRONLY | O_CREAT, 0666);
            if (fd >= 0) {
                printf("SECURITY BREACH! Successfully created file in another sandbox!\n");
                write(fd, "This sandbox has been compromised!", 34);
                close(fd);
            } else {
                printf("Failed to create file in other sandbox (errno: %d)\n", errno);
            }
        } else {
            printf("Failed to access directory %s (errno: %d)\n", path, errno);
        }
        
        // Try with absolute-looking path
        snprintf(path, sizeof(path), "/pid_%d", pid);
        printf("\nAttempting to access directory: %s\n", path);
        
        dir = opendir(path);
        if (dir != NULL) {
            printf("SECURITY BREACH! Successfully opened directory %s\n", path);
            closedir(dir);
            
            // Try to create a file in that directory
            snprintf(invasion_file, sizeof(invasion_file), "%s/INVASION.txt", path);
            printf("Attempting to create file: %s\n", invasion_file);
            
            fd = open(invasion_file, O_WRONLY | O_CREAT, 0666);
            if (fd >= 0) {
                printf("SECURITY BREACH! Successfully created file in another sandbox!\n");
                write(fd, "This sandbox has been compromised!", 34);
                close(fd);
            } else {
                printf("Failed to create file in other sandbox (errno: %d)\n", errno);
            }
        } else {
            printf("Failed to access directory %s (errno: %d)\n", path, errno);
        }
        
        // Try with a deeper path traversal
        snprintf(path, sizeof(path), "../../pid_%d", pid);
        printf("\nAttempting to access directory: %s\n", path);
        
        dir = opendir(path);
        if (dir != NULL) {
            printf("SECURITY BREACH! Successfully opened directory %s\n", path);
            closedir(dir);
            
            // Try to create a file in that directory
            snprintf(invasion_file, sizeof(invasion_file), "%s/INVASION.txt", path);
            printf("Attempting to create file: %s\n", invasion_file);
            
            fd = open(invasion_file, O_WRONLY | O_CREAT, 0666);
            if (fd >= 0) {
                printf("SECURITY BREACH! Successfully created file in another sandbox!\n");
                write(fd, "This sandbox has been compromised!", 34);
                close(fd);
            } else {
                printf("Failed to create file in other sandbox (errno: %d)\n", errno);
            }
        } else {
            printf("Failed to access directory %s (errno: %d)\n", path, errno);
        }
    }
    
    printf("\nCross-sandbox access test completed.\n");
    return 0;
} 