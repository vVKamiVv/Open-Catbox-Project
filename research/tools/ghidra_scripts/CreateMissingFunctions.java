// Creates the functions the auto-analysis missed, so a whole-program decompile covers the whole engine: every
// pointer into executable code found in the data segments (vtables, std::function tables, jump tables of callbacks),
// every BL target, and every gap between functions in the code that disassembles. Repeats until nothing is added.
// usage (headless, on a copy of the project): -postScript CreateMissingFunctions.java
//@category Umineko
import ghidra.app.cmd.disassemble.DisassembleCommand;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.Listing;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.symbol.FlowType;
import ghidra.program.model.symbol.Reference;

import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Set;

public class CreateMissingFunctions extends GhidraScript {
    private final List<MemoryBlock> code = new ArrayList<>();

    private boolean inCode(Address a) {
        for (MemoryBlock b : code) {
            if (b.contains(a)) {
                return true;
            }
        }
        return false;
    }

    private boolean make(Address a) {
        Listing listing = currentProgram.getListing();
        if (getFunctionAt(a) != null || getFunctionContaining(a) != null) {
            return false;
        }
        if (listing.getInstructionAt(a) == null) {
            DisassembleCommand cmd = new DisassembleCommand(a, null, true);
            cmd.applyTo(currentProgram, monitor);
            if (listing.getInstructionAt(a) == null) {
                return false;
            }
        }
        return createFunction(a, null) != null;
    }

    @Override
    public void run() throws Exception {
        Memory memory = currentProgram.getMemory();
        for (MemoryBlock b : memory.getBlocks()) {
            if (b.isExecute() && b.isInitialized()) {
                code.add(b);
            }
        }
        int total = 0;
        for (int round = 0; round < 8; round++) {
            Set<Address> candidates = new LinkedHashSet<>();
            // pointers into the code from the data
            for (MemoryBlock b : memory.getBlocks()) {
                if (b.isExecute() || !b.isInitialized()) {
                    continue;
                }
                Address a = b.getStart();
                long end = b.getEnd().getOffset();
                while (a.getOffset() + 8 <= end + 1) {
                    long v = memory.getLong(a);
                    if ((v & 3) == 0) {
                        Address t = a.getAddressSpace().getAddress(v, true);
                        if (t != null && inCode(t)) {
                            candidates.add(t);
                        }
                    }
                    a = a.add(8);
                    if (monitor.isCancelled()) {
                        return;
                    }
                }
            }
            // call targets
            for (Instruction i : currentProgram.getListing().getInstructions(true)) {
                FlowType flow = i.getFlowType();
                if (flow.isCall()) {
                    for (Reference r : i.getReferencesFrom()) {
                        if (inCode(r.getToAddress())) {
                            candidates.add(r.getToAddress());
                        }
                    }
                }
            }
            int made = 0;
            for (Address a : candidates) {
                if (make(a)) {
                    made++;
                }
            }
            // gaps between functions that start with code
            for (MemoryBlock b : code) {
                Address a = b.getStart();
                while (a.compareTo(b.getEnd()) < 0) {
                    Function f = getFunctionContaining(a);
                    if (f != null) {
                        Address next = f.getBody().getMaxAddress().add(1);
                        a = next.compareTo(a) > 0 ? next : a.add(4);
                        continue;
                    }
                    if ((a.getOffset() & 3) == 0 && memory.getInt(a) != 0 && make(a)) {
                        made++;
                        continue;
                    }
                    a = a.add(4);
                    if (monitor.isCancelled()) {
                        return;
                    }
                }
            }
            total += made;
            println("Round " + round + ": " + made + " functions created");
            if (made == 0) {
                break;
            }
        }
        println("Created " + total + " functions; now " + currentProgram.getFunctionManager().getFunctionCount());
    }
}
