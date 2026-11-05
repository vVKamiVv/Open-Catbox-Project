// Prints the disassembly of <count> instructions starting at each address.
// usage (headless): -postScript Disasm.java <count> <hex address> [<hex address> ...] "<output file>"
//@category Umineko
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Instruction;

import java.io.File;
import java.io.PrintWriter;

public class Disasm extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        int count = Integer.parseInt(args[0]);
        try (PrintWriter out = new PrintWriter(new File(args[args.length - 1]), "UTF-8")) {
            for (int i = 1; i < args.length - 1; i++) {
                Address address = toAddr(args[i]);
                out.println("\n===== " + address);
                Instruction insn = getInstructionAt(address);
                if (insn == null) {
                    disassemble(address);
                    insn = getInstructionAt(address);
                }
                for (int n = 0; n < count && insn != null; n++) {
                    out.println("  " + insn.getAddress() + "  " + insn);
                    insn = insn.getNext();
                }
            }
        }
    }
}
