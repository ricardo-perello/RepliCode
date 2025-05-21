// test_many_empty_files.c
#include <stdio.h>
#include <fcntl.h>
#include <unistd.h>

int main(void) {
    // Create many empty files until we hit the disk limit
    int success_count = 0;
    int max_files = 50000; // Try with a larger number since empty files use less space
    
    printf("Starting to create many empty files...\n");
    
    for (int i = 0; i < max_files; i++) {
        char filename[32];
        snprintf(filename, sizeof(filename), "emptyfile_%d.txt", i);
        
        int fd = open(filename, O_WRONLY | O_CREAT, 0666);
        if (fd < 0) {
            printf("Failed to create file %s after %d successful files\n", 
                   filename, success_count);
            return 1;
        }
        
        // Just create the file, don't write anything to it
        close(fd);
        success_count++;
        
        // Print progress once in a while
        if (i % 500 == 0) {
            printf("Created %d empty files so far...\n", i);
        }
    }
    
    printf("Finished creating %d empty files without hitting limit?\n", success_count);
    return 0;
} 