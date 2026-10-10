// Exports the function inventory of the open program for the systematic
// rebuild tracker (docs/METHODOLOGY.md, Phase 0): one row per function and
// one row per call edge. Read-only; run headless with -readOnly.
// Usage: -postScript ExportInventory.java <output directory>
//@category DDDA

import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import ghidra.program.model.address.*;
import java.io.*;
import java.util.*;

public class ExportInventory extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        File dir = new File(args.length > 0 ? args[0] : ".");
        dir.mkdirs();
        FunctionManager fm = currentProgram.getFunctionManager();
        try (PrintWriter funcs = new PrintWriter(new FileWriter(new File(dir, "functions.tsv")));
             PrintWriter edges = new PrintWriter(new FileWriter(new File(dir, "calls.tsv")))) {
            funcs.println("address\tname\tnamespace\tsize\tthunk\tcallers\tcallees");
            edges.println("caller\tcallee");
            FunctionIterator it = fm.getFunctions(true);
            int n = 0;
            while (it.hasNext() && !monitor.isCancelled()) {
                Function f = it.next();
                Set<Function> callees = f.getCalledFunctions(monitor);
                Set<Function> callers = f.getCallingFunctions(monitor);
                String ns = f.getParentNamespace() == null ? "" : f.getParentNamespace().getName(true);
                funcs.printf("0x%08X\t%s\t%s\t%d\t%b\t%d\t%d%n",
                    f.getEntryPoint().getOffset(), f.getName(), ns,
                    f.getBody().getNumAddresses(), f.isThunk(), callers.size(), callees.size());
                for (Function c : callees) {
                    edges.printf("0x%08X\t0x%08X%n", f.getEntryPoint().getOffset(), c.getEntryPoint().getOffset());
                }
                n++;
            }
            println("ExportInventory: " + n + " functions to " + dir);
        }
    }
}
