// program_b.c
#include <stdio.h>
#include <unistd.h>
extern void __builtin_rt_yield(void);

int main() {
    printf("Program D: yield\n");
    fflush(stdout);
    __builtin_rt_yield();
    printf("Program D: executing again\n");
    fflush(stdout);
    return 0;
}
