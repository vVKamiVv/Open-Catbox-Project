// For each address: lists references to it and dumps the 8-byte pointers around it with what they point to.
// Used to walk RTTI -> typeinfo -> vtable -> methods.
// usage (headless): -postScript Refs.java <hex address> [<hex address> ...] "<output file>"
//@category Umineko
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Data;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.Reference;
import ghidra.program.model.symbol.Symbol;

import java.io.File;
import java.io.PrintWriter;

public class Refs extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        try (PrintWriter out = new PrintWriter(new File(args[args.length - 1]), "UTF-8")) {
            for (int i = 0; i < args.length - 1; i++) {
                Address address = toAddr(args[i]);
                out.println("\n===== " + address);
                for (Reference ref : getReferencesTo(address)) {
                    Function f = getFunctionContaining(ref.getFromAddress());
                    out.println("  referenced from " + ref.getFromAddress()
                            + (f == null ? "" : " in " + f.getName() + "@" + f.getEntryPoint()));
                }
                for (int slot = -4; slot < 24; slot++) {
                    Address at = address.add(slot * 8L);
                    long value;
                    try {
                        value = getLong(at);
                    } catch (Exception e) {
                        continue;
                    }
                    Address target = toAddr(value);
                    Function f = getFunctionAt(target);
                    Symbol s = getSymbolAt(target);
                    Data d = getDataAt(target);
                    String what = f != null ? "func " + f.getName()
                            : s != null ? "sym " + s.getName()
                            : d != null && d.hasStringValue() ? "str \"" + d.getValue() + "\""
                            : "";
                    out.printf("  %s [%+d] = %016x %s%n", at, slot * 8, value, what);
                }
            }
        }
    }
}
