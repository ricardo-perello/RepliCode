// program_b.c
#include <stdio.h>
#include <unistd.h>
extern void __builtin_rt_yield(void);

int main() {
    printf("Program D: Before sleep\n");
    fflush(stdout);
    printf("pausing");
    fflush(stdout);
    __builtin_rt_yield();
    printf("executing again");
    printf("Program D: After sleep\n");
    fflush(stdout);
    return 0;
}
