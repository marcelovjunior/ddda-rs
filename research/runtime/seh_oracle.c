/* Windows x86 dispatcher oracle, not game code. Compile as C /Od /EHsc.
 * Raw chain cases require /SAFESEH:NO (the C handler is not an assembler
 * .safeseh symbol). The compiler-generated __try case also builds /SAFESEH.
 * Output contains stable decisions, never host pointer values.
 */
#include <windows.h>
#include <stdio.h>
#include <stddef.h>
static int scenario;
static DWORD unwind_eax;
static DWORD collided_frame;
static EXCEPTION_RECORD *active;
static DWORD fault_ip, resume_ip;
static BYTE *split_page;
static int split_filter(EXCEPTION_POINTERS *p) {
    printf("split code=%08lx kind=%lu bad_page=%d prefix=%04x\n",
        p->ExceptionRecord->ExceptionCode, p->ExceptionRecord->ExceptionInformation[0],
        p->ExceptionRecord->ExceptionInformation[1] == (ULONG_PTR)(split_page+4096),
        *(WORD*)(split_page+4094));
    return EXCEPTION_EXECUTE_HANDLER;
}
__declspec(noinline) static void inner_fault(void) { *(volatile DWORD*)0xdead=1; }
__declspec(noinline) static void outer_fault(void) { inner_fault(); puts("unexpected inner return"); }
typedef struct Frame { struct Frame *next; void *handler; DWORD id; } Frame;

static EXCEPTION_DISPOSITION __cdecl handler(EXCEPTION_RECORD *r, Frame *f,
                                            CONTEXT *c, void *dispatcher) {
    printf("handler %lu code=%08lx flags=%lx eax=%08lx\n", f->id,
           r->ExceptionCode, r->ExceptionFlags, c->Eax);
    if ((scenario == 2 || scenario == 4) && r->ExceptionCode == 0xe0420001 && f->id == 1) {
        Frame local;
        active = r;
        if (scenario == 4) {
            __asm { mov eax, fs:[0] }
            __asm { mov local.next, eax }
            local.handler=handler;local.id=3;
            __asm { lea eax, local }
            __asm { mov fs:[0], eax }
        }
        RaiseException(0xe0420002, 0, 0, NULL);
        if (scenario == 4) {
            __asm { mov eax, local.next }
            __asm { mov fs:[0], eax }
        }
        puts("nested returned");
    }
    if (r->ExceptionCode == 0xe0420002) {
        printf("nested link=%d\n", r->ExceptionRecord == active);
        if (f->id == 2) return ExceptionContinueExecution;
    }
    if (r->ExceptionFlags & EXCEPTION_UNWINDING) {
        c->Eax=0xbeef;
        if (scenario==5 && f->id==1) {
            *(DWORD*)dispatcher=collided_frame;
            return ExceptionCollidedUnwind;
        }
        return ExceptionContinueSearch;
    }
    if (f->id == 2) { c->Eax = 0xabcdef01; return ExceptionContinueExecution; }
    return ExceptionContinueSearch;
}

static int filter(EXCEPTION_POINTERS *p) {
    printf("layout record=%u context=%u eip=%u esp=%u eax=%u\n",
        (unsigned)sizeof(EXCEPTION_RECORD), (unsigned)sizeof(CONTEXT),
        (unsigned)offsetof(CONTEXT,Eip), (unsigned)offsetof(CONTEXT,Esp),
        (unsigned)offsetof(CONTEXT,Eax));
    printf("filter code=%08lx args=%lu value=%08lx\n", p->ExceptionRecord->ExceptionCode,
        p->ExceptionRecord->NumberParameters, p->ExceptionRecord->ExceptionInformation[0]);
    printf("selectors cs=%lx ss=%lx\n",p->ContextRecord->SegCs & 0xffff,p->ContextRecord->SegSs & 0xffff);
    return EXCEPTION_EXECUTE_HANDLER;
}

