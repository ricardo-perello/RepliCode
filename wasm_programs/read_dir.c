#include <stdio.h>
#include <dirent.h>  // opendir, readdir, closedir

int main(void) {
    // 1) Open the current directory
    DIR *dirp = opendir(".");
    if (!dirp) {
        printf("Failed to open current directory.\n");
        return 1;
    }

    // 2) Read all entries and print their names
    struct dirent *ent;
    while ((ent = readdir(dirp)) != NULL) {
        printf("Found entry: %s\n", ent->d_name);
    }

    // 3) Close the directory
    closedir(dirp);
    return 0;
}
