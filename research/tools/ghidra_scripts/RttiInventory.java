// Lists every C++ class that has RTTI: demangled name, typeinfo address, base typeinfo, vtable address(es) and the
// virtual methods of the primary vtable. Output is tab-separated for the docs and for symbol generation.
// usage (headless): -postScript RttiInventory.java "<output file>"
//@category Umineko
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Data;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.symbol.Reference;

import java.io.File;
import java.io.PrintWriter;
import java.util.ArrayList;
import java.util.List;

public class RttiInventory extends GhidraScript {
    private int pos;

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        Memory memory = currentProgram.getMemory();
        try (PrintWriter out = new PrintWriter(new File(args[args.length - 1]), "UTF-8")) {
            out.println("class\tmangled\ttypeinfo\tbase_typeinfo\tvtables\tvirtual_methods");
            for (Data data : currentProgram.getListing().getDefinedData(true)) {
                if (!data.hasStringValue() || data.getValue() == null) {
                    continue;
                }
                String mangled = data.getValue().toString();
                String name = demangle(mangled);
                if (name == null) {
                    continue;
                }
                for (Reference ref : getReferencesTo(data.getAddress())) {
                    // typeinfo layout: [vtable-for-typeinfo ptr][name ptr][base typeinfo ptr...]
                    Address typeinfo = ref.getFromAddress().subtract(8);
                    String base = "";
                    try {
                        long b = getLong(typeinfo.add(16));
                        if (b > 0x7100000000L && b < 0x7200000000L) {
                            base = Long.toHexString(b);
                        }
                    } catch (Exception ignored) {
                    }
                    List<String> vtables = new ArrayList<>();
                    String methods = "";
                    byte[] pattern = new byte[8];
                    long t = typeinfo.getOffset();
                    for (int b = 0; b < 8; b++) {
                        pattern[b] = (byte) (t >>> (8 * b));
                    }
                    Address hit = memory.findBytes(currentProgram.getMinAddress(), pattern, null, true, monitor);
                    while (hit != null) {
                        if (!hit.equals(ref.getFromAddress().subtract(8))) {
                            Address vtable = hit.add(8);
                            vtables.add(vtable.toString());
                            if (methods.isEmpty()) {
                                methods = listMethods(vtable);
                            }
                        }
                        hit = memory.findBytes(hit.add(1), pattern, null, true, monitor);
                    }
                    out.println(name + "\t" + mangled + "\t" + typeinfo + "\t" + base + "\t"
                            + String.join(",", vtables) + "\t" + methods);
                }
            }
        }
    }

    /** Collects code pointers from a vtable until the next non-code entry (typeinfo, offset or data). */
    private String listMethods(Address vtable) {
        List<String> methods = new ArrayList<>();
        for (int i = 0; i < 64; i++) {
            try {
                long v = getLong(vtable.add(i * 8L));
                Address target = toAddr(v);
                if (v == 0 || currentProgram.getMemory().getBlock(target) == null
                        || !currentProgram.getMemory().getBlock(target).isExecute()) {
                    break;
                }
                methods.add(Long.toHexString(v));
            } catch (Exception e) {
                break;
            }
        }
        return String.join(",", methods);
    }

    /** Minimal Itanium demangler for typeinfo names: nested names (N...E) and plain source names. */
    private String demangle(String s) {
        pos = 0;
        try {
            String result;
            if (s.startsWith("N")) {
                pos = 1;
                List<String> parts = new ArrayList<>();
                while (pos < s.length() && s.charAt(pos) != 'E') {
                    if (s.startsWith("St", pos)) {
                        parts.add("std");
                        pos += 2;
                        continue;
                    }
                    if (!Character.isDigit(s.charAt(pos))) {
                        return null;
                    }
                    parts.add(sourceName(s));
                }
                if (pos != s.length() - 1 || parts.size() < 2) {
                    return null;
                }
                result = String.join("::", parts);
            } else if (!s.isEmpty() && Character.isDigit(s.charAt(0))) {
                result = sourceName(s);
                if (pos != s.length()) {
                    return null;
                }
            } else {
                return null;
            }
            return result.matches("[A-Za-z_][A-Za-z0-9_:]*") ? result : null;
        } catch (Exception e) {
            return null;
        }
    }

    private String sourceName(String s) {
        int start = pos;
        while (Character.isDigit(s.charAt(pos))) {
            pos++;
        }
        int len = Integer.parseInt(s.substring(start, pos));
        String name = s.substring(pos, pos + len);
        pos += len;
        return name;
    }
}
