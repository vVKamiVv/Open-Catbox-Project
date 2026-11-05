// Writes a raw range of the program's memory to a binary file (tables too big for DumpData.java).
// usage (headless): -postScript DumpBytes.java <hex address> <length> "<output file>"
// The output is part of the game executable: keep it out of the repository.
//@category Umineko
import ghidra.app.script.GhidraScript;

import java.io.FileOutputStream;

public class DumpBytes extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        byte[] bytes = getBytes(toAddr(args[0]), Integer.decode(args[1]));
        try (FileOutputStream out = new FileOutputStream(args[2])) {
            out.write(bytes);
        }
        println("wrote " + bytes.length + " bytes to " + args[2]);
    }
}
