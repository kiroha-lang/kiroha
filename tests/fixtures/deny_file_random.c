#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <sys/syscall.h>
#define syscall unused_syscall_declaration
#include <unistd.h>
#undef syscall
long syscall(long n, long a, long b, long c, long d, long e, long f) {
    static long (*next)(long, ...) = 0;
    if (!next) next = dlsym(RTLD_NEXT, "syscall");
    if (n == SYS_getrandom && b == 8) {
        const char msg[] = "audit-denied-getrandom8\n";
        write(2, msg, sizeof(msg)-1);
        errno = EIO;
        return -1;
    }
    return next(n,a,b,c,d,e,f);
}
