/* Windows x86 dispatcher oracle, not game code. Compile as C /Od /EHsc.
 * Raw chain cases require /SAFESEH:NO (the C handler is not an assembler
 * .safeseh symbol). The compiler-generated __try case also builds /SAFESEH.
 * Output contains stable decisions, never host pointer values.
 */
#include <windows.h>
#include <stdio.h>
#include <stddef.h>
static int scenario;
static DWORD outer_frame;
static EXCEPTION_RECORD *active;
static DWORD fault_ip, resume_ip;
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
    if (scenario == 3 && !(r->ExceptionFlags & EXCEPTION_UNWINDING))
        RtlUnwind((void*)outer_frame, 0, r, (void*)0x1234);
    if (r->ExceptionFlags & EXCEPTION_UNWINDING) { c->Eax=0xbeef; return ExceptionContinueSearch; }
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
    printf("divide code=%08lx ip=%d eax=%lu ecx=%lu edx=%lu\n",
        p->ExceptionRecord->ExceptionCode,c->Eip==fault_ip,c->Eax,c->Ecx,c->Edx);
    c->Eip=resume_ip; c->Eax=42;
    return EXCEPTION_CONTINUE_EXECUTION;
}

int main(int argc, char **argv) {
    ULONG_PTR values[] = {0x12345678};
    __try { RaiseException(0xe0420000, 0, 1, values); }
    __except(filter(GetExceptionInformation())) { puts("caught"); }
    __asm { mov fault_ip, offset faulting_divide }
    __asm { mov resume_ip, offset after_divide }
    __try {
        __asm { mov eax, 1 }
        __asm { xor edx, edx }
        __asm { xor ecx, ecx }
faulting_divide:
        __asm { div ecx }
after_divide:
        __asm { mov scenario, eax }
    } __except(divide_filter(GetExceptionInformation())) { puts("unexpected divide handler"); }
    printf("divide resumed eax=%d\n",scenario);
#ifndef SAFE_ONLY
    for (scenario = 1; scenario <= 4; ++scenario) {
        Frame a, b;
        __asm { mov eax, fs:[0] }
        __asm { mov b.next, eax }
        b.handler = handler; b.id = 2;
        a.next = &b; a.handler = handler; a.id = 1;
        /* MSVC local layout is not guaranteed ascending: register separate
         * nested functions instead if the stack validator rejects it. */
        if (&a > &b) { puts("bad fixture stack order"); return 2; }
        __asm { lea eax, a }
        __asm { mov fs:[0], eax }
        if (scenario == 3) {
            RtlUnwind(&b, (void*)0x12345678, NULL, (void*)0x1234);
            __asm { mov outer_frame, eax }
            printf("unwind eax=%lx\n",outer_frame);
            puts("unwind returned");
        } else RaiseException(0xe0420001, 0, 0, NULL);
        __asm { mov eax, b.next }
        __asm { mov fs:[0], eax }
        printf("scenario %d returned\n", scenario);
    }
#endif
    return 0;
}
