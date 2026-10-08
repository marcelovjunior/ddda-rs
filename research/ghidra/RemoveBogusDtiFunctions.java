// Removes functions that an earlier ApplyDtiLabels run created at addresses
// which are not code: vtable runs that overran into the data after them
// (resource extension strings such as 'tex\0', ASCII/UTF-16/UTF-8 text).
// Argument: <bogus.csv> with columns address,namespace,name.
// A function is removed only if it still has exactly the expected name with
// SourceType.ANALYSIS, so user renames are never touched. Instructions in its
// body that no other function owns are cleared as well.
// @category DDDA

import java.io.BufferedReader;
import java.io.FileReader;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressRange;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.address.AddressSetView;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.SourceType;

public class RemoveBogusDtiFunctions extends GhidraScript {

    @Override
    protected void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length != 1) {
            throw new IllegalArgumentException("usage: RemoveBogusDtiFunctions <bogus.csv>");
        }
        int removed = 0, missing = 0, renamed = 0;
        try (BufferedReader r = new BufferedReader(new FileReader(args[0]))) {
            String line = r.readLine(); // header
            while ((line = r.readLine()) != null) {
                String[] f = line.split(",", -1);
                Address a = toAddr(Long.decode(f[0]));
                Function fn = getFunctionAt(a);
                if (fn == null) {
                    missing++;
                    continue;
                }
                if (fn.getSymbol().getSource() != SourceType.ANALYSIS
                        || !fn.getName().equals(f[2])) {
                    renamed++;
                    printerr("kept (renamed since): " + line + " now " + fn.getName(true));
                    continue;
                }
                AddressSetView body = fn.getBody();
                removeFunction(fn);
                AddressSet orphan = new AddressSet();
                for (AddressRange range : body) {
                    for (Address x = range.getMinAddress(); x != null
                            && x.compareTo(range.getMaxAddress()) <= 0; x = x.next()) {
                        if (getFunctionContaining(x) == null) {
                            orphan.add(x);
                        }
                    }
                }
                clearListing(orphan);
                removed++;
            }
        }
        println(String.format("RemoveBogusDtiFunctions: removed=%d missing=%d kept(renamed)=%d",
            removed, missing, renamed));
    }
}
