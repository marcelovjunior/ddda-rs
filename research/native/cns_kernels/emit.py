import re,sys,struct
sys.path.insert(0,'..')
import harness as h
BOOL={'gt','ge','eq','not'}
def const_bits(name):
    a=int(name[1:],16); return struct.unpack('<I',h.code(a,4))[0]
def emit_kernel(fname, dag, outputs, doc, role_names, trig=True):
    """outputs: list of node ids (returned as an array). Leaves become array inputs.
    Returns (rust_text, leaves) where leaves is the ordered list of leaf names that are memory loads."""
    nodes=dag.nodes
    seen=set(); order=[]
    def visit(n):
        if n in seen: return
        seen.add(n)
        for c in nodes[n][1:]:
            if isinstance(c,int): visit(c)
        order.append(n)
    for o in outputs: visit(o)
    refs={}
    for n in order:
        for c in nodes[n][1:]:
            if isinstance(c,int): refs[c]=refs.get(c,0)+1
    for o in outputs: refs[o]=refs.get(o,0)+1
    leaves=[]; consts={}
    name={}
    for n in order:
        nd=nodes[n]
        if nd[0]=='leaf':
            l=nd[1]
            if l=='0.0': name[n]='0.0_f32'
            elif l=='C15319b8': name[n]='1.0_f32'
            elif l.startswith('C') and re.match(r'^C[0-9a-f]+$',l):
                consts[l]=const_bits(l); name[n]='K_%s'%l[1:].upper()
            else:
                leaves.append(l); name[n]='l[%d]'%(len(leaves)-1)
    lines=[]
    ex={}
    cnt=[0]
    def e(n,top=False):
        if n in ex and not top: return ex[n]
        nd=nodes[n]; o=nd[0]
        if o=='leaf': return name[n]
        a=[e(c) for c in nd[1:]] if o!='sel' else None
        if o in('mul','add','sub','div'):
            return '(%s %s %s)'%(a[0],{'mul':'*','add':'+','sub':'-','div':'/'}[o],a[1])
        if o=='neg': return '(-%s)'%a[0]
        if o=='sqrt': return '%s.sqrt()'%a[0]
        if o=='quiet': return 'quiet(%s)'%a[0]
        if o in('sin','cos'): return 'trig(%s, %s)'%('true' if o=='cos' else 'false',a[0])
        if o=='gt': return '(%s > %s)'%(a[0],a[1])
        if o=='ge': return '(%s >= %s)'%(a[0],a[1])
        if o=='eq': return '(%s == %s)'%(a[0],a[1])
        if o=='not': return '(!%s)'%a[0]
        if o=='sel': return '(if %s { %s } else { %s })'%(e(nd[1]),e(nd[2]),e(nd[3]))
        raise ValueError(o)
    for n in order:
        nd=nodes[n]
        if nd[0]=='leaf': continue
        if refs.get(n,0)>1 or nd[0] in('sin','cos','sqrt','sel'):
            cnt[0]+=1; v=('c%d' if nd[0] in BOOL else 'v%d')%cnt[0]
            ty='bool' if nd[0] in BOOL else 'f32'
            lines.append('    let %s: %s = %s;'%(v,ty,e(n,top=True)))
            ex[n]=v
    outs=[e(o) for o in outputs]
    text='%s\n#[allow(clippy::all, unused_parens)]\npub(super) fn %s(l: &[f32; %d], trig: &mut dyn FnMut(bool, f32) -> f32) -> [f32; %d] {\n%s\n    [%s]\n}\n'%(
        doc,fname,len(leaves),len(outputs),'\n'.join(lines),', '.join(outs))
    return text,leaves,consts

def emit_loader(fname, leaves, doc, roles):
    items=[]
    for l in leaves:
        r,k=l[0],int(l[1:])
        items.append(roles[r].format(k=k, off=4*k))
    return '%s\npub(super) fn %s(m: &dyn Mem, pg: Pages, b: &Bases, s: &Matrix) -> [f32; %d] {\n    let _ = (m, pg, b, s);\n    [\n%s    ]\n}\n'%(doc,fname,len(leaves),''.join('        %s,\n'%i for i in items))

def emit_consts(consts):
    return ''.join('const K_%s: f32 = f32::from_bits(0x%08x);\n'%(k[1:].upper(),v) for k,v in sorted(consts.items()))
