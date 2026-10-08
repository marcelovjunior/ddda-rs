// Applies MtDTI-derived labels (from research/dti/dti_labels.py) to the
// DDDA Ghidra project. Arguments: <labels.csv>
// CSV columns: address,kind,namespace,name,source
// kind=function creates a function if none exists, then names it
// <namespace>::<name>; kind=data creates a label. Names that a user already
// set (SourceType.USER_DEFINED) are never overwritten.
// @category DDDA

import java.io.BufferedReader;
import java.io.FileReader;

import ghidra.app.cmd.disassemble.DisassembleCommand;
import ghidra.app.script.GhidraScript;
import ghidra.app.util.NamespaceUtils;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.Namespace;
import ghidra.program.model.symbol.SourceType;
import ghidra.program.model.symbol.Symbol;
import ghidra.program.model.symbol.SymbolTable;

public class ApplyDtiLabels extends GhidraScript {

    @Override
    protected void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length != 1) {
            throw new IllegalArgumentException("usage: ApplyDtiLabels <labels.csv>");
        }
        SymbolTable st = currentProgram.getSymbolTable();
        int funcsCreated = 0, funcsNamed = 0, labels = 0, skippedUser = 0, failed = 0;
        try (BufferedReader r = new BufferedReader(new FileReader(args[0]))) {
            String line = r.readLine(); // header
            while ((line = r.readLine()) != null) {
                String[] f = line.split(",", -1);
                if (f.length < 4) {
                    continue;
                }
                Address a = toAddr(Long.decode(f[0]));
                String kind = f[1], ns = f[2], name = f[3];
                try {
                    Namespace space = NamespaceUtils.createNamespaceHierarchy(
                        ns, null, currentProgram, SourceType.ANALYSIS);
                    if (kind.equals("function")) {
                        Function fn = getFunctionAt(a);
                        if (fn == null) {
                            if (getInstructionAt(a) == null) {
                                new DisassembleCommand(new AddressSet(a), null, true)
                                    .applyTo(currentProgram, monitor);
                            }
                            fn = createFunction(a, null);
                            if (fn == null) {
                                failed++;
                                continue;
                            }
                            funcsCreated++;
                        }
                        if (fn.getSymbol().getSource() == SourceType.USER_DEFINED) {
                            skippedUser++;
                            continue;
                        }
                        fn.setParentNamespace(space);
                        fn.setName(name, SourceType.ANALYSIS);
                        funcsNamed++;
                    }
                    else {
                        Symbol prim = st.getPrimarySymbol(a);
                        if (prim != null && prim.getSource() == SourceType.USER_DEFINED) {
                            skippedUser++;
                            continue;
                        }
                        Symbol s = st.createLabel(a, name, space, SourceType.ANALYSIS);
                        s.setPrimary();
                        labels++;
                    }
                }
                catch (Exception e) {
                    failed++;
                    if (failed <= 20) {
                        printerr("row failed: " + line + " : " + e.getMessage());
                    }
                }
            }
        }
        println(String.format(
            "ApplyDtiLabels: functions created=%d named=%d, data labels=%d, skipped(user)=%d, failed=%d",
            funcsCreated, funcsNamed, labels, skippedUser, failed));
        analyzeChanges(currentProgram);
    }
}