static int divide_filter(EXCEPTION_POINTERS *p) {
    CONTEXT *c=p->ContextRecord;
    DWORD live_flags;
    __asm { pushfd }
    __asm { pop live_flags }
    __asm { cld }
    printf("direction saved=%lu handler=%lu\n",(c->EFlags>>10)&1,(live_flags>>10)&1);
    printf("arithmetic flags=%lx\n",c->EFlags & 0x8c5); /* XOR leaves AF undefined. */
    printf("divide code=%08lx ip=%d eax=%lu ecx=%lu edx=%lu\n",
        p->ExceptionRecord->ExceptionCode,c->Eip==fault_ip,c->Eax,c->Ecx,c->Edx);
    c->Eip=resume_ip; c->Eax=42; c->EFlags&=~0x400;
    return EXCEPTION_CONTINUE_EXECUTION;
}

static int access_filter(EXCEPTION_POINTERS *p) {
    printf("access code=%08lx kind=%lu address=%08lx ip=%d\n",
        p->ExceptionRecord->ExceptionCode,p->ExceptionRecord->ExceptionInformation[0],
        p->ExceptionRecord->ExceptionInformation[1],p->ContextRecord->Eip==fault_ip);
    p->ContextRecord->Eip=resume_ip;
    return EXCEPTION_CONTINUE_EXECUTION;
}

int main(int argc, char **argv) {
    ULONG_PTR values[] = {0x12345678};
    DWORD old_protect;
    split_page=(BYTE*)VirtualAlloc(NULL,8192,MEM_RESERVE|MEM_COMMIT,PAGE_READWRITE);
    if (!split_page) return 2;
    *(WORD*)(split_page+4094)=0x1234;
    if (!VirtualProtect(split_page+4096,4096,PAGE_NOACCESS,&old_protect)) return 3;
    __try { *(volatile DWORD*)(split_page+4094)=0xffffffff; }
    __except(split_filter(GetExceptionInformation())) { puts("split caught"); }
    VirtualFree(split_page,0,MEM_RELEASE);
    __try { outer_fault(); puts("unexpected outer return"); }
    __except(GetExceptionCode()==EXCEPTION_ACCESS_VIOLATION ? EXCEPTION_EXECUTE_HANDLER : EXCEPTION_CONTINUE_SEARCH) {
        puts("nonlocal access caught");
    }
    __try { RaiseException(0xe0420000, 0, 1, values); }
    __except(filter(GetExceptionInformation())) { puts("caught"); }
    __asm { mov fault_ip, offset faulting_divide }
    __asm { mov resume_ip, offset after_divide }
    __try {
        __asm { mov eax, 1 }
        __asm { xor edx, edx }
        __asm { xor ecx, ecx }
        __asm { std }
faulting_divide:
        __asm { div ecx }
after_divide:
        __asm { mov scenario, eax }
    } __except(divide_filter(GetExceptionInformation())) { puts("unexpected divide handler"); }
    printf("divide resumed eax=%d\n",scenario);
    __asm { mov fault_ip, offset faulting_write }
    __asm { mov resume_ip, offset after_write }
    __try {
        __asm { mov eax, 0xdead }
faulting_write:
        __asm { mov dword ptr [eax], 1 }
after_write:
        ;
    } __except(access_filter(GetExceptionInformation())) { puts("unexpected access handler"); }
#ifndef SAFE_ONLY
    for (scenario = 1; scenario <= 5; ++scenario) {
        Frame chain[3];
        DWORD previous, remaining;
        Frame *target=&chain[scenario==5 ? 2 : 1];
        __asm { mov eax, fs:[0] }
        __asm { mov previous, eax }
        chain[2].next=(Frame*)previous;chain[2].handler=handler;chain[2].id=3;
        chain[1].next=scenario==5 ? &chain[2] : (Frame*)previous;
        chain[1].handler=handler;chain[1].id=2;
        chain[0].next=&chain[1];chain[0].handler=handler;chain[0].id=1;
        collided_frame=(DWORD)&chain[1];
        __asm { lea eax, chain }
        __asm { mov fs:[0], eax }
        if (scenario == 3 || scenario == 5) {
            RtlUnwind(target, (void*)0x12345678, NULL, (void*)0x1234);
            __asm { mov unwind_eax, eax }
            __asm { mov eax, fs:[0] }
            __asm { mov remaining, eax }
            printf("unwind eax=%lx\n",unwind_eax);
            printf("unwind target retained=%d\n",remaining==(DWORD)target);
            puts("unwind returned");
        } else RaiseException(0xe0420001, 0, 0, NULL);
        __asm { mov eax, previous }
        __asm { mov fs:[0], eax }
        printf("scenario %d returned\n", scenario);
    }
#endif
    return 0;
}
