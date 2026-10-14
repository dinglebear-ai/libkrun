// A real KVM inner guest stays in KVM_RUN while its parent reports progress.
// Static Linux executable: cc -static -pthread guest.c -o guest
#include <errno.h>
#include <fcntl.h>
#include <linux/kvm.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/mount.h>
#include <sys/random.h>
#include <unistd.h>

static int cpu;
static volatile uint32_t *counter;
static struct kvm_run *run;
static void fail(const char *s) { perror(s); exit(1); }
static void *execute(void *unused) {
    (void)unused;
    for (;;) {
        if (ioctl(cpu, KVM_RUN, 0) < 0 && errno != EINTR) fail("KVM_RUN");
        if (run->exit_reason != KVM_EXIT_INTR) {
            fprintf(stderr, "unexpected inner exit %u\n", run->exit_reason);
            exit(1);
        }
    }
}
int main(void) {
    uint64_t identity;
    if (getrandom(&identity, sizeof(identity), 0) != sizeof(identity)) fail("getrandom");
    if (mount("devtmpfs", "/dev", "devtmpfs", 0, NULL) < 0 && errno != EBUSY) fail("mount /dev");
    int kvm = open("/dev/kvm", O_RDWR | O_CLOEXEC);
    if (kvm < 0) fail("open /dev/kvm");
    int vm = ioctl(kvm, KVM_CREATE_VM, 0);
    if (vm < 0) fail("KVM_CREATE_VM");
    if (ioctl(vm, KVM_CREATE_IRQCHIP, 0) < 0) fail("KVM_CREATE_IRQCHIP");
    struct kvm_pit_config pit = {0};
    if (ioctl(vm, KVM_CREATE_PIT2, &pit) < 0) fail("KVM_CREATE_PIT2");
    uint8_t *memory = mmap(NULL, 0x10000, PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
    if (memory == MAP_FAILED) fail("mmap");
    // PIC initialization, periodic PIT channel 0, then a busy counter with IRQs enabled.
    const uint8_t code[] = {
        0xfa, 0xb0,0x11,0xe6,0x20, 0xb0,0x08,0xe6,0x21,
        0xb0,0x04,0xe6,0x21, 0xb0,0x01,0xe6,0x21,
        0xb0,0xfe,0xe6,0x21, 0xb0,0x36,0xe6,0x43,
        0xb0,0xff,0xe6,0x40,0xe6,0x40, 0xfb,
        0x66,0xff,0x06,0x00,0x08, 0xeb,0xf9
    };
    const uint8_t interrupt[] = {0x50,0x66,0xff,0x06,0x04,0x08,0xb0,0x20,0xe6,0x20,0x58,0xcf};
    memcpy(memory+0x1000, code, sizeof(code));
    memcpy(memory+0x300, interrupt, sizeof(interrupt));
    memory[8*4]=0; memory[8*4+1]=3;
    counter = (volatile uint32_t *)(memory+0x800);
    struct kvm_userspace_memory_region region = { .slot=0, .guest_phys_addr=0, .memory_size=0x10000, .userspace_addr=(uintptr_t)memory };
    if (ioctl(vm,KVM_SET_USER_MEMORY_REGION,&region)<0) fail("KVM_SET_USER_MEMORY_REGION");
    cpu=ioctl(vm,KVM_CREATE_VCPU,0); if(cpu<0)fail("KVM_CREATE_VCPU");
    int size=ioctl(kvm,KVM_GET_VCPU_MMAP_SIZE,0);
    run=mmap(NULL,size,PROT_READ|PROT_WRITE,MAP_SHARED,cpu,0);
    if(run==MAP_FAILED)fail("mmap kvm_run");
    struct kvm_sregs s;
    if(ioctl(cpu,KVM_GET_SREGS,&s)<0)fail("GET_SREGS");
    s.cs.base=0; s.cs.selector=0;
    if(ioctl(cpu,KVM_SET_SREGS,&s)<0)fail("SET_SREGS");
    struct kvm_regs r={.rip=0x1000,.rsp=0x7000,.rflags=2};
    if(ioctl(cpu,KVM_SET_REGS,&r)<0)fail("SET_REGS");
    pthread_t thread;
    if(pthread_create(&thread,NULL,execute,NULL))fail("pthread_create");
    for (;;) {
        printf("NESTED_PROGRESS %u %u %llu\n", counter[0], counter[1], (unsigned long long)identity);
        fflush(stdout);
        usleep(100000);
    }
}
