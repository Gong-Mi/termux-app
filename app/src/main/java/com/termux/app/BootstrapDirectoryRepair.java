package com.termux.app;

import java.io.File;
import java.io.IOException;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.List;

/** Repairs only missing runtime directories; never extracts, deletes or chmods user data. */
final class BootstrapDirectoryRepair {
    private BootstrapDirectoryRepair() { }

    static void ensure(File prefix) throws IOException {
        if (!prefix.isDirectory()) throw new IOException("Existing prefix is not a directory: " + prefix);
        String root = prefix.getCanonicalPath() + File.separator;
        List<File> missing = new ArrayList<>();
        // These are runtime requirements, not a request to restore the whole bootstrap.
        for (String relative : new String[]{"tmp", "etc/apt/apt.conf.d"}) {
            File target = new File(prefix, relative);
            // An existing user-provided directory, including a valid symlink, is left intact.
            if (target.isDirectory()) continue;
            if (target.exists() || Files.isSymbolicLink(target.toPath()))
                throw new IOException("Refusing to replace existing path: " + target);
            if (!target.getCanonicalPath().startsWith(root))
                throw new IOException("Refusing to create outside prefix: " + target);
            missing.add(target);
        }
        // Validate all known conflicts before making any changes. Filesystem failures
        // can still leave some newly-created directories, but never trigger rollback/deletion.
        for (File target : missing) {
            Files.createDirectories(target.toPath());
            if (!target.isDirectory()) throw new IOException("Required directory unavailable: " + target);
        }
    }
}
