/* L46: 32-bit x87 FNSAVE/FRSTOR/FNSTENV/FLDENV observations on the real FPU.
 *
 * Build and run with x87_state_oracle.ps1 (MSVC x86, as seh_oracle.c). Each
 * experiment prints the saved image as hex next to the guest addresses it
 * should contain, so the rules (which instructions set the instruction and
 * data pointers, selectors, reserved words, the state FNSAVE leaves,
 * what FRSTOR keeps) are read off the hardware, not assumed.
 */
#include <stdio.h>
#include <string.h>

static unsigned char image[108], image2[108], env[28], crafted[108];
static double memv = 1.5;
static double outv;
static unsigned short cw_after, sw_after;

static void dump(const char *name, const unsigned char *p, int n) {
    int k;
    printf("%s", name);
    for (k = 0; k < n; k++) printf("%s%02x", k % 4 ? "" : " ", p[k]);
    printf("\n");
}

int main(void) {
    unsigned int ip_fld, ip_fadd, ip_fstp, ip_ctrl;
    unsigned short cs_sel, ds_sel;
    __asm {
        mov ax, cs
        mov cs_sel, ax
        mov ax, ds
        mov ds_sel, ax
    }
    printf("selectors cs=%x ds=%x\n", cs_sel, ds_sel);
    printf("data memv=%08x outv=%08x\n", (unsigned)&memv, (unsigned)&outv);

    /* E1: memory operand last (fld m64) */
    __asm {
        finit
        lea eax, f1
        mov ip_fld, eax
    f1: fld qword ptr memv
        fnsave image
    }
    printf("E1 fld_ip=%08x\n", ip_fld);
    dump("E1", image, 108);
    __asm { fninit }

    /* E2: register op after a memory op: FDP kept or cleared? */
    __asm {
        finit
        fld qword ptr memv
        fld1
        lea eax, f2
        mov ip_fadd, eax
    f2: fadd st, st(1)
        fnsave image
    }
    printf("E2 fadd_ip=%08x\n", ip_fadd);
    dump("E2", image, 108);
    __asm { fninit }

    /* E3: control instructions after a store: FIP stays on the store? */
    __asm {
        finit
        fld qword ptr memv
        lea eax, f3
        mov ip_fstp, eax
        lea eax, f4
        mov ip_ctrl, eax
    f3: fst qword ptr outv
    f4: fnstcw cw_after
        fnstsw sw_after
        fnclex
        fwait
        fnsave image
    }
    printf("E3 fst_ip=%08x ctrl_ip=%08x\n", ip_fstp, ip_ctrl);
    dump("E3", image, 108);

    /* E4: the state FNSAVE leaves (as FNINIT) */
    __asm {
        fnstcw cw_after
        fnstsw sw_after
        fnstenv env
    }
    printf("E4 cw=%04x sw=%04x\n", cw_after, sw_after);
    dump("E4env", env, 28);
    __asm { fninit }

    /* E5: FRSTOR of a crafted image, then FNSAVE: what round-trips */
    memset(crafted, 0, 108);
    crafted[0] = 0x7f; crafted[1] = 0x02; crafted[2] = 0xaa; crafted[3] = 0x55; /* cw + reserved */
    crafted[4] = 0x00; crafted[5] = 0x30; crafted[6] = 0xcc; crafted[7] = 0x33; /* sw top=6 + reserved */
    crafted[8] = 0xff; crafted[9] = 0x4f; crafted[10] = 0x11; crafted[11] = 0x22; /* tags: 6 valid(0),7 zero(1)? */
    crafted[12] = 0x78; crafted[13] = 0x56; crafted[14] = 0x34; crafted[15] = 0x12; /* fip */
    crafted[16] = 0x23; crafted[17] = 0x00; crafted[18] = 0xdd; crafted[19] = 0x07; /* fcs + fop */
    crafted[20] = 0xef; crafted[21] = 0xbe; crafted[22] = 0xad; crafted[23] = 0xde; /* fdp */
    crafted[24] = 0x2b; crafted[25] = 0x00; crafted[26] = 0x99; crafted[27] = 0x88; /* fds + reserved */
    /* st0 (phys 6) = 1.0, st1 (phys 7) = 2.0 */
    crafted[28 + 7] = 0x80; crafted[28 + 8] = 0xff; crafted[28 + 9] = 0x3f;
    crafted[38 + 7] = 0x80; crafted[38 + 8] = 0x00; crafted[38 + 9] = 0x40;
    __asm {
        frstor crafted
        fnsave image
    }
    dump("E5in ", crafted, 108);
    dump("E5out", image, 108);
    __asm { fninit }

    /* E6: FNSTENV/FLDENV round trip, and the masks FNSTENV sets */
    __asm {
        finit
        fld qword ptr memv
        fnstenv env
        fnstcw cw_after
        fldenv env
        fnsave image
    }
    printf("E6 cw_after_fnstenv=%04x\n", cw_after);
    dump("E6env", env, 28);
    dump("E6", image, 108);
    __asm { fninit }

    /* E7: FRSTOR with a wrong tag (valid tag on a zero value, zero tag on 1.0) */
    memcpy(crafted, image2, 108);
    memset(crafted, 0, 108);
    crafted[0] = 0x7f; crafted[1] = 0x03;
    crafted[4] = 0x00; crafted[5] = 0x30;                 /* top 6 */
    crafted[8] = 0xff; crafted[9] = 0x1f;                 /* phys6 valid(00), phys7 zero(01) */
    /* phys6 (st0) = 0.0 with tag valid; phys7 (st1) = 1.0 with tag zero */
    crafted[38 + 7] = 0x80; crafted[38 + 8] = 0xff; crafted[38 + 9] = 0x3f;
    __asm {
        frstor crafted
        fnsave image
    }
    dump("E7in ", crafted, 108);
    dump("E7out", image, 108);
    __asm { fninit }
    printf("done\n");
    return 0;
}
