// test_many_small_files.c
#include <stdio.h>
#include <fcntl.h>
#include <unistd.h>
#include <string.h>

int main(void) {
    // Write many small files until we hit the disk limit
    const char *buf = "This is a small file content.\n";
    size_t len = strlen(buf);
    int success_count = 0;
    int max_files = 10000;
    
    printf("Starting to create many small files...\n");
    
    for (int i = 0; i < max_files; i++) {
        char filename[32];
        snprintf(filename, sizeof(filename), "smallfile_%d.txt", i);
        
        int fd = open(filename, O_WRONLY | O_CREAT, 0666);
        if (fd < 0) {
            printf("Failed to create file %s after %d successful files\n", 
                   filename, success_count);
            return 1;
        }
        
        ssize_t written = write(fd, buf, len);
        close(fd);
        
        if (written < 0) {
            printf("Write failed at file %d\n", i);
            return 1;
        }
        
        success_count++;
        
        // Print progress once in a while
        if (i % 100 == 0) {
            printf("Created %d files so far...\n", i);
        }
    }
    
    printf("Finished creating %d files without hitting limit?\n", success_count);
    return 0;
} 