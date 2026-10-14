"""Kernels of other float functions ported by s5 (0x00DEB960 ...)."""
import sys; sys.path.insert(0,'.')
from sym import *
from emit import *
parts=[]; allconsts={}
which=sys.argv[1:] or ['deb960']
if 'deb960' in which:
    d=Dag(); S=Sym(d,'q_00deb960.asm',{'ECX':'Q'})
    S.run(S.st,0xdeb96a,0xdeba52)
    S.run(S.st,0xdeba58,0xdebaaa)
    outs=[S.st.slot[0x10+4*k] for k in range(16)]   # the matrix at [esp+0x10..]
    txt,leaves,consts=emit_kernel('kernel_quaternion_matrix',d,outs,'/// Kernel of 0x00DEB960: the rotation matrix of the quaternion at this+0x50 (x, y, z, w).',{})
    allconsts.update(consts)
    roles={'Q':'rd(m, pg, b.this + {off})'}
    parts.append(emit_loader('load_quaternion_matrix',leaves,'/// Inputs of `kernel_quaternion_matrix`.',roles)); parts.append(txt)
    print('deb960',len(d.nodes),leaves)
open('k_misc.rs','w').write(emit_consts(allconsts)+'\n'+'\n'.join(parts))
